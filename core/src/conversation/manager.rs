//! Context Manager Module
//!
//! Encapsulates all context logic including token counting, compression, condensation,
//! and UI formatting for conversation history management.

use crate::provider::chat::{ChatMessage, MessageRole};
use crate::provider::LlmClient;
use crate::ui::action_stamp::{ActionStamp, ActionStampRegistry};
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use std::sync::Arc;

/// Configuration for context management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Maximum tokens allowed in context
    pub max_tokens: usize,
    /// Threshold ratio (0.0 - 1.0) at which to trigger condensation
    pub condense_threshold: f64,
    /// Maximum tokens for LLM output/reserve
    pub max_output_tokens: usize,
    /// Input price per 1M tokens (for cost calculation)
    pub input_price_per_million: f64,
    /// Output price per 1M tokens (for cost calculation)
    pub output_price_per_million: f64,
    /// Maximum total byte size for the context (API safety limit)
    /// Default is 3MB to stay safely under typical 4MB API limits
    pub max_bytes: usize,
}

/// Context transformation metrics for debugging and monitoring
/// 
/// Tracks how context changes during preparation (compression, condensation)
#[derive(Debug, Clone)]
pub struct ContextMetrics {
    /// Original number of messages before processing
    pub original_messages: usize,
    /// Final number of messages after processing
    pub pruned_messages: usize,
    /// Original token count
    pub original_tokens: usize,
    /// Final token count
    pub pruned_tokens: usize,
    /// Original byte size
    pub original_bytes: usize,
    /// Final byte size
    pub pruned_bytes: usize,
    /// Whether compression occurred
    pub was_compressed: bool,
    /// Whether condensation occurred
    pub was_condensed: bool,
    /// Timestamp of the operation
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ContextMetrics {
    /// Create new metrics from before/after state
    pub fn new(
        original_messages: usize,
        pruned_messages: usize,
        original_tokens: usize,
        pruned_tokens: usize,
        original_bytes: usize,
        pruned_bytes: usize,
        was_condensed: bool,
    ) -> Self {
        Self {
            original_messages,
            pruned_messages,
            original_tokens,
            pruned_tokens,
            original_bytes,
            pruned_bytes,
            was_compressed: original_messages > pruned_messages,
            was_condensed,
            timestamp: chrono::Utc::now(),
        }
    }
    
    /// Messages removed
    pub fn messages_removed(&self) -> usize {
        self.original_messages.saturating_sub(self.pruned_messages)
    }
    
    /// Tokens saved
    pub fn tokens_saved(&self) -> usize {
        self.original_tokens.saturating_sub(self.pruned_tokens)
    }
    
    /// Bytes saved
    pub fn bytes_saved(&self) -> usize {
        self.original_bytes.saturating_sub(self.pruned_bytes)
    }
    
    /// Format for logging
    pub fn format_log(&self) -> String {
        format!(
            "ContextMetrics: {}→{} msgs ({} removed), {}→{} tokens ({} saved), {}→{} bytes ({} saved), condensed={}",
            self.original_messages,
            self.pruned_messages,
            self.messages_removed(),
            self.original_tokens,
            self.pruned_tokens,
            self.tokens_saved(),
            self.original_bytes,
            self.pruned_bytes,
            self.bytes_saved(),
            self.was_condensed
        )
    }
}

/// History inspection snapshot for debugging
#[derive(Debug, Clone)]
pub struct HistorySnapshot {
    /// Total messages in history
    pub message_count: usize,
    /// Total tokens
    pub total_tokens: usize,
    /// Total bytes
    pub total_bytes: usize,
    /// Breakdown by role
    pub by_role: std::collections::HashMap<String, usize>,
    /// Recent message previews (last 5)
    pub recent_previews: Vec<(String, String)>, // (role, preview)
}

impl HistorySnapshot {
    /// Format for display/logging
    pub fn format(&self) -> String {
        let mut output = format!(
            "HistorySnapshot: {} messages, {} tokens, {} bytes\n",
            self.message_count, self.total_tokens, self.total_bytes
        );
        
        output.push_str("By role:\n");
        for (role, count) in &self.by_role {
            output.push_str(&format!("  {}: {}\n", role, count));
        }
        
        if !self.recent_previews.is_empty() {
            output.push_str("Recent messages:\n");
            for (i, (role, preview)) in self.recent_previews.iter().enumerate() {
                output.push_str(&format!("  [{}] {}: {}...\n", i, role, preview));
            }
        }
        
        output
    }
}

impl ContextConfig {
    /// Create a new context config with sensible defaults
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            condense_threshold: 0.8,
            max_output_tokens: 4096,
            input_price_per_million: 0.0,
            output_price_per_million: 0.0,
            max_bytes: 3 * 1024 * 1024, // 3MB default
        }
    }

    /// Set the condensation threshold
    pub fn with_condense_threshold(mut self, threshold: f64) -> Self {
        self.condense_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set the maximum output tokens (reserve)
    pub fn with_max_output_tokens(mut self, tokens: usize) -> Self {
        self.max_output_tokens = tokens;
        self
    }

    /// Set pricing for cost calculation
    pub fn with_pricing(mut self, input_price: f64, output_price: f64) -> Self {
        self.input_price_per_million = input_price;
        self.output_price_per_million = output_price;
        self
    }

    /// Set the maximum byte size limit
    pub fn with_max_bytes(mut self, bytes: usize) -> Self {
        self.max_bytes = bytes;
        self
    }

    /// Calculate the effective context limit (max_tokens - reserve)
    pub fn effective_limit(&self) -> usize {
        self.max_tokens.saturating_sub(self.max_output_tokens)
    }

    /// Calculate cost for given token usage
    pub fn calculate_cost(&self, input_tokens: usize, output_tokens: usize) -> f64 {
        let input_cost = input_tokens as f64 * (self.input_price_per_million / 1_000_000.0);
        let output_cost = output_tokens as f64 * (self.output_price_per_million / 1_000_000.0);
        input_cost + output_cost
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 128_000,
            condense_threshold: 0.8,
            max_output_tokens: 4096,
            input_price_per_million: 0.0,
            output_price_per_million: 0.0,
            max_bytes: 3 * 1024 * 1024, // 3MB default
        }
    }
}

