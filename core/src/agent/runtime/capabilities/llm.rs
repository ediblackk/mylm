//! LLM Capability implementation using existing LlmClient
//!
//! Bridges V3 architecture with existing LLM infrastructure.
//! 
//! # Context Management
//! 
//! This capability enforces context size limits at the LLM boundary:
//! 1. Receives raw history from Intent::RequestLLM
//! 2. Uses ContextManager to prune/condense if needed
//! 3. Fails fast if context still exceeds limits after pruning
//! 4. Logs metrics for debugging

use crate::agent::runtime::core::{
    Capability, LLMCapability, StreamChunk, RuntimeContext, LLMError,
};
use crate::agent::types::intents::LLMRequest;
use crate::agent::types::events::LLMResponse;
use crate::agent::memory::MemoryProvider;
use crate::conversation::ContextManager;
use crate::provider::LlmClient;
use crate::provider::chat::{ChatRequest, ChatMessage};
use std::sync::Arc;
use std::pin::Pin;
use futures::{Stream, StreamExt};
use tracing::{debug, info, warn};

/// LLM capability backed by existing LlmClient
/// 
/// Enforces context size limits via ContextManager integration.
pub struct LlmClientCapability {
    client: Arc<LlmClient>,
    memory_provider: Option<Arc<dyn MemoryProvider>>,
    context_manager: Arc<tokio::sync::Mutex<ContextManager>>,
}

