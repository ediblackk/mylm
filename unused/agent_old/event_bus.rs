//! Event Bus for agent communication
//!
//! This module re-exports the EventBus and CoreEvent from v2/orchestrator
//! since that's where the main implementation lives.

// Re-export from v2/orchestrator - this is the canonical location
pub use crate::agent_old::v2::orchestrator::{CoreEvent, EventBus};

// RuntimeEvent is a separate concept used by V1 agent
use crate::llm::TokenUsage;
use crate::agent_old::protocol::{AgentRequest, AgentResponse as ProtocolResponse};
use tokio::sync::oneshot;

/// Events that the core runtime emits during execution.
/// These will be mapped to the WebSocket protocol in the server.
/// NOTE: This is used by V1 agent. V2 uses CoreEvent.
#[derive(Debug)]
pub enum RuntimeEvent {
    StatusUpdate { message: String },
    AgentResponse { content: String, usage: TokenUsage },
    InternalObservation { data: Vec<u8> },
    SuggestCommand { command: String },
    Step { request: AgentRequest },
    ToolOutput { response: ProtocolResponse },
    /// Execute a terminal command (requires terminal access)
    ExecuteTerminalCommand { command: String, tx: oneshot::Sender<String> },
    /// Get the current terminal screen (requires terminal access)
    GetTerminalScreen { tx: oneshot::Sender<String> },
}