/// A message in the conversation history with token tracking
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Role of the message sender ("system", "user", "assistant", "tool")
    pub role: String,
    /// Content of the message
    pub content: String,
    /// Pre-calculated token count
    #[serde(default = "default_token_count")]
    pub token_count: usize,
    /// Byte size of the content (for API limit enforcement)
    #[serde(default = "default_byte_size")]
    pub byte_size: usize,
}

fn default_token_count() -> usize {
    0
}

fn default_byte_size() -> usize {
    0
}

impl Message {
    /// Create a new message with token estimation
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        let content_str = content.into();
        let token_count = TokenCounter::estimate(&content_str);
        let byte_size = content_str.len(); // UTF-8 byte length
        Self {
            role: role.into(),
            content: content_str,
            token_count,
            byte_size,
        }
    }

    /// Convert from ChatMessage
    pub fn from_chat_message(msg: &ChatMessage) -> Self {
        let role = match msg.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        Self::new(role, &msg.content)
    }

    /// Convert back to ChatMessage
    pub fn to_chat_message(&self) -> ChatMessage {
        match self.role.as_str() {
            "system" => ChatMessage::system(&self.content),
            "user" => ChatMessage::user(&self.content),
            "assistant" => ChatMessage::assistant(&self.content),
            "tool" => {
                // For tool messages, we need to extract tool_call_id if stored
                // For now, create with empty tool_call_id
                ChatMessage::tool("unknown", "unknown", &self.content)
            }
            _ => ChatMessage::user(&self.content),
        }
    }
}

/// Convert from ChatMessage to canonical Message
impl From<ChatMessage> for Message {
    fn from(msg: ChatMessage) -> Self {
        Self::from_chat_message(&msg)
    }
}

/// Simple token counter using character-based estimation
#[derive(Debug, Clone, Default)]
pub struct TokenCounter;

impl TokenCounter {
    /// Estimate token count from text
    /// Uses simple heuristic: ~4 characters per token for English text
    pub fn estimate(text: &str) -> usize {
        text.chars().count() / 4 + 1
    }

    /// Estimate tokens for a slice of ChatMessages
    pub fn estimate_messages(messages: &[ChatMessage]) -> usize {
        messages.iter()
            .map(|m| Self::estimate(&m.content))
            .sum()
    }
}

/// Errors that can occur during context operations
#[derive(Debug, Clone)]
pub enum ContextError {
    /// Condensation failed (LLM error, etc.)
    CondensationFailed(String),
    /// History is empty
    EmptyHistory,
    /// No messages to summarize
    NothingToSummarize,
    /// Invalid configuration
    InvalidConfig(String),
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextError::CondensationFailed(msg) => write!(f, "Condensation failed: {}", msg),
            ContextError::EmptyHistory => write!(f, "History is empty"),
            ContextError::NothingToSummarize => write!(f, "No messages to summarize"),
            ContextError::InvalidConfig(msg) => write!(f, "Invalid configuration: {}", msg),
        }
    }
}

impl StdError for ContextError {}

/// Token usage breakdown for debugging/display
#[derive(Debug, Clone)]
pub struct TokenBreakdown {
    /// System prompt tokens (always included)
    pub system: usize,
    /// Ephemeral context tokens (one-turn only)
    pub ephemeral: usize,
    /// Conversation history tokens (persistent)
    pub history: usize,
    /// Pending user message tokens
    pub pending: usize,
    /// Maximum allowed tokens
    pub max: usize,
}

impl TokenBreakdown {
    /// Total cached tokens (system + history)
    pub fn cached(&self) -> usize {
        self.system + self.history
    }
    
    /// Total tokens for next request
    pub fn next_request(&self) -> usize {
        self.system + self.ephemeral + self.history + self.pending
    }
    
    /// Format as human-readable string
    pub fn format(&self) -> String {
        format!(
            "Cached: {} | Ephemeral: {} | Next: {}/{}",
            self.cached(),
            self.ephemeral,
            self.next_request(),
            self.max
        )
    }
}

/// Manages conversation context including compression and condensation
/// 
/// Context structure for each request:
/// - System prompt (always included, not shown in conversation)
/// - Ephemeral context (terminal snapshots, etc. - ONE turn only)
/// - Conversation history (persistent)
/// - Pending user message (being composed)
///
/// After LLM responds, ephemeral is cleared and response added to history.
#[derive(Debug, Clone)]
pub struct ContextManager {
    config: ContextConfig,
    /// System prompt (always included, estimated size)
    system_prompt_tokens: usize,
    /// Persistent conversation history (user/assistant/tool messages)
    history: Vec<Message>,
    /// Ephemeral context - included for ONE turn only (snapshots, etc.)
    ephemeral_context: Vec<Message>,
    /// Pending user message (not yet sent)
    pending_user_message: Option<Message>,
    /// Action stamps for tracking agent actions
    pub action_stamps: ActionStampRegistry,
    /// Current conversation topic/focus (to prevent context jumping)
    pub conversation_topic: Option<String>,
    /// Archive of compressed segments for recovery
    compression_archive: crate::conversation::context_compression::CompressionArchive,
    /// Metrics from last prepare_context call (for debugging)
    last_metrics: Option<ContextMetrics>,
}

