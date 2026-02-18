//! V3 Contract - Re-export module for backward compatibility
//!
//! This module re-exports types from their canonical locations:
//! - `types/` - Core data types (Intent, Observation, KernelConfig, etc.)
//! - `cognition/` - Kernel trait
//! - `runtime/` - Runtime trait
//! - `session/` - Session trait
//!
//! DEPRECATED: Import directly from the source modules instead.

#![forbid(unsafe_code)]

// Remaining contract modules (to be distributed)
pub mod kernel;
pub mod transport;
pub mod session;

// Re-export types from types module (canonical location)
pub use crate::agent::types::ids::{IntentId, NodeId, EventId, LogicalClock, SessionId};
pub use crate::agent::types::events::KernelEvent;
pub use crate::agent::types::intents::ToolSchema;
pub use crate::agent::types::intents::{Intent, IntentNode, Priority, ApprovalRequest, ExitReason};
pub use crate::agent::types::observations::{Observation, HaltReason, ExecutionError};
pub use crate::agent::types::config::{KernelConfig, PolicySet, WorkerLimits, PromptConfig, FeatureFlags};
pub use crate::agent::types::graph::{IntentGraph, IntentGraphBuilder};
pub use crate::agent::types::envelope::{KernelEventEnvelope, EventSource};
pub use crate::agent::types::error::{AgentError, AgentResult, ContractError, ContractResult};

// Re-export runtime trait and types from runtime module
pub use crate::agent::runtime::{
    AgencyRuntime, AgencyRuntimeError, TelemetryEvent, 
    HealthStatus, RuntimeConfig, RetryConfig, RetryableErrorType,
    ToolProvider, LLMProvider, WorkerProvider, WorkerStatus,
};

// Trait re-exports from contract modules
pub use kernel::AgencyKernel;
pub use transport::EventTransport;
pub use session::{Session, SessionError};
