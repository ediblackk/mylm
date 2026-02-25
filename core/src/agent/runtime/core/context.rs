//! Runtime context
//!
//! Passed to every capability call. Contains cancellation and tracing.
//! This is the "request context" that flows through all async operations.
//!
//! Links:
//! - Used by: All capability traits (LLMCapability, ToolCapability, etc.)
//! - Contains: TraceId (distributed tracing), CancellationToken (cooperative cancel)
//! - Created by: Session, passed to Runtime::interpret()

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use super::terminal::TerminalExecutor;
use crate::agent::identity::AgentId;

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
    
    /// Agent identity for this execution
    agent_id: Option<AgentId>,
    
    /// Current working directory for shell commands
    /// Shared state so it can be updated (e.g., from PTY CWD changes)
    working_dir: Arc<RwLock<PathBuf>>,
    
    /// Sandbox root directory - commands cannot escape above this
    /// None = no restriction
    sandbox_root: Option<PathBuf>,
}

impl std::fmt::Debug for RuntimeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeContext")
            .field("trace_id", &self.trace_id)
            .field("cancellation", &self.cancellation)
            .field("has_terminal", &self.terminal.is_some())
            .field("sandbox_root", &self.sandbox_root)
            .finish_non_exhaustive()
    }
}

impl RuntimeContext {
    pub fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        Self {
            trace_id: TraceId::new(),
            cancellation: CancellationToken::new(),
            terminal: None,
            agent_id: None,
            working_dir: Arc::new(RwLock::new(cwd)),
            sandbox_root: None,
        }
    }
    
    /// Create context with specific working directory
    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Arc::new(RwLock::new(dir));
        self
    }
    
    /// Create context with sandbox restriction
    pub fn with_sandbox(mut self, root: Option<PathBuf>) -> Self {
        self.sandbox_root = root;
        self
    }
    
    /// Get current working directory
    pub async fn current_dir(&self) -> PathBuf {
        self.working_dir.read().await.clone()
    }
    
    /// Update current working directory
    pub async fn set_current_dir(&self, dir: PathBuf) {
        let mut wd = self.working_dir.write().await;
        *wd = dir;
    }
    
    /// Get sandbox root
    pub fn sandbox_root(&self) -> Option<&PathBuf> {
        self.sandbox_root.as_ref()
    }
    
    /// Check if a path is within sandbox
    pub fn is_within_sandbox(&self, path: &PathBuf) -> bool {
        match &self.sandbox_root {
            None => true,
            Some(root) => {
                // Canonicalize would be better but requires sync I/O
                // For now, check if path starts with root
                path.starts_with(root)
            }
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
    
    /// Set the agent ID
    pub fn with_agent_id(mut self, agent_id: AgentId) -> Self {
        self.agent_id = Some(agent_id);
        self
    }
    
    /// Get the agent ID if set
    pub fn agent_id(&self) -> Option<AgentId> {
        self.agent_id.clone()
    }
    
    /// Create child context with same trace
    /// Workers inherit parent's working_dir and sandbox
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            cancellation: self.cancellation.child_token(),
            terminal: self.terminal.clone(),
            agent_id: self.agent_id.clone(),
            working_dir: Arc::clone(&self.working_dir),
            sandbox_root: self.sandbox_root.clone(),
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