impl ContextManager {
    /// Create a new ContextManager with the given configuration
    pub fn new(config: ContextConfig) -> Self {
        Self {
            config,
            // Estimate system prompt at ~800 tokens (tools + instructions + date)
            system_prompt_tokens: 800,
            history: Vec::new(),
            ephemeral_context: Vec::new(),
            pending_user_message: None,
            last_metrics: None,
            action_stamps: ActionStampRegistry::new(50),
            conversation_topic: None,
            compression_archive: crate::conversation::context_compression::CompressionArchive::new(10),
        }
    }

    /// Add an action stamp to the registry
    pub fn add_stamp(&mut self, stamp: ActionStamp) {
        self.action_stamps.add(stamp);
    }

    /// Get recent action stamps
    pub fn recent_stamps(&self, count: usize) -> Vec<ActionStamp> {
        self.action_stamps.recent(count).iter().map(|s| (*s).clone()).collect()
    }

    /// Set the conversation topic to prevent context jumping
    pub fn set_topic(&mut self, topic: impl Into<String>) {
        self.conversation_topic = Some(topic.into());
    }

    /// Get the current conversation topic
    pub fn topic(&self) -> Option<&str> {
        self.conversation_topic.as_deref()
    }

    /// Check if a message is on-topic with the current conversation
    /// This helps prevent context jumping
    pub fn is_on_topic(&self, message: &str) -> bool {
        if let Some(ref topic) = self.conversation_topic {
            // Simple heuristic: check if key terms from topic appear in message
            let topic_lower = topic.to_lowercase();
            let msg_lower = message.to_lowercase();
            
            // Extract key words from topic (words longer than 4 chars)
            let topic_words: Vec<&str> = topic_lower
                .split_whitespace()
                .filter(|w| w.len() > 4)
                .collect();
            
            // If no significant words, consider it on-topic
            if topic_words.is_empty() {
                return true;
            }
            
            // Check if at least one topic word appears in message
            let matches = topic_words.iter().filter(|&&w| msg_lower.contains(w)).count();
            
            // Require at least 1 match or consider it potentially off-topic
            matches > 0
        } else {
            // No topic set, anything is on-topic
            true
        }
    }

    /// Estimate tokens for a message before adding it
    /// Returns (estimated_tokens, would_fit, remaining_tokens)
    pub fn estimate_message(&self, content: &str) -> (usize, bool, usize) {
        let estimated = TokenCounter::estimate(content);
        let current: usize = self.history.iter().map(|m| m.token_count).sum();
        let limit = self.config.effective_limit();
        let remaining = limit.saturating_sub(current);
        
        (estimated, estimated <= remaining, remaining)
    }

    /// Pre-flight check before sending to LLM
    /// Returns warning message if context might be problematic
    pub fn preflight_check(&self, new_content: Option<&str>) -> Option<String> {
        let current: usize = self.history.iter().map(|m| m.token_count).sum();
        let limit = self.config.effective_limit();
        let ratio = current as f64 / limit as f64;
        
        let mut warnings = Vec::new();
        
        // Check if we're near the token limit
        if ratio > self.config.condense_threshold {
            warnings.push(format!(
                "Context at {:.0}% token capacity - condensation recommended",
                ratio * 100.0
            ));
        }
        
        // Check byte size limit (hard API limit)
        let byte_size = self.total_byte_size();
        if byte_size > self.config.max_bytes {
            warnings.push(format!(
                "Context exceeds byte limit: {:.1}MB / {:.1}MB",
                byte_size as f64 / (1024.0 * 1024.0),
                self.config.max_bytes as f64 / (1024.0 * 1024.0)
            ));
        } else if byte_size as f64 > self.config.max_bytes as f64 * 0.8 {
            warnings.push(format!(
                "Context at {:.0}% byte capacity",
                (byte_size as f64 / self.config.max_bytes as f64) * 100.0
            ));
        }
        
        // Check new content size
        if let Some(content) = new_content {
            let estimated = TokenCounter::estimate(content);
            let remaining = limit.saturating_sub(current);
            
            if estimated > remaining {
                warnings.push(format!(
                    "Message may not fit (est. {} tokens, {} remaining)",
                    estimated, remaining
                ));
            }
            
            // Check if new content would exceed byte limit
            let new_content_bytes = content.len();
            if byte_size + new_content_bytes > self.config.max_bytes {
                warnings.push(format!(
                    "Message may exceed byte limit (+{} bytes)",
                    new_content_bytes
                ));
            }
        }
        
        if warnings.is_empty() {
            None
        } else {
            Some(warnings.join("; "))
        }
    }

    /// Create a new ContextManager from LlmClient configuration
    pub fn from_llm_client(client: &LlmClient) -> Self {
        let llm_config = client.config();
        // Use default byte limit (3MB) to stay under typical 4MB API limits
        // This is a safety measure independent of the token-based limit
        let config = ContextConfig {
            max_tokens: llm_config.max_context_tokens,
            condense_threshold: llm_config.condense_threshold,
            max_output_tokens: llm_config.max_tokens.unwrap_or(4096) as usize,
            input_price_per_million: llm_config.input_price_per_1m,
            output_price_per_million: llm_config.output_price_per_1m,
            max_bytes: 3 * 1024 * 1024, // 3MB default - hard safety limit
        };
        Self::new(config)
    }
    
