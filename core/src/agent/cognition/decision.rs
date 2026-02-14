//! Agent decisions
//! 
//! INTENT only. No execution. Side effects expressed as data.
//! 
//! NOTE: Types are re-exported from types::intents for consistency.

use serde::{Deserialize, Serialize};

// Re-export types from unified types module
pub use crate::agent::types::intents::{
    ToolCall, LLMRequest, WorkerSpec, ApprovalRequest
};

/// Reason for agent exit
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentExitReason {
    Complete,
    StepLimit,
    UserRequest,
    Error(String),
}

/// Agent decision - pure intent, no execution
#[derive(Debug, Clone)]
pub enum AgentDecision {
    /// Call a tool
    CallTool(ToolCall),
    
    /// Spawn a worker
    SpawnWorker(WorkerSpec),
    
    /// Request user approval
    RequestApproval(ApprovalRequest),
    
    /// Request LLM completion
    RequestLLM(LLMRequest),
    
    /// Emit response to user
    EmitResponse(String),
    
    /// Exit agent
    Exit(AgentExitReason),
    
    /// No action this step
    None,
}

impl AgentDecision {
    /// Check if decision requires external fulfillment
    pub fn requires_external(&self) -> bool {
        matches!(
            self,
            AgentDecision::CallTool(_)
                | AgentDecision::SpawnWorker(_)
                | AgentDecision::RequestApproval(_)
                | AgentDecision::RequestLLM(_)
        )
    }
}

/// State transition produced by cognitive step
#[derive(Debug, Clone)]
pub struct Transition {
    /// Next state (always provided)
    pub next_state: crate::agent::cognition::state::AgentState,
    
    /// Decision to execute (if any)
    pub decision: AgentDecision,
}

impl Transition {
    /// Create transition with decision
    pub fn new(next_state: crate::agent::cognition::state::AgentState, decision: AgentDecision) -> Self {
        Self {
            next_state,
            decision,
        }
    }
    
    /// Create transition with no action
    pub fn none(next_state: crate::agent::cognition::state::AgentState) -> Self {
        Self {
            next_state,
            decision: AgentDecision::None,
        }
    }
    
    /// Create exit transition
    pub fn exit(next_state: crate::agent::cognition::state::AgentState, reason: AgentExitReason) -> Self {
        Self {
            next_state,
            decision: AgentDecision::Exit(reason),
        }
    }
}
