//! Agent state
//! 
//! Immutable snapshot of agent cognition.
//! No runtime objects. No Arc. No Mutex.

use crate::agent::cognition::history::Message;

/// Pure agent state - serializable, cloneable, deterministic
#[derive(Debug, Clone)]
pub struct AgentState {
    /// Message history
    pub history: Vec<Message>,
    
    /// Current step count
    pub step_count: usize,
    
    /// Maximum steps allowed
    pub max_steps: usize,
    
    /// Worker spawn count this turn
    pub delegation_count: usize,
    
    /// Maximum workers per turn
    pub max_delegations: usize,
    
    /// Tool rejection count this turn
    pub rejection_count: usize,
    
    /// Maximum rejections before giving up
    pub max_rejections: usize,
    
    /// Internal reasoning scratchpad
    pub scratchpad: String,
    
    /// Shutdown requested flag
    pub shutdown_requested: bool,
    
    /// Pending LLM request (waiting for response)
    pub pending_llm: bool,
    
    /// Pending approval (waiting for user)
    pub pending_approval: bool,
}

impl AgentState {
    /// Create initial state
    pub fn new(max_steps: usize) -> Self {
        Self {
            history: Vec::new(),
            step_count: 0,
            max_steps,
            delegation_count: 0,
            max_delegations: 10,
            rejection_count: 0,
            max_rejections: 2,
            scratchpad: String::new(),
            shutdown_requested: false,
            pending_llm: false,
            pending_approval: false,
        }
    }
    
    /// Check if can continue stepping
    pub fn can_continue(&self) -> bool {
        !self.shutdown_requested
            && self.step_count < self.max_steps
            && self.rejection_count < self.max_rejections
    }
    
    /// Check if at step limit
    pub fn at_limit(&self) -> bool {
        self.step_count >= self.max_steps
    }
    
    /// Check if too many rejections
    pub fn too_many_rejections(&self) -> bool {
        self.rejection_count >= self.max_rejections
    }
    
    /// Increment rejection counter, returns new state
    pub fn increment_rejection(mut self) -> Self {
        self.rejection_count += 1;
        self
    }
    
    /// Increment step counter, returns new state
    pub fn increment_step(mut self) -> Self {
        self.step_count += 1;
        self.delegation_count = 0;
        self.rejection_count = 0;
        self
    }
    
    /// Add message to history, returns new state
    pub fn with_message(mut self, message: Message) -> Self {
        self.history.push(message);
        self
    }
    
    /// Record delegation, returns new state
    pub fn with_delegation(mut self) -> Self {
        self.delegation_count += 1;
        self
    }
    
    /// Record rejection, returns new state
    pub fn with_rejection(mut self) -> Self {
        self.rejection_count += 1;
        self
    }
    
    /// Set pending LLM flag
    pub fn with_pending_llm(mut self, pending: bool) -> Self {
        self.pending_llm = pending;
        self
    }
    
    /// Set pending approval flag
    pub fn with_pending_approval(mut self, pending: bool) -> Self {
        self.pending_approval = pending;
        self
    }
    
    /// Request shutdown
    pub fn with_shutdown(mut self) -> Self {
        self.shutdown_requested = true;
        self
    }
    
    /// Append to scratchpad
    pub fn with_scratchpad(mut self, entry: impl Into<String>) -> Self {
        if !self.scratchpad.is_empty() {
            self.scratchpad.push('\n');
        }
        self.scratchpad.push_str(&entry.into());
        self
    }
}
