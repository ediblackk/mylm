//! Input events
//! 
//! External stimuli fed into cognitive engine.
//! Pure enum - no runtime types.
//!
//! NOTE: Types are re-exported from types module for consistency.

// serde re-exported from types module

// Re-export types from unified types module
pub use crate::agent::types::events::{
    ToolResult, LLMResponse, ApprovalOutcome, WorkerId
};

/// Error from worker execution
#[derive(Debug, Clone)]
pub struct WorkerError {
    pub message: String,
}

/// External input events
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// User sent a message
    UserMessage(String),
    
    /// Worker completed or failed
    WorkerResult(WorkerId, Result<String, WorkerError>),
    
    /// Tool execution completed
    ToolResult(ToolResult),
    
    /// User responded to approval request
    ApprovalResult(ApprovalOutcome),
    
    /// LLM response arrived
    LLMResponse(LLMResponse),
    
    /// Runtime error occurred (e.g., LLM request failed)
    RuntimeError { intent_id: crate::agent::contract::ids::IntentId, error: String },
    
    /// Shutdown requested
    Shutdown,
    
    /// Heartbeat / tick (no external input)
    Tick,
}

impl InputEvent {
    /// Check if this input should reset turn counters
    pub fn is_new_turn(&self) -> bool {
        matches!(self, InputEvent::UserMessage(_))
    }
}