impl LlmClientCapability {
    /// Create new LLM capability with ContextManager for size enforcement
    pub fn new(
        client: Arc<LlmClient>,
        context_manager: Arc<tokio::sync::Mutex<ContextManager>>,
    ) -> Self {
        Self { 
            client,
            memory_provider: None,
            context_manager,
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
    async fn build_messages_from_context(&self, req: &LLMRequest) -> Vec<ChatMessage> {
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
            // Convert history from intents::Message (deprecated) to canonical Message
            let canonical_history: Vec<crate::conversation::manager::Message> = req.context.history
                .iter()
                .map(|m| m.clone().into())
                .collect();
            
            let memory_context = provider.build_context(
                &canonical_history,
                &req.context.scratchpad,
                &req.context.system_prompt
            ).await;
            
            if !memory_context.is_empty() {
                system_parts.push(memory_context);
            }
        }
        
        // Add main system prompt
        if !req.context.system_prompt.is_empty() {
            system_parts.push(req.context.system_prompt.clone());
        }
        
        // Note: Tool descriptions are embedded in the main system prompt (system.rs).
        // We do NOT re-inject available_tools here — doing so causes models like Hermes/Mixtral
        // to activate XML tool-calling format, which breaks our ShortKey JSON parser.
        
        // Combine all system parts
        if !system_parts.is_empty() {
            let full_system_prompt = system_parts.join("\n\n");
            messages.push(ChatMessage::system(full_system_prompt));
        }
        
        // 2. Add conversation history
        // Note: Tool messages are sent as user messages since we use Short-Key JSON
        // protocol instead of OpenAI's native tool calling API
        for msg in &req.context.history {
            let chat_msg = match msg.role.as_str() {
                "user" => ChatMessage::user(msg.content.clone()),
                "assistant" => ChatMessage::assistant(msg.content.clone()),
                "system" => ChatMessage::system(msg.content.clone()),
                "tool" => ChatMessage::user(msg.content.clone()), // Tool results as user messages
                _ => ChatMessage::user(msg.content.clone()),
            };
            messages.push(chat_msg);
        }
        
        // 3. Add current scratchpad as user message
        if !req.context.scratchpad.is_empty() {
            messages.push(ChatMessage::user(req.context.scratchpad.clone()));
        }
        
        messages
    }
    
    /// Prepare context with size enforcement
    /// 
    /// 1. Builds complete system prompt INCLUDING memory context
    /// 2. Adds system prompt as first message in history
    /// 3. Calls prepare_context() for pruning/condensation (respects token limits)
    /// 4. Returns sized messages ready for LLM API
    /// 
    /// CRITICAL: Memory context is built BEFORE pruning so it's included in
    /// token limit calculations. This prevents context overflow when memory
    /// contains large content (e.g., PDF chunks).
    ///
    /// # Errors
    /// Returns LLMError if context exceeds limits even after pruning
    async fn prepare_sized_context(
        &self,
        req: &LLMRequest,
    ) -> Result<Vec<ChatMessage>, LLMError> {
        let mut cm = self.context_manager.lock().await;
        
        // === STEP 1: Build system prompt WITH memory BEFORE pruning ===
        let mut system_parts: Vec<String> = vec![];
        
        // Add extra system messages first
        for msg in &req.extra_system_messages {
            if !msg.is_empty() {
                system_parts.push(msg.clone());
            }
        }
        
        // Inject memory context BEFORE pruning (critical fix)
        if let Some(ref provider) = self.memory_provider {
            let canonical_history: Vec<crate::conversation::manager::Message> = req.context.history
                .iter()
                .map(|m| m.clone().into())
                .collect();
            
            let memory_context = provider.build_context(
                &canonical_history,
                &req.context.scratchpad,
                &req.context.system_prompt
            ).await;
            
            if !memory_context.is_empty() {
                crate::debug_log!("[LLM] Injecting {} bytes of memory context", memory_context.len());
                system_parts.push(memory_context);
            }
        }
        
        // Add main system prompt
        if !req.context.system_prompt.is_empty() {
            system_parts.push(req.context.system_prompt.clone());
        }
        
        // Note: Tool descriptions are embedded in the main system prompt (system.rs).
        // We do NOT re-inject available_tools here — doing so causes models like Hermes/Mixtral
        // to activate XML tool-calling format, which breaks our ShortKey JSON parser.
        
        // Combine system parts into single system message
        let system_message = if !system_parts.is_empty() {
            Some(ChatMessage::system(system_parts.join("\n\n")))
        } else {
            None
        };
        
        // === STEP 2: Build full message list including system message ===
        let mut full_messages: Vec<ChatMessage> = vec![];
        
        // System message FIRST (if any)
        if let Some(sys_msg) = system_message {
            full_messages.push(sys_msg);
        }
        
        // Add conversation history
        for msg in &req.context.history {
            let chat_msg = match msg.role.as_str() {
                "user" => ChatMessage::user(msg.content.clone()),
                "assistant" => ChatMessage::assistant(msg.content.clone()),
                "system" => ChatMessage::system(msg.content.clone()),
                "tool" => ChatMessage::user(msg.content.clone()),
                _ => ChatMessage::user(msg.content.clone()),
            };
            full_messages.push(chat_msg);
        }
        
        // Add scratchpad
        if !req.context.scratchpad.is_empty() {
            full_messages.push(ChatMessage::user(req.context.scratchpad.clone()));
        }
        
        // === STEP 3: Set full history in context manager and prune ===
        cm.set_history(&full_messages);
        
        // Get pre-pruning metrics
        let (before_tokens, max_tokens) = cm.get_cached_token_usage();
        let (before_bytes, max_bytes) = cm.get_byte_usage();
        
        debug!(
            "Context before pruning: {} tokens, {} bytes (max: {} tokens, {} bytes)",
            before_tokens, before_bytes, max_tokens, max_bytes
        );
        
        // Prune context (now includes memory in token count!)
        let pruned = match cm.prepare_context(None).await {
            Ok(msgs) => msgs,
            Err(e) => {
                warn!("Context preparation failed: {}", e);
                // Fall back to raw history if pruning fails
                return Ok(self.build_messages_from_context(req).await);
            }
        };
        
        // Get post-pruning metrics
        let (after_tokens, _) = cm.get_cached_token_usage();
        let (after_bytes, _) = cm.get_byte_usage();
        
        info!(
            "Context prepared: {} -> {} tokens, {} -> {} bytes (saved: {} tokens, {} bytes)",
            before_tokens, after_tokens, before_bytes, after_bytes,
            before_tokens.saturating_sub(after_tokens),
            before_bytes.saturating_sub(after_bytes)
        );
        
        // === STEP 4: Return pruned messages ===
        // The system message with memory is now part of pruned messages
        // and respects the token limit
        
        // Final size check - fail fast if still too large
        let total_bytes: usize = pruned.iter()
            .map(|m| m.content.len())
            .sum();
        
        if total_bytes > max_bytes {
            return Err(LLMError::new(format!(
                "Context too large after pruning: {} bytes (max: {})",
                total_bytes, max_bytes
            )));
        }
        
        // Return pruned messages (system message with memory is included)
        Ok(pruned)
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
        // Prepare sized context (prunes/condenses if needed)
        let messages = self.prepare_sized_context(&req).await?;
        
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
        Box::pin(async_stream::try_stream! {
            // Prepare sized context (prunes/condenses if needed)
            let messages = match self.prepare_sized_context(&req).await {
                Ok(msgs) => msgs,
                Err(e) => {
                    Err(e)?;
                    return; // unreachable, but satisfies type checker
                }
            };
            
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
            
            let mut stream = self.client.chat_stream(&chat_request);
            let mut accumulated_usage: Option<crate::agent::types::events::TokenUsage> = None;
            
            while let Some(event) = stream.next().await {
                match event {
                    Ok(crate::provider::chat::StreamEvent::Content(content)) => {
                        yield StreamChunk {
                            content,
                            is_final: false,
                            usage: None,
                        };
                    }
                    Ok(crate::provider::chat::StreamEvent::Done) => {
                        yield StreamChunk {
                            content: String::new(),
                            is_final: true,
                            usage: accumulated_usage.clone(),
                        };
                        break;
                    }
                    Ok(crate::provider::chat::StreamEvent::Usage(usage)) => {
                        // Accumulate usage for final reporting
                        accumulated_usage = Some(crate::agent::types::events::TokenUsage {
                            prompt_tokens: usage.prompt_tokens,
                            completion_tokens: usage.completion_tokens,
                            total_tokens: usage.total_tokens,
                        });
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
