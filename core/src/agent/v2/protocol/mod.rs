//! Agent V2 Protocol Types
//!
//! Core types for agent decision-making and communication.

use crate::agent::tool::ToolKind;
use crate::llm::TokenUsage;

pub mod parser;
pub use parser::{parse_short_key_actions_from_content, ShortKeyAction};

// Re-export protocol types from the main agent::protocol module
pub use crate::agent::protocol::{AgentRequest, AgentResponse, AgentError};

/// The decision made by the agent after a step.
#[derive(Debug, Clone)]
pub enum AgentDecision {
    /// The LLM produced a text response (final answer or question).
    Message(String, TokenUsage),
    /// The LLM wants to execute a tool.
    Action {
        tool: String,
        args: String,
        kind: ToolKind,
    },
    /// The LLM output a tool call that couldn't be parsed correctly.
    MalformedAction(String),
    /// The agent has reached maximum iterations or an error occurred.
    Error(String),
}
