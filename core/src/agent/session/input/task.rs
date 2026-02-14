//! Task input handler

use crate::agent::session::input::SessionInput;

/// Handles single task execution input
#[derive(Debug)]
pub struct TaskInputHandler;

impl TaskInputHandler {
    pub fn new() -> Self {
        Self
    }
    
    /// Process task command into session input
    pub fn handle(&self, command: String, args: Vec<String>) -> SessionInput {
        SessionInput::Task { command, args }
    }
}

impl Default for TaskInputHandler {
    fn default() -> Self {
        Self::new()
    }
}
