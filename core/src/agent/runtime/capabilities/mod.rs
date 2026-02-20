//! Capability Implementations
//!
//! Concrete implementations of runtime capability traits.

pub mod llm;
pub mod approval;
pub mod worker;
pub mod telemetry;
pub mod memory;
pub mod retry;
pub mod local;
pub mod transport;

pub use llm::LlmClientCapability;
pub use crate::agent::tools::{ToolRegistry, ToolDescription};
pub use approval::{TerminalApprovalCapability, AutoApproveCapability};
pub use worker::LocalWorkerCapability;
pub use telemetry::ConsoleTelemetry;
pub use memory::{MemoryCapability, MemoryCategory};
pub use retry::{
    RetryConfig, RetryLLM, RetryTools, CircuitBreaker, CircuitState,
    CircuitBreakerLLM, ResilientLLM,
};
pub use local::SimpleToolExecutor;
pub use transport::{InMemoryTransport, connected_pair};

// Re-export from agent::memory for convenience
pub use crate::agent::memory::{AgentMemoryManager, MemoryMode, MemoryStats};
