use crate::llm::TokenUsage;
use crate::llm::chat::ChatMessage;
use crate::memory::graph::MemoryGraph;
use crate::config::ConfigV2 as Config;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Thinking(String),      // Provider info
    Streaming(String),     // Progress or partial content
    ExecutingTool(String), // Tool name
    WaitingForUser,        // Auto-approve off
    AwaitingApproval { tool: String, args: String }, // Waiting for user to approve/deny
    Error(String),
    ConfirmExit,           // Esc -> Confirmation dialog
    NamingSession,         // S -> Name the session
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
    PaCoReProgress { completed: usize, total: usize, current_round: usize, total_rounds: usize },
    Tick,
    /// Approval request for a tool execution
    AwaitingApproval {
        tool: String,
        args: String,
        tx: tokio::sync::oneshot::Sender<bool>,
    },
}
