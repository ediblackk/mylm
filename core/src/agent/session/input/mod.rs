//! Session input handlers

pub mod chat;
pub mod task;
pub mod worker;

use crate::agent::types::JobId;

/// Unified session input event
#[derive(Debug, Clone)]
pub enum SessionInput {
    /// User chat message
    Chat(String),
    
    /// Single task execution request
    Task { command: String, args: Vec<String> },
    
    /// Worker event
    Worker(WorkerEvent),
    
    /// Approval response
    Approval(crate::agent::cognition::input::ApprovalOutcome),
    
    /// Interrupt
    Interrupt,
}

/// Worker-related events
#[derive(Debug, Clone)]
pub enum WorkerEvent {
    Spawned { job_id: JobId, description: String },
    Completed { job_id: JobId, result: String },
    Failed { job_id: JobId, error: String },
    Stalled { job_id: JobId, reason: String },
}
