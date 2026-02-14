//! Chat input handler

use crate::agent::session::input::SessionInput;

/// Handles user chat input
#[derive(Debug)]
pub struct ChatInputHandler;

impl ChatInputHandler {
    pub fn new() -> Self {
        Self
    }
    
    /// Process chat message into session input
    pub fn handle(&self, msg: String) -> SessionInput {
        SessionInput::Chat(msg)
    }
}

impl Default for ChatInputHandler {
    fn default() -> Self {
        Self::new()
    }
}