    /// Get the compression archive
    pub fn compression_archive(&self) -> &crate::conversation::context_compression::CompressionArchive {
        &self.compression_archive
    }
    
    /// Get mutable compression archive
    pub fn compression_archive_mut(&mut self) -> &mut crate::conversation::context_compression::CompressionArchive {
        &mut self.compression_archive
    }
    
    /// Perform context compression and return a segment for UI indicator
    /// 
    /// This method:
    /// 1. Identifies important messages (with remember markers, etc.)
    /// 2. Extracts memories before compression
    /// 3. Archives compressed content
    /// 4. Returns a segment for UI display
    pub fn compress_with_indicator(&mut self) -> Option<crate::conversation::context_compression::CompressedSegment> {
        use crate::conversation::context_compression::{compress_context, CompressionConfig};
        
        let config = CompressionConfig::default();
        let limit = self.config.effective_limit();
        
        let result = compress_context(self.history.clone(), limit, &config);
        
        if !result.was_compressed {
            return None;
        }
        
        // Log metrics before moving
        let kept_len = result.kept.len();
        let trimmed_len = result.trimmed.len();
        let tokens_saved = result.segment.tokens_saved;
        let segment = result.segment.clone();
        
        // Update history with kept messages
        self.history = result.kept;
        
        // Archive the segment
        self.compression_archive.push(result.segment);
        
        crate::info_log!(
            "[CONTEXT] Context compression: kept {} messages, trimmed {} messages, saved ~{} tokens",
            kept_len,
            trimmed_len,
            tokens_saved
        );
        
        Some(segment)
    }

    // ===== Context Building Methods =====

    /// Set the pending user message (to be sent)
    pub fn set_pending_user_message(&mut self, content: &str) {
        self.pending_user_message = Some(Message::new("user", content));
    }

    /// Clear the pending user message
    pub fn clear_pending_user_message(&mut self) {
        self.pending_user_message = None;
    }

    /// Add ephemeral context (terminal snapshot, etc.) - cleared after one turn
    pub fn add_ephemeral_context(&mut self, role: &str, content: &str) {
        let message = Message::new(role, content);
        self.ephemeral_context.push(message);
    }

    /// Clear all ephemeral context
    pub fn clear_ephemeral_context(&mut self) {
        self.ephemeral_context.clear();
    }

    /// Add a message to the persistent history
    /// Automatically prunes history if token limit is exceeded
    pub fn add_message(&mut self, role: &str, content: &str) {
        let message = Message::new(role, content);
        self.history.push(message);
        
        // Auto-prune if over token limit
        let total_tokens: usize = self.history.iter().map(|m| m.token_count).sum();
        if total_tokens > self.config.effective_limit() {
            crate::info_log!(
                "[CONTEXT] Auto-pruning after add: {} tokens > {} limit",
                total_tokens,
                self.config.effective_limit()
            );
            self.prune_history();
        }
        
        // Auto-prune if over byte limit
        if self.exceeds_byte_limit() {
            crate::info_log!(
                "[CONTEXT] Auto-pruning after add: {} bytes > {} limit",
                self.total_byte_size(),
                self.config.max_bytes
            );
            self.prune_to_byte_limit();
        }
    }

    /// Add a ChatMessage to the persistent history
    pub fn add_chat_message(&mut self, msg: &ChatMessage) {
        let message = Message::from_chat_message(msg);
        self.history.push(message);
    }

    /// Set the entire history from ChatMessages (replaces history, keeps ephemeral)
    pub fn set_history(&mut self, messages: &[ChatMessage]) {
        self.history = messages.iter().map(Message::from_chat_message).collect();
    }

    /// Called when LLM response is complete
    /// - Adds assistant response to history
    /// - Clears ephemeral context (one-turn only)
    /// - Clears pending user message
    /// - Auto-prunes if over limits
    pub fn on_llm_complete(&mut self, assistant_content: &str) {
        // Add assistant response to persistent history
        self.history.push(Message::new("assistant", assistant_content));
        
        // Clear ephemeral context (was only needed for this turn)
        self.ephemeral_context.clear();
        
        // Clear pending user message (now in history)
        self.pending_user_message = None;
        
        // Auto-prune if over token limit
        let total_tokens: usize = self.history.iter().map(|m| m.token_count).sum();
        if total_tokens > self.config.effective_limit() {
            crate::info_log!(
                "[CONTEXT] Auto-pruning after LLM response: {} tokens > {} limit",
                total_tokens,
                self.config.effective_limit()
            );
            self.prune_history();
        }
        
        // Auto-prune if over byte limit
        if self.exceeds_byte_limit() {
            crate::info_log!(
                "[CONTEXT] Auto-pruning after LLM response: {} bytes > {} limit",
                self.total_byte_size(),
                self.config.max_bytes
            );
            self.prune_to_byte_limit();
        }
    }

    // ===== Token Counting Methods =====

    /// Get cached token usage (what's "saved" in history)
    /// This is: system_prompt + conversation_history
    /// Returns (cached_tokens, max_tokens)
    pub fn get_cached_token_usage(&self) -> (usize, usize) {
        let history_tokens: usize = self.history.iter().map(|m| m.token_count).sum();
        (self.system_prompt_tokens + history_tokens, self.config.max_tokens)
    }

