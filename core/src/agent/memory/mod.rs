//! Agent Memory Module
//!
//! Provides memory capabilities for the agent system.
//!
//! # Overview
//!
//! This module integrates the core memory storage (`crate::memory`) with the
//! agent system, providing:
//!
//! - **Hot memory**: Recent activity from journal
//! - **Cold memory**: Semantic search via vector store
//! - **Context injection**: Automatic memory inclusion in prompts
//!
//! # Usage
//!
//! ```rust,ignore
//! use mylm_core::agent::memory::{AgentMemoryManager, MemoryMode};
//! use mylm_core::config::agent::MemoryConfig;
//!
//! let config = MemoryConfig::default();
//! let manager = AgentMemoryManager::new(config).await?;
//!
//! // Add a memory
//! manager.add_user_note("User prefers dark mode").await?;
//!
//! // Search memories
//! let results = manager.search_memories("dark mode", 5).await?;
//!
//! // Get context for prompt
//! let context = manager.format_hot_memory_for_prompt(5).await;
//! ```

pub mod manager;
pub mod context;
pub mod extraction;

pub use manager::{AgentMemoryManager, AgentMemoryProvider, MemoryMode, MemoryStats};
pub use context::{MemoryContextBuilder, InjectionStrategy, inject_memory_context, get_context_for_query};
pub use extraction::{MemoryExtractor, ExtractedMemory, extract_memories};

/// Trait for memory providers that can inject context into prompts
/// 
/// Implementors provide relevant memories based on the current conversation context.
/// This is called proactively BEFORE the LLM generates a response.
pub trait MemoryProvider: Send + Sync {
    /// Get relevant memory context for the given user message
    /// 
    /// Returns a formatted string to be injected into the system prompt.
    fn get_context(&self, user_message: &str) -> String;
    
    /// Save a memory fire-and-forget style
    /// 
    /// This should not block - the save happens asynchronously.
    fn remember(&self, content: &str);
    
    /// Build memory context from full conversation state
    /// 
    /// This receives the complete request context (history, scratchpad, system prompt)
    /// and returns relevant memories to augment the prompt.
    /// 
    /// # Arguments
    /// * `history` - Recent conversation messages (bounded window)
    /// * `scratchpad` - Current working context/scratchpad
    /// * `system_prompt` - Current system prompt being used
    /// 
    /// # Returns
    /// Formatted memory context string, or empty string if no relevant memories
    fn build_context(&self, history: &[crate::agent::types::intents::Message], scratchpad: &str, system_prompt: &str) -> String;
}
