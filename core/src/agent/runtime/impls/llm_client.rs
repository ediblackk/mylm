//! LLM Capability implementation using existing LlmClient
//!
//! Bridges V3 architecture with existing LLM infrastructure.

use crate::agent::runtime::{
    capability::{Capability, LLMCapability, StreamChunk},
    context::RuntimeContext,
    error::LLMError,
};
use crate::agent::types::intents::LLMRequest;
use crate::agent::types::events::LLMResponse;
use crate::llm::LlmClient;
use crate::llm::chat::{ChatRequest, ChatMessage};
use std::sync::Arc;
use std::pin::Pin;
use futures::{Stream, StreamExt};

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
        // Build messages from context
        let mut messages = vec![];
        if !req.context.system_prompt.is_empty() {
            messages.push(ChatMessage::system(req.context.system_prompt.clone()));
        }
        messages.push(ChatMessage::user(req.context.scratchpad.clone()));
        
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
        // Build messages from context
        let mut messages = vec![];
        if !req.context.system_prompt.is_empty() {
            messages.push(ChatMessage::system(req.context.system_prompt.clone()));
        }
        messages.push(ChatMessage::user(req.context.scratchpad.clone()));
        
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
                    Ok(crate::llm::chat::StreamEvent::Content(content)) => {
                        yield StreamChunk {
                            content,
                            is_final: false,
                        };
                    }
                    Ok(crate::llm::chat::StreamEvent::Done) => {
                        yield StreamChunk {
                            content: String::new(),
                            is_final: true,
                        };
                        break;
                    }
                    Ok(crate::llm::chat::StreamEvent::Usage(_)) => {
                        // Usage info at end, ignore for now
                    }
                    Ok(crate::llm::chat::StreamEvent::Error(msg)) => {
                        crate::error_log!("[LLM_CLIENT] Stream error from provider: {}", msg);
                        // Don't fail - stream might have produced valid content already
                        break;
                    }
                    Err(e) => {
                        crate::error_log!("[LLM_CLIENT] Stream error: {:?}", e);
                        // Don't fail - stream might have produced valid content already
                        break;
                    }
                }
            }
        })
    }
}