    /// Get token usage for NEXT request (includes ephemeral)
    /// This is: system_prompt + ephemeral + history + pending
    /// Returns (next_request_tokens, max_tokens)
    pub fn get_next_request_tokens(&self) -> (usize, usize) {
        let ephemeral_tokens: usize = self.ephemeral_context.iter().map(|m| m.token_count).sum();
        let history_tokens: usize = self.history.iter().map(|m| m.token_count).sum();
        let pending_tokens = self.pending_user_message.as_ref().map(|m| m.token_count).unwrap_or(0);
        
        let total = self.system_prompt_tokens + ephemeral_tokens + history_tokens + pending_tokens;
        (total, self.config.max_tokens)
    }

    /// Get the context ratio for cached content (what UI should display)
    pub fn get_cached_context_ratio(&self) -> f64 {
        let (cached, max) = self.get_cached_token_usage();
        if max == 0 {
            0.0
        } else {
            (cached as f64 / max as f64).clamp(0.0, 1.0)
        }
    }

    /// Get the context ratio for next request (for warnings)
    pub fn get_next_request_context_ratio(&self) -> f64 {
        let (next, max) = self.get_next_request_tokens();
        if max == 0 {
            0.0
        } else {
            (next as f64 / max as f64).clamp(0.0, 1.0)
        }
    }

    /// Get breakdown of token usage for debugging/display
    pub fn get_token_breakdown(&self) -> TokenBreakdown {
        TokenBreakdown {
            system: self.system_prompt_tokens,
            ephemeral: self.ephemeral_context.iter().map(|m| m.token_count).sum(),
            history: self.history.iter().map(|m| m.token_count).sum(),
            pending: self.pending_user_message.as_ref().map(|m| m.token_count).unwrap_or(0),
            max: self.config.max_tokens,
        }
    }

    /// Check if condensation is needed based on cached content
    pub fn needs_condensation(&self) -> bool {
        self.get_cached_context_ratio() > self.config.condense_threshold
    }

    /// Get the total token count in history
    pub fn total_tokens(&self) -> usize {
        self.history.iter().map(|m| m.token_count).sum()
    }

    /// Get the total byte size of all messages in history
    pub fn total_byte_size(&self) -> usize {
        self.history.iter().map(|m| m.byte_size).sum()
    }

    /// Check if history exceeds the byte size limit
    pub fn exceeds_byte_limit(&self) -> bool {
        self.total_byte_size() > self.config.max_bytes
    }

    /// Get byte usage statistics (current, max)
    pub fn get_byte_usage(&self) -> (usize, usize) {
        let current = self.total_byte_size();
        (current, self.config.max_bytes)
    }

    /// Prepare context for LLM request
    /// - If over threshold: condense first, then prune
    /// - If over byte limit: prune aggressively
    /// - Otherwise: just prune if needed
    ///   Returns optimized history as ChatMessages
    pub async fn prepare_context(
        &mut self,
        llm_client: Option<&Arc<LlmClient>>,
    ) -> Result<Vec<ChatMessage>, ContextError> {
        // Capture before state for metrics
        let messages_before = self.history.len();
        let byte_size_before = self.total_byte_size();
        let token_count_before = self.total_tokens();
        let mut was_condensed = false;
        
        // CRITICAL: Check byte size limit first - this is a hard API limit
        if self.exceeds_byte_limit() {
            let (current_bytes, max_bytes) = self.get_byte_usage();
            crate::warn_log!(
                "Context exceeds byte limit: {} / {} bytes ({:.1}MB / {:.1}MB). Pruning aggressively.",
                current_bytes,
                max_bytes,
                current_bytes as f64 / (1024.0 * 1024.0),
                max_bytes as f64 / (1024.0 * 1024.0)
            );
            self.prune_to_byte_limit();
        }
        
        // Check if we need condensation based on token threshold
        if self.needs_condensation() {
            if let Some(client) = llm_client {
                match self.condense_history(client).await {
                    Ok(condensed) => {
                        self.history = condensed.iter().map(Message::from_chat_message).collect();
                        was_condensed = true;
                    }
                Err(e) => {
                    // Log and continue with compression only
                    crate::info_log!("Context condensation failed: {}. Continuing with compression only.", e);
                }
                }
            }
        }

        // Final prune to ensure we're within limits
        let pruned = self.prune_history();
        
        // Capture after state for metrics
        let messages_after = self.history.len();
        let byte_size_after = self.total_byte_size();
        let token_count_after = self.total_tokens();
        
        // Store metrics
        self.last_metrics = Some(ContextMetrics::new(
            messages_before,
            messages_after,
            token_count_before,
            token_count_after,
            byte_size_before,
            byte_size_after,
            was_condensed,
        ));
        
        // Log if we made significant changes
        if byte_size_before != byte_size_after || token_count_before != token_count_after {
            if let Some(ref metrics) = self.last_metrics {
                crate::info_log!("{}", metrics.format_log());
            }
        }
        
        Ok(pruned.iter().map(|m| m.to_chat_message()).collect())
    }

    /// Prune history to fit within token limits
    /// - Always preserves system prompt (first message if role == "system")
    /// - Ensures conversation starts with User after system prompt (Gemini requirement)
    /// - Keeps as many recent messages as fit
    pub fn prune_history(&mut self) -> Vec<Message> {
        if self.history.is_empty() {
            return Vec::new();
        }

        let limit = self.config.effective_limit();
        let total_tokens: usize = self.history.iter().map(|m| m.token_count).sum();

        // If under limit, return all
        if total_tokens <= limit {
            return self.history.clone();
        }

        // Extract system prompt if present
        let system_msg = if self.history[0].role == "system" {
            Some(self.history[0].clone())
        } else {
            None
        };

        let start_idx = if system_msg.is_some() { 1 } else { 0 };
        let mut pruned: Vec<Message> = Vec::new();

        // Add system message first
        if let Some(ref sys) = system_msg {
            pruned.push(sys.clone());
        }

        // Work backwards to keep recent messages
        let mut current_tokens: usize = system_msg.as_ref().map(|m| m.token_count).unwrap_or(0);
        let mut to_keep: Vec<Message> = Vec::new();

        for msg in self.history.iter().skip(start_idx).rev() {
            if current_tokens + msg.token_count <= limit {
                to_keep.push(msg.clone());
                current_tokens += msg.token_count;
            } else {
                break;
            }
        }

        // Reverse to maintain chronological order
        to_keep.reverse();

        // Gemini/Strict API Requirement: Ensure we don't start with Assistant/Tool after system
        while !to_keep.is_empty() && to_keep[0].role != "user" {
            to_keep.remove(0);
        }

        pruned.extend(to_keep);

        // Update internal history
        self.history = pruned.clone();
        pruned
    }

