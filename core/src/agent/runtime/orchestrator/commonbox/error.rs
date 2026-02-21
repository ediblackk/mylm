//! Commonbox error types

use crate::agent::runtime::orchestrator::commonbox::job::JobStatus;

/// Errors from Commonbox operations.
#[derive(Debug, Clone, PartialEq)]
pub enum CommonboxError {
    /// Permission denied (trying to update another agent's entry)
    PermissionDenied,
    /// Agent not found
    AgentNotFound,
    /// Job not found
    JobNotFound,
    /// Invalid state transition
    InvalidTransition { from: JobStatus, to: JobStatus },
    /// Invalid state for operation
    InvalidState { state: String, operation: String },
    /// Dependency not found
    DependencyNotFound,
    /// Circular dependency
    CircularDependency,
    /// Resource already claimed by another agent
    ResourceAlreadyClaimed,
}

impl std::fmt::Display for CommonboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommonboxError::PermissionDenied => write!(f, "Permission denied"),
            CommonboxError::AgentNotFound => write!(f, "Agent not found"),
            CommonboxError::JobNotFound => write!(f, "Job not found"),
            CommonboxError::InvalidTransition { from, to } => {
                write!(f, "Invalid transition from {:?} to {:?}", from, to)
            }
            CommonboxError::InvalidState { state, operation } => {
                write!(f, "Invalid state {} for operation {}", state, operation)
            }
            CommonboxError::DependencyNotFound => write!(f, "Dependency not found"),
            CommonboxError::CircularDependency => write!(f, "Circular dependency detected"),
            CommonboxError::ResourceAlreadyClaimed => {
                write!(f, "Resource already claimed by another agent")
            }
        }
    }
}

impl std::error::Error for CommonboxError {}
