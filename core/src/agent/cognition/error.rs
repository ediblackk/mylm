//! Cognitive errors
//!
//! Errors from pure cognitive processing (no runtime/IO errors).
//! These represent logic failures: malformed actions, invalid state, context overflow.
//!
//! Links:
//! - Used by: StepEngine implementations (engine.rs, llm_engine.rs)
//! - Returned from: `StepEngine::step()`, `GraphEngine::process()`
//! - Distinct from: RuntimeError (IO failures), AgentError (general)

use std::fmt;

/// Errors from cognitive processing
#[derive(Debug, Clone)]
pub enum CognitiveError {
    /// Action could not be parsed
    MalformedAction(String),
    
    /// Invalid state for operation
    InvalidState(String),
    
    /// Limit exceeded
    LimitExceeded { limit: usize, current: usize },
    
    /// History too long
    ContextOverflow,
}

impl fmt::Display for CognitiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CognitiveError::MalformedAction(msg) => {
                write!(f, "Malformed action: {}", msg)
            }
            CognitiveError::InvalidState(msg) => {
                write!(f, "Invalid state: {}", msg)
            }
            CognitiveError::LimitExceeded { limit, current } => {
                write!(f, "Limit exceeded: {} > {}", current, limit)
            }
            CognitiveError::ContextOverflow => {
                write!(f, "Context window overflow")
            }
        }
    }
}

impl std::error::Error for CognitiveError {}
