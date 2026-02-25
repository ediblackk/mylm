//! Conversation management module
//!
//! Manages chat history, token counting, compression, and condensation
//! to keep the LLM context window within limits.

pub mod manager;
pub mod context_compression;

// Re-export conversation manager types
pub use manager::{ContextConfig, ContextManager, ContextError, Message, TokenCounter, TokenBreakdown};

// Re-export context compression types
pub use context_compression::{
    CompressedSegment, 
    CompressionArchive, 
    CompressionConfig, 
    CompressionResult,
    ContextCompression,
    compress_context,
};