    /// Prune history to fit within byte size limit
    /// This is a more aggressive compression that prioritizes recent messages
    /// and is used when we hit the hard API byte size limit
    pub fn prune_to_byte_limit(&mut self) -> Vec<Message> {
        if self.history.is_empty() {
            return Vec::new();
        }

        let limit = self.config.max_bytes;
        let total_bytes: usize = self.history.iter().map(|m| m.byte_size).sum();

        // If under limit, return all
        if total_bytes <= limit {
            return self.history.clone();
        }

        // Extract system prompt if present
        let system_msg = if !self.history.is_empty() && self.history[0].role == "system" {
            Some(self.history[0].clone())
        } else {
            None
        };

        let start_idx = if system_msg.is_some() { 1 } else { 0 };
        
        // Reserve space for system message
        let system_bytes = system_msg.as_ref().map(|m| m.byte_size).unwrap_or(0);
        let available_for_messages = limit.saturating_sub(system_bytes);
        
        // Work backwards to keep recent messages that fit
        let mut to_keep: Vec<Message> = Vec::new();
        let mut current_bytes: usize = 0;
        
        for msg in self.history.iter().skip(start_idx).rev() {
            if current_bytes + msg.byte_size <= available_for_messages {
                to_keep.push(msg.clone());
                current_bytes += msg.byte_size;
            } else {
                // Message doesn't fit, skip it
                crate::info_log!(
                    "Pruning message ({} bytes, {} tokens) to fit byte limit",
                    msg.byte_size,
                    msg.token_count
                );
            }
        }

        // Reverse to maintain chronological order
        to_keep.reverse();

        // Ensure we start with a user message after system
        while !to_keep.is_empty() && to_keep[0].role != "user" {
            let removed = to_keep.remove(0);
            crate::info_log!(
                "Removing non-user start message ({} bytes) to maintain conversation flow",
                removed.byte_size
            );
        }

        // Build final pruned list
        let mut pruned: Vec<Message> = Vec::new();
        if let Some(ref sys) = system_msg {
            pruned.push(sys.clone());
        }
        pruned.extend(to_keep);

        // Update internal history
        self.history = pruned.clone();
        
        let final_bytes: usize = self.history.iter().map(|m| m.byte_size).sum();
        crate::info_log!(
            "Byte pruning complete: {} -> {} bytes ({:.1}MB -> {:.1}MB)",
            total_bytes,
            final_bytes,
            total_bytes as f64 / (1024.0 * 1024.0),
            final_bytes as f64 / (1024.0 * 1024.0)
        );
        
        pruned
    }

    /// Condense history by summarizing middle messages
    /// - Preserves system prompt
    /// - Keeps 3 most recent messages intact
    /// - Uses LLM to summarize everything in between
    ///   Returns: [System] + [Summary] + [Latest 3]
    pub async fn condense_history(
        &self,
        llm_client: &Arc<LlmClient>,
    ) -> Result<Vec<ChatMessage>, ContextError> {
        if self.history.len() <= 5 {
            return Err(ContextError::NothingToSummarize);
        }

        // Extract system prompt
        let system_msg = if !self.history.is_empty() && self.history[0].role == "system" {
            Some(self.history[0].clone())
        } else {
            None
        };

        let start_idx = if system_msg.is_some() { 1 } else { 0 };

        // Messages to summarize (everything except system and last 3)
        if self.history.len() - start_idx <= 3 {
            return Err(ContextError::NothingToSummarize);
        }

        let to_summarize = &self.history[start_idx..self.history.len() - 3];
        let latest = &self.history[self.history.len() - 3..];

        // Build summary input
        let mut summary_input = String::from(
            "Summarize the following conversation history into a concise summary \
             that preserves all key facts, decisions, and context for an AI assistant to continue the task:\n\n"
        );

        for msg in to_summarize {
            let role_label = match msg.role.as_str() {
                "system" => "System",
                "user" => "User",
                "assistant" => "Assistant",
                "tool" => "Tool",
                _ => "Unknown",
            };
            summary_input.push_str(&format!("{}: {}\n", role_label, msg.content));
        }

        // Request summary from LLM
        let summary_request = crate::provider::chat::ChatRequest::new(
            llm_client.config().model.clone(),
            vec![
                ChatMessage::system("You are a helpful assistant that summarizes technical conversations."),
                ChatMessage::user(&summary_input),
            ],
        );

        match llm_client.chat(&summary_request).await {
            Ok(response) => {
                let summary = response.content();

                let mut new_history: Vec<ChatMessage> = Vec::new();

                // Add system prompt
                if let Some(ref sys) = system_msg {
                    new_history.push(sys.to_chat_message());
                }

                // Add summary as an assistant message
                new_history.push(ChatMessage::assistant(format!("[Context Summary]: {}", summary)));

                // Add latest messages
                for msg in latest {
                    new_history.push(msg.to_chat_message());
                }

                Ok(new_history)
            }
            Err(e) => Err(ContextError::CondensationFailed(e.to_string())),
        }
    }

