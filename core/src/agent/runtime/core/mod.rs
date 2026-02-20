//! Runtime Core
//!
//! Fundamental types and traits for the runtime layer.
//! These are the foundation - no async runtime dependencies required.

pub mod context;
pub mod error;
pub mod capability;
pub mod terminal;

pub use context::{RuntimeContext, TraceId};
pub use error::{
    RuntimeError, ToolError, LLMError, ApprovalError, WorkerError,
    AgencyRuntime, AgencyRuntimeError, TelemetryEvent, HealthStatus,
    RuntimeConfig, RetryConfig,
};
pub use capability::{
    Capability, LLMCapability, ToolCapability, ApprovalCapability, 
    WorkerCapability, TelemetryCapability, StreamChunk, WorkerSpawnHandle,
};
pub use terminal::{TerminalExecutor, DefaultTerminalExecutor, SharedTerminalExecutor, TerminalExecutorRef};
