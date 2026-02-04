//! Agent V2 implementation - modular architecture
//!
//! This module provides the core agentic loop implementation with:
//! - `core`: Main AgentV2 struct and step logic
//! - `execution`: Parallel tool execution
//! - `lifecycle`: State management and memory injection
//! - `driver`: Execution drivers (legacy and event-driven)
//! - `protocol`: Message protocols and parsing
//! - `prompt`: System prompt construction
//! - `memory`: Memory management helpers
//! - `recovery`: Error recovery mechanisms
//! - `jobs`: Background job registry

pub mod core;
pub mod driver;
pub mod execution;
pub mod jobs;
pub mod lifecycle;
pub mod memory;
pub mod prompt;
pub mod protocol;
pub mod recovery;

// Public API exports
pub use self::core::AgentV2;
pub use self::execution::{execute_parallel_tools, execute_single_tool};
pub use self::lifecycle::LifecycleManager;
pub use self::protocol::{AgentDecision, AgentRequest, AgentResponse, AgentError};
pub use self::protocol::parser::{ShortKeyAction, parse_short_key_actions_from_content};
pub use self::prompt::PromptBuilder;
pub use self::memory::MemoryManager;
