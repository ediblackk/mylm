//! AgencyKernel trait - the pure, deterministic core
//!
//! The kernel is a state machine that:
//! - Takes events as input
//! - Returns a graph of intents as output
//! - Has NO async, NO IO, NO side effects
//! - Is fully deterministic and replayable

use super::{
    events::KernelEvent,
    graph::IntentGraph,
    config::KernelConfig,
    ContractError,
};

// Re-use Message type from intents
pub use crate::agent::types::intents::Message;

/// The AgencyKernel trait - pure cognitive core
///
/// Implementors of this trait provide the decision-making logic.
/// They produce INTENT (what to do), not execution.
///
/// # Purity Guarantees
/// - No async functions
/// - No IO operations
/// - No network calls
/// - No channel operations
/// - No thread spawning
/// - Deterministic: same input → same output
///
/// # Example Implementation
/// ```rust,ignore
/// pub struct LLMBasedKernel {
///     state: AgentState,
///     config: KernelConfig,
/// }
///
/// impl AgencyKernel for LLMBasedKernel {
///     fn init(&mut self, config: KernelConfig) -> Result<(), KernelError> {
///         self.config = config;
///         self.state = AgentState::new();
///         Ok(())
///     }
///
///     fn process(&mut self, events: &[KernelEvent]) -> Result<IntentGraph, KernelError> {
///         // Update state with events
///         for event in events {
///             self.state.apply(event);
///         }///
///         // Build LLM prompt from state
///         let prompt = self.build_prompt();
///
///         // ... (LLM would be called by runtime, not here)
///         // For now, emit simple intent based on state
///
///         Ok(IntentGraph::single(
///             IntentId::new(1),
///             Intent::EmitResponse("Hello".to_string()),
///         ))
///     }
///
///     fn state(&self) -> &AgentState {
///         &self.state
///     }
/// }
/// ```
pub trait AgencyKernel {
    /// Initialize the kernel with configuration
    ///
    /// This is NOT async. If runtime resources need initialization,
    /// that happens in the Runtime, not here.
    ///
    /// # Arguments
    /// * `config` - Pure configuration (descriptors only, no executors)
    ///
    /// # Errors
    /// Returns error if configuration is invalid
    fn init(&mut self, config: KernelConfig) -> Result<(), KernelError>;

    /// Process a batch of events, emit a graph of intents
    ///
    /// This is the core reducer function:
    /// (CurrentState, Events) → (NewState, IntentGraph)
    ///
    /// # Arguments
    /// * `events` - Batch of events that occurred since last process()
    ///
    /// # Returns
    /// A directed acyclic graph of intents to execute
    ///
    /// # Errors
    /// Returns error if events are invalid or processing fails
    fn process(&mut self, events: &[KernelEvent]) -> Result<IntentGraph, KernelError>;

    /// Get current state snapshot
    ///
    /// The state is immutable from the outside.
    /// Changes only happen through process().
    fn state(&self) -> &AgentState;

    /// Check if kernel has reached a terminal state
    ///
    /// Default implementation checks if state indicates completion
    fn is_terminal(&self) -> bool {
        // Default: check if halted
        self.state().step_count >= self.state().max_steps
    }
}

/// Errors that can occur in kernel processing
#[derive(Debug, Clone, PartialEq)]
pub enum KernelError {
    /// Invalid configuration
    InvalidConfig(String),

    /// Invalid input events
    InvalidInput(String),

    /// State machine error
    StateError(String),

    /// Policy violation
    PolicyViolation(String),

    /// Maximum steps reached
    MaxStepsReached,

    /// Internal error
    Internal(String),
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KernelError::InvalidConfig(msg) => write!(f, "Invalid kernel config: {}", msg),
            KernelError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            KernelError::StateError(msg) => write!(f, "State error: {}", msg),
            KernelError::PolicyViolation(msg) => write!(f, "Policy violation: {}", msg),
            KernelError::MaxStepsReached => write!(f, "Maximum steps reached"),
            KernelError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for KernelError {}

impl From<ContractError> for KernelError {
    fn from(e: ContractError) -> Self {
        KernelError::Internal(e.to_string())
    }
}

/// Agent state - the kernel's internal state
///
/// This is implementation-specific, but typically includes:
/// - Conversation history
/// - Step count
/// - Pending operations
/// - Context/scratchpad
#[derive(Debug, Clone, Default)]
pub struct AgentState {
    /// Current step number
    pub step_count: usize,

