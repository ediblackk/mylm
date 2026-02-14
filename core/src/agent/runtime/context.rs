//! Runtime context
//!
//! Passed to every capability call. Contains cancellation and tracing.

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Distributed trace identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TraceId(String);

impl TraceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
    
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

/// Runtime context for capability calls
#[derive(Debug, Clone)]
pub struct RuntimeContext {
    /// Distributed trace ID
    pub trace_id: TraceId,
    
    /// Cancellation token
    pub cancellation: CancellationToken,
}

impl RuntimeContext {
    pub fn new() -> Self {
        Self {
            trace_id: TraceId::new(),
            cancellation: CancellationToken::new(),
        }
    }
    
    /// Create child context with same trace
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            cancellation: self.cancellation.child_token(),
        }
    }
    
    /// Check if cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

impl Default for RuntimeContext {
    fn default() -> Self {
        Self::new()
    }
}
