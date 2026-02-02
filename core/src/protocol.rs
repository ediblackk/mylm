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
    #[serde(alias = "CREATE_TASK")]
    CreateSession {
        #[serde(default)]
        profile: String,
        #[serde(default)]
        enable_terminal: bool,
        #[serde(default)]
        config: Option<serde_json::Value>,
    },
    ListSessions,
    #[serde(alias = "GET_PROJECT_INFO")]
    GetProjectInfo,
    #[serde(alias = "SWITCH_WORKSPACE")]
    SwitchWorkspace {
        path: String,
    },
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
    GetServerConfig,
    UpdateServerConfig {
        config: serde_json::Value,
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
    #[serde(alias = "GET_WORKFLOWS")]
    GetWorkflows,
    #[serde(alias = "SYNC_WORKFLOWS")]
    SyncWorkflows {
        workflows: Vec<Workflow>,
        stages: Vec<Stage>,
    },
    Ping,
    GetSystemInfo,
    TestConnection {
        provider: String,
        base_url: Option<String>,
        api_key: String,
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
    ProjectInfo {
        root_path: String,
        files: Vec<FileInfo>,
        #[serde(default)]
        stats: Option<ProjectStats>,
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
    StatusUpdate {
        session_id: Uuid,
        status: String,
    },
    TypingIndicator {
        session_id: Uuid,
        is_typing: bool,
    },
    JobsUpdate {
        session_id: Uuid,
        jobs: Vec<serde_json::Value>,
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
    Workflows {
        workflows: Vec<Workflow>,
        stages: Vec<Stage>,
    },
    SystemInfo {
        info: SystemInfo,
    },
    CreateSessionAck {
        session_id: Uuid,
    },
    Pong,
    ConnectionTestResult {
        ok: bool,
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolRequest {
    ReadFile { path: String },
    WriteFile { path: String, content: String },
    ListFiles { path: String },
    GetDiagnostics,
    ShowDiff { left: String, right: String, title: String },
    OpenFile { path: String, line: Option<u32> },
    RevealInExplorer { path: String },
    TerminalCreate { name: Option<String>, shell: Option<String> },
    TerminalInput { terminal_id: String, data: String },
    TerminalKill { terminal_id: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolResponse {
    pub ok: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectStats {
    pub file_count: u32,
    pub total_size: u64,
    pub loc: u32,
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
    pub status: String,
    pub created_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub is_directory: bool,
    pub children: Option<Vec<FileInfo>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CursorPos {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Workflow {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(rename = "stageIds")]
    pub stage_ids: Vec<String>,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Stage {
    pub id: String,
    #[serde(rename = "workflowId")]
    pub workflow_id: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(rename = "taskIds")]
    pub task_ids: Vec<String>,
    pub order: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SystemInfo {
    pub config_path: String,
    pub data_path: String,
    pub memory_db_path: String,
    pub sessions_path: String,
    pub workflows_path: String,
    pub version: String,
}
