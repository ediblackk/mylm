//! Runtime context
//!
//! Passed to every capability call. Contains cancellation and tracing.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use super::terminal::TerminalExecutor;

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
#[derive(Clone)]
pub struct RuntimeContext {
    /// Distributed trace ID
    pub trace_id: TraceId,
    
    /// Cancellation token
    pub cancellation: CancellationToken,
    
    /// Terminal executor for shell commands
    /// When None, commands run via std::process::Command
    terminal: Option<Arc<dyn TerminalExecutor>>,
}

impl std::fmt::Debug for RuntimeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeContext")
            .field("trace_id", &self.trace_id)
            .field("cancellation", &self.cancellation)
            .field("has_terminal", &self.terminal.is_some())
            .finish()
    }
}

impl RuntimeContext {
    pub fn new() -> Self {
        Self {
            trace_id: TraceId::new(),
            cancellation: CancellationToken::new(),
            terminal: None,
        }
    }
    
    /// Set the terminal executor
    pub fn with_terminal(mut self, terminal: Arc<dyn TerminalExecutor>) -> Self {
        self.terminal = Some(terminal);
        self
    }
    
    /// Get the terminal executor if set
    pub fn terminal(&self) -> Option<&dyn TerminalExecutor> {
        self.terminal.as_ref().map(|t| &**t)
    }
    
    /// Check if a terminal executor is available
    pub fn has_terminal(&self) -> bool {
        self.terminal.is_some()
    }
    
    /// Create child context with same trace
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            cancellation: self.cancellation.child_token(),
            terminal: self.terminal.clone(),
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
