use crate::llm::TokenUsage;
use crate::llm::chat::ChatMessage;
use crate::memory::graph::MemoryGraph;
use crate::config::Config;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Thinking(String),      // Provider info
    Streaming(String),     // Progress or partial content
    ExecutingTool(String), // Tool name
    WaitingForUser,        // Auto-approve off
    Error(String),
}

pub enum TuiEvent {
    Input(crossterm::event::Event),
    Pty(Vec<u8>),
    PtyWrite(Vec<u8>),
    InternalObservation(Vec<u8>),
    AgentResponse(String, TokenUsage),
    AgentResponseFinal(String, TokenUsage),
    StatusUpdate(String),
    ActivityUpdate { summary: String, detail: Option<String> },
    CondensedHistory(Vec<ChatMessage>),
    ConfigUpdate(Config),
    SuggestCommand(String),
    ExecuteTerminalCommand(String, tokio::sync::oneshot::Sender<String>),
    GetTerminalScreen(tokio::sync::oneshot::Sender<String>),
    AppStateUpdate(AppState),
    MemoryGraphUpdate(MemoryGraph),
    Tick,
}
