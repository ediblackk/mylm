//! Agent V2 implementation - modular architecture
//!
//! This module provides the core agentic loop implementation with:
//! - `core`: Main AgentV2 struct and step logic
//! - `execution`: Parallel tool execution
//! - `lifecycle`: State management and memory injection
//! - `driver`: Execution drivers (legacy and event-driven)
//! - `protocol`: Message protocols and parsing
//! - `memory`: Memory management helpers
//! - `recovery`: Error recovery mechanisms
//! - `jobs`: Background job registry
//! - `orchestrator`: Session management and chat loop

pub mod core;
pub mod driver;
pub mod execution;
pub mod jobs;
pub mod lifecycle;
pub mod memory;
pub mod protocol;
pub mod recovery;
pub mod orchestrator;

// Public API exports
pub use self::core::{AgentV2, AgentV2Config};
pub use self::execution::{execute_parallel_tools, execute_single_tool};
pub use self::lifecycle::LifecycleManager;
pub use self::protocol::{AgentDecision, AgentRequest, AgentResponse, AgentError};
pub use self::protocol::parser::{ShortKeyAction, parse_short_key_actions_from_content};
pub use self::memory::MemoryManager;

// Note: PromptBuilder is re-exported from crate::agent::prompt (shared at root)
