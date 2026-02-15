//! TUI Types Module
//!
//! This module consolidates type definitions used by the TUI layer.
//! It re-exports from both mylm_core (agent business logic) and provides
//! TUI-specific types, serving as the primary type namespace for the TUI.
//!
//! ## Architecture
//!
//! - **Agent/Business Logic Layer**: Types from mylm_core (ChatMessage, TokenUsage, etc.)
//! - **Connection/Networking Layer**: PTY management (PtyManager)
//! - **TUI Layer**: UI-specific types (AppState, Focus, HelpSystem, TuiEvent)
//! - **Shared Utilities**: StructuredScratchpad (shared between TUI and agent)

// ============================================================================
// Re-exports from mylm_core (Agent/Business Logic Layer)
// ============================================================================

// Chat/LLM types
pub use mylm_core::llm::TokenUsage;

// Use the real ChatMessage from mylm_core (with fallback to stub for compatibility)
pub use mylm_core::llm::chat::ChatMessage;

// Memory types
// pub use mylm_core::memory::graph::MemoryGraph;  // Currently unused

// Context management
// pub use mylm_core::context::manager::ContextManager;  // Currently unused
// pub use mylm_core::context::action_stamp::{ActionStamp, ActionStampType};  // Currently unused

// Agent session contract types
// pub use mylm_core::agent::contract::session::{OutputEvent, UserInput};  // Currently unused

// ============================================================================
// TUI-Specific Types
// ============================================================================

// ---------------------------------------------------------------------------
// Chat Message with Metadata (timestamp, generation time)
// ---------------------------------------------------------------------------

/// Enhanced chat message with timestamp and generation time metadata
#[derive(Debug, Clone)]
pub struct TimestampedChatMessage {
    /// The underlying chat message
    pub message: ChatMessage,
    /// Unix timestamp when the message was created (seconds since epoch)
    pub timestamp: i64,
    /// Generation time in milliseconds (for AI responses)
    pub generation_time_ms: Option<u64>,
}

impl TimestampedChatMessage {
    /// Create a new timestamped message with current time
    pub fn new(message: ChatMessage) -> Self {
        Self {
            message,
            timestamp: chrono::Utc::now().timestamp(),
            generation_time_ms: None,
        }
    }
    
    /// Create a new user message with timestamp
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(ChatMessage::user(content))
    }
    
    /// Create a new assistant message with timestamp
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(ChatMessage::assistant(content))
    }
    
    /// Create a new system message with timestamp
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(ChatMessage::system(content))
    }
    
    /// Set the generation time
    pub fn with_generation_time(mut self, ms: u64) -> Self {
        self.generation_time_ms = Some(ms);
        self
    }
    
    /// Get formatted timestamp for display
    pub fn formatted_time(&self) -> String {
        use chrono::{DateTime, Local, Utc};
        let dt = DateTime::<Utc>::from_timestamp(self.timestamp, 0)
            .map(|dt| dt.with_timezone(&Local))
            .unwrap_or_else(|| Local::now());
        dt.format("%H:%M").to_string()
    }
}

impl From<ChatMessage> for TimestampedChatMessage {
    fn from(message: ChatMessage) -> Self {
        Self::new(message)
    }
}

impl From<TimestampedChatMessage> for ChatMessage {
    fn from(msg: TimestampedChatMessage) -> Self {
        msg.message
    }
}

// ---------------------------------------------------------------------------
// Application State (TUI State Machine)
// ---------------------------------------------------------------------------

/// Application state enum - represents the current state of the TUI
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum AppState {
    /// Idle - ready for user input
    Idle,
    /// Agent is thinking/processing
    Thinking(String),
    /// Streaming response from agent
    Streaming(String),
    /// Executing a tool
    ExecutingTool(String),
    /// Waiting for user input (auto-approve mode)
    WaitingForUser,
    /// Waiting for user approval of a tool
    AwaitingApproval { tool: String, args: String },
    /// Error state
    Error(String),
    /// Confirming exit
    ConfirmExit,
    /// Naming session
    NamingSession,
}

// ---------------------------------------------------------------------------
// Focus (TUI Panel Navigation)
// ---------------------------------------------------------------------------

/// Focus enum - represents which panel has keyboard focus
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    /// Terminal panel focused
    Terminal,
    /// Chat panel focused
    Chat,
    /// Jobs panel focused
    Jobs,
}

// ---------------------------------------------------------------------------
// Job System Types (TUI-specific for background jobs panel)
// ---------------------------------------------------------------------------

/// Job status for background jobs display
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    TimeoutPending,
    Stalled,
}

/// Action type for job tracking
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum ActionType {
    Shell,
    Read,
    Write,
    Search,
    Ask,
    Done,
    ToolCall,
    Thought,
    ToolResult,
    Error,
    FinalAnswer,
    System,
}

