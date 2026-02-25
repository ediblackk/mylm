//! GraphEngine trait - the pure, deterministic core
//!
//! The graph engine is a state machine that:
//! - Takes events as input
//! - Returns a graph of intents as output
//! - Has NO async, NO IO, NO side effects
//! - Is fully deterministic and replayable
//!
//! For single-step decision making, see `StepEngine` in `engine.rs`.

use crate::agent::types::graph::IntentGraph;
use crate::agent::types::ids::IntentId;
use crate::agent::types::intents::IntentNode;
use crate::agent::types::config::KernelConfig;
use crate::agent::types::error::ContractError;
use crate::agent::types::intents::{Intent, ExitReason};
use crate::agent::types::events::KernelEvent;
use crate::agent::cognition::decision::ToolCall;
use std::collections::HashMap;


/// Canonical Message type from conversation module
pub type Message = crate::conversation::manager::Message;

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
pub trait GraphEngine {
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

/// Agent state - unified state for both AgencyKernel and CognitiveEngine
///
/// Combines all state fields from the legacy architecture.
/// This is the single source of truth for agent cognition state.
#[derive(Debug, Clone)]
pub struct AgentState {
    /// Current step number
    pub step_count: usize,

    /// Maximum allowed steps
    pub max_steps: usize,

    /// Conversation history (messages)
    pub history: Vec<crate::conversation::manager::Message>,

    /// Working memory / scratchpad
    pub scratchpad: String,

    /// Worker spawn count this turn
    pub delegation_count: usize,

    /// Maximum workers per turn
    pub max_delegations: usize,

    /// Tool rejection count this turn
    pub rejection_count: usize,

    /// Maximum rejections before giving up
    pub max_rejections: usize,

    /// Shutdown requested flag
    pub shutdown_requested: bool,

    /// Pending LLM request (waiting for response)
    pub pending_llm: bool,

    /// Pending approval (waiting for user)
    pub pending_approval: bool,

    /// Pending tool call (waiting for approval)
    pub pending_tool: Option<ToolCall>,

    /// Number of active workers
    pub active_workers: usize,

    /// Whether execution is halted
    pub halted: bool,

    /// Halt reason (if halted)
    pub halt_reason: Option<String>,

    /// Token usage so far
    pub token_usage: TokenUsage,

    /// Pending approval requests (batch processing)
    pub pending_approvals: Vec<PendingApproval>,

    /// Monotonic intent sequence counter (unique ID generator)
    pub intent_seq: u64,

    /// Track LLM request retry attempts: intent_id -> retry_count
    pub llm_retry_counts: HashMap<IntentId, u32>,
}

impl AgentState {
    /// Create initial state with max_steps
    pub fn new(max_steps: usize) -> Self {
        Self {
            step_count: 0,
            max_steps,
            history: Vec::new(),
            scratchpad: String::new(),
            delegation_count: 0,
            max_delegations: 10,
            rejection_count: 0,
            max_rejections: 2,
            shutdown_requested: false,
            pending_llm: false,
            pending_approval: false,
            pending_tool: None,
            active_workers: 0,
            halted: false,
            halt_reason: None,
            token_usage: TokenUsage::default(),
            pending_approvals: Vec::new(),
            intent_seq: 0,
            llm_retry_counts: HashMap::new(),
        }
    }

    /// Create initial state with defaults
    pub fn default() -> Self {
        Self::new(50)
    }

    /// Check if can continue stepping
    pub fn can_continue(&self) -> bool {
        !self.shutdown_requested
            && !self.halted
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

    /// Increment step counter (mutable style)
    pub fn increment_step(&mut self) {
        self.step_count += 1;
        self.delegation_count = 0;
        self.rejection_count = 0;
    }

    /// Increment step counter, returns new state (immutable style for legacy compatibility)
    pub fn increment_step_immutable(mut self) -> Self {
        self.step_count += 1;
        self.delegation_count = 0;
        self.rejection_count = 0;
        self
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

    /// Increment rejection counter
    pub fn increment_rejection(&mut self) {
        self.rejection_count += 1;
    }

    /// Record delegation
    pub fn with_delegation(mut self) -> Self {
        self.delegation_count += 1;
        self
    }

    /// Add message to history
    pub fn with_message(mut self, role: &str, content: impl Into<String>) -> Self {
        self.history.push(crate::conversation::manager::Message::new(role, content));
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

    /// Set pending tool call
    pub fn with_pending_tool(mut self, tool: Option<ToolCall>) -> Self {
        self.pending_tool = tool;
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

    /// Increment and return next intent sequence number
    pub fn next_intent_seq(&mut self) -> u64 {
        self.intent_seq += 1;
        self.intent_seq
    }
}

/// Pending approval state
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub intent_id: IntentId,
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

/// A simple graph engine implementation for testing
pub struct StubGraphEngine {
    state: AgentState,
    config: KernelConfig,
}

impl StubGraphEngine {
    pub fn new() -> Self {
        Self {
            state: AgentState::default(),
            config: KernelConfig::default(),
        }
    }
}

impl Default for StubGraphEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphEngine for StubGraphEngine {
    fn init(&mut self, config: KernelConfig) -> Result<(), KernelError> {
        self.state.max_steps = config.max_steps;
        self.config = config;
        Ok(())
    }

    fn process(&mut self, events: &[KernelEvent]) -> Result<IntentGraph, KernelError> {
        self.state.increment_step();

        // Simple stub: echo first user message or halt
        for event in events {
            if let KernelEvent::UserMessage { content } = event {
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
    use crate::agent::types::events::KernelEvent;

    #[test]
    fn test_stub_kernel() {
        let mut kernel = StubGraphEngine::new();
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
        let mut kernel = StubGraphEngine::new();
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
