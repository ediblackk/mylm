//! Session input handlers
//!
//! Translates external input (chat, tasks, worker events) into SessionInput.
//! The Session consumes these and feeds them to the cognitive engine.
//!
//! Links:
//! - Used by: session (Session receives these as input)
//! - Uses: types (JobId, WorkerId)
//! - Handlers: chat.rs, task.rs, worker.rs

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
