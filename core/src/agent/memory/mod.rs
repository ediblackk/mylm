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

pub use manager::{AgentMemoryManager, MemoryMode, MemoryStats};
pub use context::{MemoryContextBuilder, InjectionStrategy, inject_memory_context, get_context_for_query};
pub use extraction::{MemoryExtractor, ExtractedMemory, extract_memories};
