use serde::{Deserialize, Serialize};
use crate::llm::TokenUsage;
use crate::agent::protocol::{AgentRequest, AgentResponse as ProtocolResponse};

/// Events that the core runtime emits during execution.
/// These will be mapped to the WebSocket protocol in the server.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    StatusUpdate { message: String },
    AgentResponse { content: String, usage: TokenUsage },
    InternalObservation { data: Vec<u8> },
    SuggestCommand { command: String },
    #[serde(skip)]
    GetTerminalScreen {
        #[serde(skip)]
        tx: tokio::sync::oneshot::Sender<String>
    },
    #[serde(skip)]
    ExecuteTerminalCommand {
        command: String,
        #[serde(skip)]
        tx: tokio::sync::oneshot::Sender<String>
    },
    Step { request: AgentRequest },
    ToolOutput { response: ProtocolResponse },
}