    /// Maximum allowed steps
    pub max_steps: usize,

    /// Conversation history (messages)
    pub history: Vec<Message>,

    /// Working memory / scratchpad
    pub scratchpad: String,

    /// Number of active workers
    pub active_workers: usize,

    /// Whether execution is halted
    pub halted: bool,

    /// Halt reason (if halted)
    pub halt_reason: Option<String>,

    /// Token usage so far
    pub token_usage: TokenUsage,

    /// Pending approval requests
    pub pending_approvals: Vec<PendingApproval>,
}

impl AgentState {
    /// Create initial state
    pub fn new() -> Self {
        Self {
            step_count: 0,
            max_steps: 50,
            history: Vec::new(),
            scratchpad: String::new(),
            active_workers: 0,
            halted: false,
            halt_reason: None,
            token_usage: TokenUsage::default(),
            pending_approvals: Vec::new(),
        }
    }

    /// Check if at step limit
    pub fn at_limit(&self) -> bool {
        self.step_count >= self.max_steps
    }

    /// Increment step counter
    pub fn increment_step(&mut self) {
        self.step_count += 1;
    }

    /// Mark as halted
    pub fn halt(&mut self, reason: impl Into<String>) {
        self.halted = true;
        self.halt_reason = Some(reason.into());
    }

    /// Check if there are pending approvals blocking progress
    pub fn has_pending_approvals(&self) -> bool {
        !self.pending_approvals.is_empty()
    }
}

/// Pending approval state
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub intent_id: super::ids::IntentId,
    pub tool: String,
    pub args: String,
    pub requested_at: std::time::SystemTime,
}

/// Token usage tracking
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub prompt: u32,
    pub completion: u32,
}

impl TokenUsage {
    pub fn total(&self) -> u32 {
        self.prompt + self.completion
    }

    pub fn add(&mut self, other: &TokenUsage) {
        self.prompt += other.prompt;
        self.completion += other.completion;
    }
}

/// A simple kernel implementation for testing
pub struct StubKernel {
    state: AgentState,
    config: KernelConfig,
}

impl StubKernel {
    pub fn new() -> Self {
        Self {
            state: AgentState::new(),
            config: KernelConfig::default(),
        }
    }
}

impl Default for StubKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl AgencyKernel for StubKernel {
    fn init(&mut self, config: KernelConfig) -> Result<(), KernelError> {
        self.state.max_steps = config.max_steps;
        self.config = config;
        Ok(())
    }

    fn process(&mut self, events: &[KernelEvent]) -> Result<IntentGraph, KernelError> {
        use super::intents::{Intent, ExitReason, IntentNode};
        use super::ids::IntentId;

        self.state.increment_step();

        // Simple stub: echo first user message or halt
        for event in events {
            if let super::events::KernelEvent::UserMessage { content } = event {
                let mut graph = IntentGraph::new();
                
                if self.state.at_limit() {
                    graph.add(IntentNode::new(
                        IntentId::new(1),
                        Intent::Halt(ExitReason::Completed),
                    ));
                } else {
                    graph.add(IntentNode::new(
                        IntentId::new(self.state.step_count as u64),
                        Intent::EmitResponse(format!("Echo: {}", content)),
                    ));
                }
                
                return Ok(graph);
            }
        }

        // No user message, return empty graph
        Ok(IntentGraph::new())
    }

    fn state(&self) -> &AgentState {
        &self.state
    }
}

// These are already imported at the top

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::events::KernelEvent;

    #[test]
    fn test_stub_kernel() {
        let mut kernel = StubKernel::new();
        kernel.init(KernelConfig::default()).unwrap();

        let events = vec![KernelEvent::UserMessage {
            content: "hello".to_string(),
        }];

        let graph = kernel.process(&events).unwrap();
        assert_eq!(graph.len(), 1);
        assert_eq!(kernel.state().step_count, 1);
    }

    #[test]
    fn test_step_limit() {
        let mut kernel = StubKernel::new();
        let config = KernelConfig::default().with_max_steps(2);
        kernel.init(config).unwrap();

        // Process first event
        kernel.process(&[KernelEvent::UserMessage {
            content: "first".to_string(),
        }]).unwrap();

        // Process second event
        kernel.process(&[KernelEvent::UserMessage {
            content: "second".to_string(),
        }]).unwrap();

        // Should be at limit now
        assert!(kernel.state().at_limit());
    }
}
