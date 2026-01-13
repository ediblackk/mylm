use serde::{Deserialize, Serialize};
use crate::llm::TokenUsage;

/// Events that the core runtime emits during execution.
/// These will be mapped to the WebSocket protocol in the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    StatusUpdate(String),
    AgentResponse(String, TokenUsage),
    InternalObservation(Vec<u8>),
    SuggestCommand(String),
    GetTerminalScreen(#[serde(skip)] tokio::sync::oneshot::Sender<String>),
    ExecuteTerminalCommand(String, #[serde(skip)] tokio::sync::oneshot::Sender<String>),
}
