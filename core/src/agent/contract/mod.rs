//! V3 Contract - Core type definitions for the Agency Kernel architecture
//!
//! This module defines the contracts between:
//! - AgencyKernel (pure, sync, deterministic)
//! - AgencyRuntime (async, side effects)
//! - EventTransport (pluggable event queue)
//! - Session (orchestration with dynamic DAG expansion)
//!
//! # Architectural Principles
//!
//! 1. **Kernel is pure**: No async, no IO, no channels
//! 2. **Runtime executes**: All side effects, all async operations
//! 3. **Transport abstracts**: Event source/sink, can be local or distributed
//! 4. **Session orchestrates**: Dynamic DAG expansion, coordination
//!
//! # Distributed Execution Guarantees
//!
//! ## Deterministic IntentIds
//!
//! IntentIds are derived from kernel evolution, not runtime state:
//! ```rust,ignore
//! IntentId::from_step(step_count, intent_index)
//! // High 32 bits = step_count (from AgentState)
//! // Low 32 bits = intent_index (canonical ordering)
//! ```
//!
//! This ensures replay generates identical IDs.
//!
//! ## Execution Model
//!
//! - **Single Leader**: Session owns DAG, assigns IDs, tracks completion
//! - **Multiple Workers**: Runtime executes intents on any available node
//! - **At-least-once delivery**: Transport guarantees delivery (may duplicate)
//! - **Exactly-once execution**: Leader deduplicates via `completed` set
//!
//! ## Idempotency Requirement
//!
//! All intents MUST be idempotent. If a worker crashes and intent is retried,
//! the result must be identical (or semantically equivalent).
//!
//! ## Event Ordering
//!
//! - Transport preserves FIFO per session
//! - Kernel sees events in deterministic order
//! - LogicalClock assigned by Session before kernel processing
//!
//! ## Failure Modes
//!
//! - **Worker crash**: Leader detects timeout, reassigns IntentId
//! - **Leader crash**: Session dies (no distributed consensus)

#![forbid(unsafe_code)]

// Core type modules
pub mod ids;
pub mod events;
pub mod intents;
pub mod observations;
pub mod config;
pub mod graph;
pub mod envelope;

// Core traits
pub mod kernel;
pub mod runtime;
pub mod transport;
pub mod session;

// Re-exports for convenience
pub use ids::{IntentId, NodeId, EventId, LogicalClock, SessionId};
pub use events::KernelEvent;
pub use intents::{Intent, IntentNode, Priority};
pub use observations::Observation;
pub use config::{KernelConfig, PolicySet, WorkerLimits};
pub use events::ToolSchema;
pub use graph::IntentGraph;
pub use envelope::{KernelEventEnvelope, EventSource};

// Trait re-exports
pub use kernel::AgencyKernel;
pub use runtime::AgencyRuntime;
pub use transport::EventTransport;
pub use session::{Session, SessionError};

/// Errors that can occur in the contract layer
#[derive(Debug, Clone, PartialEq)]
pub enum ContractError {
    InvalidIntentId(String),
    CyclicDependency(Vec<IntentId>),
    UnknownDependency(IntentId),
    InvalidState(String),
    Transport(String),
    Kernel(String),
    Runtime(String),
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContractError::InvalidIntentId(id) => write!(f, "Invalid intent ID: {}", id),
            ContractError::CyclicDependency(ids) => {
                write!(f, "Cyclic dependency detected: {:?}", ids)
            }
            ContractError::UnknownDependency(id) => write!(f, "Unknown dependency: {:?}", id),
            ContractError::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
            ContractError::Transport(msg) => write!(f, "Transport error: {}", msg),
            ContractError::Kernel(msg) => write!(f, "Kernel error: {}", msg),
            ContractError::Runtime(msg) => write!(f, "Runtime error: {}", msg),
        }
    }
}

impl std::error::Error for ContractError {}

/// Result type for contract operations
pub type ContractResult<T> = Result<T, ContractError>;