    /// Format context for UI display
    /// Example output:
    /// ```text
    /// Context: 45,000 / 128,000 tokens (35%), 1.2MB / 3MB bytes (40%)
    /// [System] You are a helpful assistant...
    /// [User] Hello
    /// [Assistant] Hi there!
    /// [Condensed] User asked about X, discussed Y, decided Z...
    /// ```
    pub fn format_for_ui(&self) -> String {
        let (current, max) = self.get_cached_token_usage();
        let ratio = self.get_cached_context_ratio();
        let percentage = (ratio * 100.0) as usize;
        
        let (byte_size, max_bytes) = self.get_byte_usage();
        let byte_ratio = byte_size as f64 / max_bytes as f64;
        let byte_percentage = (byte_ratio * 100.0) as usize;

        let mut output = format!(
            "Context: {} / {} tokens ({}%), {:.1}MB / {:.1}MB bytes ({}%)\n",
            format_number(current),
            format_number(max),
            percentage,
            byte_size as f64 / (1024.0 * 1024.0),
            max_bytes as f64 / (1024.0 * 1024.0),
            byte_percentage
        );

        // Show up to 5 most recent messages
        let show_count = self.history.len().min(5);
        let start_idx = self.history.len().saturating_sub(show_count);

        for msg in &self.history[start_idx..] {
            let role_label = match msg.role.as_str() {
                "system" => "[System]",
                "user" => "[User]",
                "assistant" => "[Assistant]",
                "tool" => "[Tool]",
                _ => "[Unknown]",
            };

            // Truncate long content
            let content_preview = if msg.content.len() > 100 {
                format!("{}...", &msg.content[..100])
            } else {
                msg.content.clone()
            };

            output.push_str(&format!("{} {}\n", role_label, content_preview));
        }

        // Indicate if there are more messages not shown
        if self.history.len() > show_count {
            let hidden = self.history.len() - show_count;
            output.push_str(&format!("... ({} earlier messages)\n", hidden));
        }

        output
    }

    /// Get a reference to the current history
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// Get mutable reference to history
    pub fn history_mut(&mut self) -> &mut Vec<Message> {
        &mut self.history
    }

    /// Clear all history
    pub fn clear(&mut self) {
        self.history.clear();
    }

