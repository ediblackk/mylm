//! Capability Implementations
//!
//! Concrete implementations of runtime capability traits.
//!
//! # Available Implementations
//!
//! ## Transport
//! - `InMemoryTransport`: Single-process FIFO transport
//!
//! ## Tools
//! - `ToolRegistry`: Dynamic tool registry with 8 built-in tools
//! - `SimpleToolExecutor`: Basic tool executor
//!
//! ## LLM
//! - `LlmClientCapability`: Bridge to existing LlmClient
//!
//! ## Approval
//! - `TerminalApprovalCapability`: Interactive terminal prompts
//! - `AutoApproveCapability`: Auto-approve all (testing)
//!
//! ## Workers
//! - `LocalWorkerCapability`: Spawns actual tokio tasks
//!
//! ## Telemetry
//! - `ConsoleTelemetry`: Logs to console/file with metrics
//!
//! ## Web Search
//! - `WebSearchCapability`: Kimi/SerpAPI/Brave search
//! - `StubWebSearch`: Stub for testing
//!
//! ## Memory
//! - `MemoryCapability`: Long-term memory storage
//!
//! ## Wrappers
//! - `Retry`: Add retry logic to any capability
//!
//! # Built-in Tools (ToolRegistry)
//!
//! | Tool | Description |
//! |------|-------------|
//! | `shell` | Execute shell commands |
//! | `read_file` / `cat` | Read file contents |
//! | `write_file` | Write to file |
//! | `list_dir` / `ls` | List directory |
//! | `search` | Search files |
//! | `pwd` | Print working directory |
//!
//! See `MOD.md` for implementation guide.

pub mod retry;
pub mod local;
pub mod llm_client;
pub mod terminal_approval;
pub mod simple_tool;
pub mod tool_registry;
pub mod local_worker;
pub mod console_telemetry;
pub mod web_search;
pub mod memory;
pub mod vector_store;
pub mod in_memory_transport;
pub mod dag_executor;

pub use retry::{
    RetryConfig, RetryLLM, RetryTools, CircuitBreaker, CircuitState,
    CircuitBreakerLLM, ResilientLLM,
};
pub use llm_client::LlmClientCapability;
pub use terminal_approval::{TerminalApprovalCapability, AutoApproveCapability};
pub use simple_tool::SimpleToolExecutor;
pub use tool_registry::ToolRegistry;
pub use local_worker::LocalWorkerCapability;
pub use console_telemetry::ConsoleTelemetry;
pub use web_search::{WebSearchCapability, StubWebSearch, SearchProvider};
pub use memory::{MemoryCapability, MemoryEntry, MemoryCategory, VectorStore, StubVectorStore, SearchResult};
pub use crate::agent::memory::{AgentMemoryManager, MemoryMode, MemoryStats};
pub use vector_store::{InMemoryVectorStore, VectorEntry, SimpleEmbedder};
pub use in_memory_transport::{InMemoryTransport, connected_pair};
pub use dag_executor::{DagExecutor, DagExecutionResult};
