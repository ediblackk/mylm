use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::llm::TokenUsage;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageEnvelope<T> {
    pub v: u32,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub request_id: Option<Uuid>,
    pub event_id: Option<u64>,
    pub payload: T,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        client: ClientInfo,
        auth: AuthInfo,
    },
    CreateSession {
        profile: String,
        enable_terminal: bool,
    },
    ListSessions,
    ResumeSession {
        session_id: Uuid,
    },
    SendUserMessage {
        session_id: Uuid,
        message: UserMessage,
    },
    ApproveAction {
        session_id: Uuid,
        approval_id: Uuid,
        decision: String, // "approve" | "deny"
    },
    GetConfig {
        scope: String,
    },
    SetConfig {
        scope: String,
        patch: serde_json::Value,
    },
    TerminalInput {
        session_id: Uuid,
        data: String, // base64
    },
    TerminalResize {
        session_id: Uuid,
        cols: u16,
        rows: u16,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthInfo {
    pub mode: String,
    pub pairing_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserMessage {
    pub text: String,
    pub attachments: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    HelloAck {
        server: ServerInfo,
        capabilities: Capabilities,
    },
    SessionCreated {
        session_id: Uuid,
    },
    Sessions {
        sessions: Vec<SessionSummary>,
    },
    MessageStarted {
        session_id: Uuid,
        message_id: Uuid,
        role: String,
    },
    TokenDelta {
        session_id: Uuid,
        message_id: Uuid,
        seq: u64,
        text: String,
    },
    MessageFinal {
        session_id: Uuid,
        message_id: Uuid,
        text: String,
        usage: TokenUsage,
    },
    Activity {
        session_id: Uuid,
        kind: String,
        detail: Option<String>,
    },
    ToolCall {
        session_id: Uuid,
        tool: String,
        call_id: Uuid,
        input: serde_json::Value,
    },
    ToolResult {
        session_id: Uuid,
        tool: String,
        call_id: Uuid,
        ok: bool,
        output: serde_json::Value,
    },
    ApprovalRequested {
        session_id: Uuid,
        approval_id: Uuid,
        kind: String,
        summary: String,
        details: serde_json::Value,
    },
    TerminalOutput {
        session_id: Uuid,
        data: String, // base64
    },
    TerminalSnapshot {
        session_id: Uuid,
        text: String,
        cursor: CursorPos,
    },
    Config {
        config: serde_json::Value,
    },
    MemoryUpdate {
        session_id: Uuid,
        graph: serde_json::Value, // Replace with actual MemoryGraph serialization later
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Capabilities {
    pub terminal: bool,
    pub approvals: bool,
    pub tools: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionSummary {
    pub session_id: Uuid,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CursorPos {
    pub x: u16,
    pub y: u16,
}