// ---------------------------------------------------------------------------
// Session Monitoring Types (TUI display)
// ---------------------------------------------------------------------------

// Session types (Session, SessionMetadata, SessionStats, SessionMonitor) are defined in session.rs
// Re-exported at the top of this module via: pub use crate::tui::session::{Session, SessionMonitor};

// ---------------------------------------------------------------------------
// Structured Scratchpad (Shared state between TUI and agent)
// ---------------------------------------------------------------------------

/// Structured scratchpad for agent/TUI shared state
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct StructuredScratchpad;

#[allow(dead_code)]
impl StructuredScratchpad {
    pub fn new() -> Self {
        Self::default()
    }
}

// ---------------------------------------------------------------------------
// Job Registry (TUI background jobs panel)
// ---------------------------------------------------------------------------

/// Job entry for background jobs display
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Job {
    pub id: String,
    pub status: JobStatus,
    pub description: String,
    pub tool_name: String,
    pub action_log: Vec<ActionLogEntry>,
    pub output: String,
    pub error: Option<String>,
    pub metrics: JobMetrics,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct JobMetrics {
    pub tokens_used: usize,
    pub cost: f64,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub request_count: usize,
    pub error_count: usize,
    pub rate_limit_hits: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ActionLogEntry {
    pub action_type: ActionType,
    pub description: String,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Local>,
}

impl Job {
    #[allow(dead_code)]
    pub fn current_action(&self) -> Option<&str> {
        self.action_log.last().map(|e| e.description.as_str())
    }

    #[allow(dead_code)]
    pub fn context_window(&self) -> (usize, usize) {
        (0, 8192) // Could use real context manager
    }
}

/// Job registry for background jobs
#[derive(Debug, Clone, Default)]
pub struct JobRegistry;

impl JobRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn list_all_jobs(&self) -> Vec<Job> {
        // Returns empty list until scheduler integration
        Vec::new()
    }

    pub fn list_active_jobs(&self) -> Vec<Job> {
        // Returns empty list until scheduler integration
        Vec::new()
    }

    pub fn cancel_job(&self, _job_id: &str) -> bool {
        // Stub implementation - no jobs to cancel
        false
    }

    pub fn cancel_all_jobs(&self) -> usize {
        // Stub implementation - no jobs to cancel
        0
    }
}

// ---------------------------------------------------------------------------
// TUI Events (TUI event loop)
// ---------------------------------------------------------------------------

/// TUI event types for the event loop
#[derive(Debug)]
#[allow(dead_code)]
pub enum TuiEvent {
    /// Tick event for periodic updates
    Tick,
    /// User input event
    Input(crossterm::event::Event),
    /// PTY data received
    Pty(Vec<u8>),
    /// Agent response event
    AgentResponse(#[allow(dead_code)] ChatMessage, #[allow(dead_code)] TokenUsage),
    /// Tool output event
    ToolOutput(#[allow(dead_code)] String),
    /// Tool error event
    ToolError(#[allow(dead_code)] String),
    /// Configuration update event
    ConfigUpdate(#[allow(dead_code)] String),
    /// Condensed history after memory management
    CondensedHistory(#[allow(dead_code)] Vec<ChatMessage>),
}

// ---------------------------------------------------------------------------
// Streaming Parser Types (TUI JSON streaming)
// ---------------------------------------------------------------------------

/// Streaming JSON parser state machine for LLM response parsing
/// 
/// Uses pattern-based detection:
/// - `{"t": "..."` for thought field
/// - `: "..."` for final answer (after "f" key)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[allow(dead_code)]
pub enum StreamState {
    #[default]
    /// Looking for {"t": pattern
    LookingForThought,
    /// Found {, looking for "t":
    SawOpenBrace,
    /// Found {"t, looking for "":
    SawThoughtT,
    /// Found {"t":, waiting for thought value
    ExpectingThoughtValue,
    /// Streaming thought content
    InThoughtValue,
    /// Looking for "f": pattern after thought or at start
    LookingForFinal,
    /// Found "f", looking for :
    SawFinalF,
    /// Found "f":, waiting for final value
    ExpectingFinalValue,
    /// Streaming final answer content
    InFinalValue,
    /// Finished streaming final - ignore rest
    Done,
}

// ---------------------------------------------------------------------------
// PTY Types (Connection/Networking Layer)
// ---------------------------------------------------------------------------

/// PTY manager for terminal emulation
/// This is the real implementation from src/tui/pty.rs
pub use crate::tui::pty::PtyManager;

/// Spawn a new PTY with the given working directory
pub use crate::tui::pty::spawn_pty;
