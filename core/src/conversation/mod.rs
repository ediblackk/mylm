//! Conversation management module
//!
//! Manages chat history, token counting, pruning, and condensation
//! to keep the LLM context window within limits.

pub mod manager;
pub mod pruning;

// Re-export conversation manager types
pub use manager::{ContextConfig, ContextManager, ContextError, Message, TokenCounter, TokenBreakdown};

// Re-export pruning types
pub use pruning::{
    PrunedSegment, 
    PrunedHistory, 
    SmartPruningConfig, 
    SmartPruneResult,
    SmartPruning,
    smart_prune,
};
