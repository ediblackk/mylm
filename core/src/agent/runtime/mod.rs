//! Runtime Layer
//!
//! Async capability execution with strict separation of concerns:
//! - core: Fundamental types and traits (no async runtime deps)
//! - executor: Decision interpretation
//! - capabilities: Capability implementations
//! - governance: Policy enforcement
//! - session: Orchestration
//! - stubs: Test utilities

// Core types and traits - foundation
pub mod core;

// Decision interpretation
pub mod executor;

// Capability implementations
pub mod capabilities;

// Governance and policy enforcement
pub mod governance;

// Session orchestration
pub mod session;

// Test stubs
pub mod stubs;

// Re-exports for convenience
pub use core::{
    RuntimeContext, TraceId, RuntimeError,
    LLMError, ToolError, ApprovalError, WorkerError,
    Capability, LLMCapability, ToolCapability, ApprovalCapability,
    WorkerCapability, TelemetryCapability, StreamChunk, WorkerSpawnHandle,
    TerminalExecutor, DefaultTerminalExecutor, SharedTerminalExecutor, TerminalExecutorRef,
};

pub use executor::{AgentRuntime, CapabilityGraph};

pub use session::{
    Session, UserInput, OutputEvent, SessionStatus, SessionResult, SessionError,
    ContractRuntime,
};
