//! Worker input handler

use crate::agent::session::input::{SessionInput, WorkerEvent};
use crate::agent::types::JobId;

/// Handles worker events
#[derive(Debug)]
pub struct WorkerInputHandler;

impl WorkerInputHandler {
    pub fn new() -> Self {
        Self
    }
    
    /// Convert worker event to session input
    pub fn handle(&self, event: WorkerEvent) -> SessionInput {
        SessionInput::Worker(event)
    }
    
    /// Create spawned event
    pub fn spawned(&self, job_id: JobId, description: String) -> SessionInput {
        SessionInput::Worker(WorkerEvent::Spawned { job_id, description })
    }
    
    /// Create completed event
    pub fn completed(&self, job_id: JobId, result: String) -> SessionInput {
        SessionInput::Worker(WorkerEvent::Completed { job_id, result })
    }
}

impl Default for WorkerInputHandler {
    fn default() -> Self {
        Self::new()
    }
}
