//! LLM Capability implementation using existing LlmClient
//!
//! Bridges V3 architecture with existing LLM infrastructure.

use crate::agent::runtime::core::{
    Capability, LLMCapability, StreamChunk, RuntimeContext, LLMError,
};
use crate::agent::types::intents::{LLMRequest, Role};
use crate::agent::types::events::LLMResponse;
use crate::agent::memory::MemoryProvider;
use crate::provider::LlmClient;
use crate::provider::chat::{ChatRequest, ChatMessage};
use std::sync::Arc;
use std::pin::Pin;
use futures::{Stream, StreamExt};

/// LLM capability backed by existing LlmClient
pub struct LlmClientCapability {
    client: Arc<LlmClient>,
    memory_provider: Option<Arc<dyn MemoryProvider>>,
}

impl LlmClientCapability {
    pub fn new(client: Arc<LlmClient>) -> Self {
        Self { 
            client,
            memory_provider: None,
        }
    }
    
    /// Set the memory provider for context augmentation
    pub fn with_memory_provider(mut self, provider: Arc<dyn MemoryProvider>) -> Self {
        self.memory_provider = Some(provider);
        self
    }
    
    /// Build messages from LLMRequest context
    /// 
    /// Constructs a complete message history including:
    /// - System prompt (with available tools appended, and memory context if available)
    /// - Conversation history
    /// - Current scratchpad content
    fn build_messages_from_context(&self, req: &LLMRequest) -> Vec<ChatMessage> {
        let mut messages = vec![];
        
        // 1. Build system prompt with extra messages, tools and memory
        let mut system_parts: Vec<String> = vec![];
        
        // Add extra system messages first (e.g., format correction)
        for msg in &req.extra_system_messages {
            if !msg.is_empty() {
                system_parts.push(msg.clone());
            }
        }
        
        // Inject memory context if provider is available
        if let Some(ref provider) = self.memory_provider {
            let memory_context = provider.build_context(
                &req.context.history,
                &req.context.scratchpad,
                &req.context.system_prompt
            );
            
            if !memory_context.is_empty() {
                system_parts.push(memory_context);
            }
        }
        
        // Add main system prompt
        if !req.context.system_prompt.is_empty() {
            system_parts.push(req.context.system_prompt.clone());
        }
        
        // Add tools section
        if !req.context.available_tools.is_empty() {
            let mut tools_section = "=== AVAILABLE TOOLS ===\n".to_string();
            for tool in &req.context.available_tools {
                if let Some(usage) = &tool.usage {
                    tools_section.push_str(&format!("- {}: {} (Usage: {})\n", tool.name, tool.description, usage));
                } else {
                    tools_section.push_str(&format!("- {}: {}\n", tool.name, tool.description));
                }
            }
            tools_section.push_str("\nUse these tools with the Short-Key JSON format: {\"a\": \"tool_name\", \"i\": {...}}");
            system_parts.push(tools_section);
        }
        
        // Combine all system parts
        if !system_parts.is_empty() {
            let full_system_prompt = system_parts.join("\n\n");
            messages.push(ChatMessage::system(full_system_prompt));
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
        let messages = self.build_messages_from_context(&req);
        
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
        let messages = self.build_messages_from_context(&req);
        
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
