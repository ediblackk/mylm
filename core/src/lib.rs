//! Core library for mylm - AI agent system with LLM integration.
//!
//! Provides the foundational components for building and running AI agents
//! including tool systems, memory management, LLM clients, terminal control,
//! and configuration management.
//!
//! # Main Modules
//! - `agent`: Agent implementations and tool system
//! - `pacore`: Parallel Consensus Reasoning (PaCoRe) experimental engine
//! - `llm`: LLM client abstractions and API integrations
//! - `memory`: Vector store and long-term memory management
//! - `config`: Configuration management (v1 and v2)
//! - `terminal`: Terminal emulation and PTY management
//! - `protocol`: MCP (Model Context Protocol) message definitions

pub mod agent;
pub mod error;
// TODO: pub mod pacore;  // Module doesn't exist - commented out
pub mod terminal;
pub mod config;
pub mod context;
pub mod executor;
pub mod llm;
pub mod memory;
pub mod output;
pub mod scheduler;
pub mod state;
pub mod protocol;
pub mod factory;
pub mod util;
pub mod rate_limiter;

// Re-exports for convenience
// TODO: pub use agent::v1::Agent;  // Legacy V1 - marked for deletion
pub use agent::v2::AgentV2;
pub use agent::v2::driver::factory::BuiltAgent;
pub use config::Config;
pub use error::{MylmError, Result};
pub use memory::store::VectorStore as MemoryStore;