    /// Get the configuration
    pub fn config(&self) -> &ContextConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: ContextConfig) {
        self.config = config;
    }
    
    // ===== Debugging and Inspection Methods =====
    
    /// Generate metrics for the last context preparation
    /// 
    /// Call this after prepare_context() to get detailed metrics
    pub fn get_last_metrics(&self) -> Option<&ContextMetrics> {
        self.last_metrics.as_ref()
    }
    
    /// Take a snapshot of current history state for debugging
    pub fn inspect_history(&self) -> HistorySnapshot {
        use std::collections::HashMap;
        
        let mut by_role: HashMap<String, usize> = HashMap::new();
        for msg in &self.history {
            *by_role.entry(msg.role.clone()).or_insert(0) += 1;
        }
        
        let recent_previews: Vec<(String, String)> = self.history
            .iter()
            .rev()
            .take(5)
            .map(|m| {
                let preview = if m.content.len() > 50 {
                    format!("{}...", &m.content[..50])
                } else {
                    m.content.clone()
                };
                (m.role.clone(), preview)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        
        HistorySnapshot {
            message_count: self.history.len(),
            total_tokens: self.total_tokens(),
            total_bytes: self.total_byte_size(),
            by_role,
            recent_previews,
        }
    }
    
    /// Log current context state at debug level
    pub fn log_state(&self, label: &str) {
        let snapshot = self.inspect_history();
        crate::debug_log!("[ContextManager:{}] {}", label, snapshot.format());
    }
}

/// Implement ContextCompression trait for ContextManager
impl crate::conversation::context_compression::ContextCompression for ContextManager {
    fn compress_with_indicator(&mut self, config: &crate::conversation::context_compression::CompressionConfig) -> Option<crate::conversation::context_compression::CompressedSegment> {
        use crate::conversation::context_compression::compress_context;
        
        let limit = self.config.effective_limit();
        
        let result = compress_context(self.history.clone(), limit, config);
        
        if !result.was_compressed {
            return None;
        }
        
        let kept_len = result.kept.len();
        let trimmed_len = result.trimmed.len();
        let tokens_saved = result.segment.tokens_saved;
        let segment = result.segment.clone();
        
        // Update history with kept messages
        self.history = result.kept;
        
        // Archive the segment
        self.compression_archive.push(result.segment);
        
        crate::info_log!(
            "[CONTEXT] Context compression: kept {} messages, trimmed {} messages, saved ~{} tokens",
            kept_len,
            trimmed_len,
            tokens_saved
        );
        
        Some(segment)
    }
    
    fn compression_archive(&self) -> &crate::conversation::context_compression::CompressionArchive {
        &self.compression_archive
    }
    
    fn compression_archive_mut(&mut self) -> &mut crate::conversation::context_compression::CompressionArchive {
        &mut self.compression_archive
    }
    
    fn check_auto_restore(&self, user_message: &str) -> crate::conversation::context_compression::AutoRestoreResult {
        crate::conversation::context_compression::check_auto_restore(user_message, &self.compression_archive)
    }
}

/// Format a number with commas as thousands separators
fn format_number(n: usize) -> String {
    n.to_string()
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(std::str::from_utf8)
        .collect::<Result<Vec<&str>, _>>()
        .unwrap_or_default()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_counter() {
        let text = "Hello world"; // 11 chars
        let tokens = TokenCounter::estimate(text);
        assert_eq!(tokens, 11 / 4 + 1); // 3 + 1 = 4
    }

    #[test]
    fn test_context_config() {
        let config = ContextConfig::new(100_000)
            .with_condense_threshold(0.7)
            .with_max_output_tokens(2048);

        assert_eq!(config.max_tokens, 100_000);
        assert_eq!(config.condense_threshold, 0.7);
        assert_eq!(config.max_output_tokens, 2048);
        assert_eq!(config.effective_limit(), 100_000 - 2048);
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::new("user", "Hello there");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello there");
        assert!(msg.token_count > 0);
    }

    #[test]
    fn test_context_manager_add_message() {
        let mut manager = ContextManager::new(ContextConfig::new(1000));
        manager.add_message("system", "You are helpful");
        manager.add_message("user", "Hello");

        assert_eq!(manager.history.len(), 2);
        assert_eq!(manager.history[0].role, "system");
        assert_eq!(manager.history[1].role, "user");
    }

    #[test]
    fn test_prune_history_preserves_system() {
        let mut manager = ContextManager::new(ContextConfig::new(100));

        // Add system message
        manager.add_message("system", "You are a helpful assistant");

        // Add many user/assistant messages
        for i in 0..20 {
            manager.add_message("user", &format!("Question {}", i));
            manager.add_message("assistant", &format!("Answer {} with some longer content to use tokens", i));
        }

        let pruned = manager.prune_history();

        // System message should be preserved
        assert_eq!(pruned[0].role, "system");

        // After pruning, should have fewer messages
        assert!(pruned.len() < 41);
    }

    #[test]
    fn test_prune_history_ensures_user_start() {
        let mut manager = ContextManager::new(ContextConfig::new(100));

        // Add system message
        manager.add_message("system", "System prompt");

        // Add messages that would result in assistant being first after system
        manager.add_message("assistant", "First assistant message that is very long to use up tokens quickly");
        manager.add_message("user", "User question");
        manager.add_message("assistant", "Answer");

        // Manually limit to force pruning
        manager.config.max_tokens = 20;

        let pruned = manager.prune_history();

        // After system, first message should be user
        if pruned.len() > 1 {
            assert_eq!(pruned[1].role, "user");
        }
    }

    #[test]
    fn test_context_ratio() {
        let mut manager = ContextManager::new(ContextConfig::new(100));
        manager.add_message("user", "Hello world test message");

        let ratio = manager.get_cached_context_ratio();
        assert!(ratio > 0.0);
        assert!(ratio <= 1.0);
    }

    #[test]
    fn test_format_for_ui() {
        let mut manager = ContextManager::new(ContextConfig::new(100_000));
        manager.add_message("system", "You are helpful");
        manager.add_message("user", "Hello");
        manager.add_message("assistant", "Hi there! How can I help?");

        let formatted = manager.format_for_ui();

        assert!(formatted.contains("Context:"));
        assert!(formatted.contains("[System]"));
        assert!(formatted.contains("[User]"));
        assert!(formatted.contains("[Assistant]"));
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1000000), "1,000,000");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn test_byte_size_tracking() {
        let mut manager = ContextManager::new(ContextConfig::new(1000));
        
        // Add a message with known content
        let content = "Hello world"; // 11 bytes
        manager.add_message("user", content);
        
        // Check byte size is tracked correctly
        let byte_size = manager.total_byte_size();
        assert_eq!(byte_size, content.len());
        
        // Check message has byte_size field set
        assert_eq!(manager.history[0].byte_size, content.len());
    }

    #[test]
    fn test_prune_to_byte_limit() {
        // Create config with very small byte limit
        let mut config = ContextConfig::new(1000);
        config.max_bytes = 50; // Only 50 bytes allowed
        
        let mut manager = ContextManager::new(config);
        
        // Add system message
        manager.add_message("system", "System prompt here"); // 18 bytes
        
        // Add multiple user messages with larger content
        for i in 0..5 {
            manager.add_message("user", &format!("Message number {}", i)); // ~16 bytes each
        }
        
        // Should exceed byte limit (18 + 5*16 = 98 bytes > 50)
        assert!(manager.exceeds_byte_limit());
        
        // Prune to fit
        let pruned = manager.prune_to_byte_limit();
        
        // Should have system + some recent messages
        assert!(!pruned.is_empty());
        assert_eq!(pruned[0].role, "system");
        
        // Should be under byte limit now
        let final_bytes = manager.total_byte_size();
        assert!(final_bytes <= manager.config().max_bytes);
    }

    #[test]
    fn test_byte_limit_prevents_api_overflow() {
        // Simulate the bug scenario: 34MB of content
        let mut config = ContextConfig::new(128_000);
        config.max_bytes = 1000; // Small limit for testing
        
        let mut manager = ContextManager::new(config);
        
        // Add system prompt
        manager.add_message("system", "You are helpful");
        
        // Add a huge message (simulating the bug)
        let huge_content = "x".repeat(2000); // 2000 bytes
        manager.add_message("tool", &huge_content);
        
        // Should detect byte limit exceeded
        assert!(manager.exceeds_byte_limit());
        
        // Prune should fix it
        manager.prune_to_byte_limit();
        
        // Should be under limit
        assert!(!manager.exceeds_byte_limit());
        let final_bytes = manager.total_byte_size();
        assert!(final_bytes <= 1000);
    }
}
