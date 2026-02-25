//! Error types for the agent system
//!
//! Central error types used across all agent layers.
//! These unify error handling between cognition, runtime, and orchestration.
//!
//! Links:
//! - Used by: cognition (CognitiveError wraps these), runtime (RuntimeError), orchestrator
//! - Defines: `AgentError` enum, `AgentResult` type alias

use super::ids::IntentId;

/// Errors that can occur in the agent system
#[derive(Debug, Clone, PartialEq)]
pub enum AgentError {
    InvalidIntentId(String),
    CyclicDependency(Vec<IntentId>),
    UnknownDependency(IntentId),
    InvalidState(String),
    Transport(String),
    Kernel(String),
    Runtime(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::InvalidIntentId(id) => write!(f, "Invalid intent ID: {}", id),
            AgentError::CyclicDependency(ids) => {
                write!(f, "Cyclic dependency detected: {:?}", ids)
            }
            AgentError::UnknownDependency(id) => write!(f, "Unknown dependency: {:?}", id),
            AgentError::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
            AgentError::Transport(msg) => write!(f, "Transport error: {}", msg),
            AgentError::Kernel(msg) => write!(f, "Kernel error: {}", msg),
            AgentError::Runtime(msg) => write!(f, "Runtime error: {}", msg),
        }
    }
}

impl std::error::Error for AgentError {}

/// Result type for agent operations
pub type AgentResult<T> = Result<T, AgentError>;

// Backward compatibility - ContractError is now AgentError
pub use AgentError as ContractError;
pub use AgentResult as ContractResult;
