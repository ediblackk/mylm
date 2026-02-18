//! LLM Capability implementation using existing LlmClient
//!
//! Bridges V3 architecture with existing LLM infrastructure.

use crate::agent::runtime::{
    capability::{Capability, LLMCapability, StreamChunk},
    context::RuntimeContext,
    error::LLMError,
};
use crate::agent::types::intents::{LLMRequest, Role};
use crate::agent::types::events::LLMResponse;
use crate::provider::LlmClient;
use crate::provider::chat::{ChatRequest, ChatMessage};
use std::sync::Arc;
use std::pin::Pin;
use futures::{Stream, StreamExt};

/// Build messages from LLMRequest context
/// 
/// Constructs a complete message history including:
/// - System prompt (with available tools appended)
/// - Conversation history
/// - Current scratchpad content
fn build_messages_from_context(req: &LLMRequest) -> Vec<ChatMessage> {
    let mut messages = vec![];
    
    // 1. Build system prompt with tools
    let mut system_prompt = req.context.system_prompt.clone();
    if !req.context.available_tools.is_empty() {
        system_prompt.push_str("\n\n=== AVAILABLE TOOLS ===\n");
        for tool in &req.context.available_tools {
            system_prompt.push_str(&format!("- {}: {}\n", tool.name, tool.description));
        }
        system_prompt.push_str("\nUse these tools with the Short-Key JSON format: {\"a\": \"tool_name\", \"i\": {...}}");
    }
    if !system_prompt.is_empty() {
        messages.push(ChatMessage::system(system_prompt));
    }
    
    // 2. Add conversation history
    // Note: Tool messages are sent as user messages since we use Short-Key JSON
    // protocol instead of OpenAI's native tool calling API
    for msg in &req.context.history {
        let chat_msg = match msg.role {
            Role::User => ChatMessage::user(msg.content.clone()),
            Role::Assistant => ChatMessage::assistant(msg.content.clone()),
            Role::System => ChatMessage::system(msg.content.clone()),
            Role::Tool => ChatMessage::user(msg.content.clone()), // Tool results as user messages
        };
        messages.push(chat_msg);
    }
    
    // 3. Add current scratchpad as user message
    if !req.context.scratchpad.is_empty() {
        messages.push(ChatMessage::user(req.context.scratchpad.clone()));
    }
    
    messages
}

/// LLM capability backed by existing LlmClient
pub struct LlmClientCapability {
    client: Arc<LlmClient>,
}

impl LlmClientCapability {
    pub fn new(client: Arc<LlmClient>) -> Self {
        Self { client }
    }
}

impl Capability for LlmClientCapability {
    fn name(&self) -> &'static str {
        "llm-client"
    }
}

#[async_trait::async_trait]
impl LLMCapability for LlmClientCapability {
    async fn complete(
        &self,
        _ctx: &RuntimeContext,
        req: LLMRequest,
    ) -> Result<LLMResponse, LLMError> {
        // Build messages from context (system prompt + history + scratchpad)
        let messages = build_messages_from_context(&req);
        
        let chat_request = ChatRequest {
            model: req.model.unwrap_or_default(), // Will be filled by LlmClient from its config if empty
            messages,
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            stream: req.stream,
            stop: None,
            tools: None,
            response_format: None,
        };

        match self.client.chat(&chat_request).await {
            Ok(response) => Ok(LLMResponse {
                content: response.choices
                    .first()
                    .map(|c| c.message.content.clone())
                    .unwrap_or_default(),
                usage: crate::agent::types::events::TokenUsage::new(
                    response.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0) as u32,
                    response.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0) as u32,
                ),
                model: "unknown".to_string(),
                provider: "unknown".to_string(),
                finish_reason: crate::agent::types::events::FinishReason::Stop,
                structured: None,
            }),
            Err(e) => Err(LLMError::new(e.to_string())),
        }
    }
    
    fn complete_stream<'a>(
        &'a self,
        _ctx: &'a RuntimeContext,
        req: LLMRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk, LLMError>> + Send + 'a>> {
        // Build messages from context (system prompt + history + scratchpad)
        let messages = build_messages_from_context(&req);
        
        let chat_request = ChatRequest {
            model: req.model.unwrap_or_default(),
            messages,
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            stream: true,
            stop: None,
            tools: None,
            response_format: None,
        };
        
        Box::pin(async_stream::try_stream! {
            let mut stream = self.client.chat_stream(&chat_request);
            
            while let Some(event) = stream.next().await {
                match event {
                    Ok(crate::provider::chat::StreamEvent::Content(content)) => {
                        yield StreamChunk {
                            content,
                            is_final: false,
                        };
                    }
                    Ok(crate::provider::chat::StreamEvent::Done) => {
                        yield StreamChunk {
                            content: String::new(),
                            is_final: true,
                        };
                        break;
                    }
                    Ok(crate::provider::chat::StreamEvent::Usage(_)) => {
                        // Usage info at end, ignore for now
                    }
                    Ok(crate::provider::chat::StreamEvent::Error(msg)) => {
                        crate::error_log!("[LLM_CLIENT] Stream error from provider: {}", msg);
                        // Yield error so fallback can be triggered
                        Err(LLMError::new(msg))?;
                    }
                    Err(e) => {
                        crate::error_log!("[LLM_CLIENT] Stream error: {:?}", e);
                        // Yield error so fallback can be triggered
                        Err(LLMError::new(e.to_string()))?;
                    }
                }
            }
        })
    }
}
