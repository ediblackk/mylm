//! TUI Types Module
//!
//! This module consolidates type definitions used by the TUI layer.
//! It re-exports from both mylm_core (agent business logic) and provides
//! TUI-specific types, serving as the primary type namespace for the TUI.
//!
//! ## Architecture
//!
//! - **Agent/Business Logic Layer**: Types from mylm_core (Message, TokenUsage, etc.)
//! - **Connection/Networking Layer**: PTY management (PtyManager)
//! - **TUI Layer**: UI-specific types (AppState, Focus, HelpSystem, TuiEvent)
//! - **Shared Utilities**: StructuredScratchpad (shared between TUI and agent)

// ============================================================================
// Re-exports from mylm_core (Agent/Business Logic Layer)
// ============================================================================

// Chat/LLM types
pub use mylm_core::provider::TokenUsage;

// Use the real Message from mylm_core (with fallback to stub for compatibility)
pub use mylm_core::provider::chat::Message;

// Memory types
// pub use mylm_core::memory::graph::MemoryGraph;  // Currently unused

// Context management (from reorganized modules)
// pub use mylm_core::conversation::ContextManager;  // Currently unused
// pub use mylm_core::ui::{ActionStamp, ActionStampType};  // Currently unused

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
pub struct TimestampedMessage {
    /// The underlying chat message
    pub message: Message,
    /// Unix timestamp when the message was created (seconds since epoch)
    pub timestamp: i64,
    /// Generation time in milliseconds (for AI responses)
    pub generation_time_ms: Option<u64>,
}

impl TimestampedMessage {
    /// Create a new timestamped message with current time
    pub fn new(message: Message) -> Self {
        Self {
            message,
            timestamp: chrono::Utc::now().timestamp(),
            generation_time_ms: None,
        }
    }
    
    /// Create a new user message with timestamp
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Message::user(content))
    }
    
    /// Create a new assistant message with timestamp
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Message::assistant(content))
    }
    
    /// Create a new system message with timestamp
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Message::system(content))
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

impl From<Message> for TimestampedMessage {
    fn from(message: Message) -> Self {
        Self::new(message)
    }
}

impl From<TimestampedMessage> for Message {
    fn from(msg: TimestampedMessage) -> Self {
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
    pub context_window: usize,
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
        (0, self.context_window)
    }
}

/// Job registry for background jobs
#[derive(Debug, Clone)]
pub struct JobRegistry {
    jobs: std::sync::Arc<std::sync::Mutex<Vec<Job>>>,
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            jobs: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn list_all_jobs(&self) -> Vec<Job> {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn list_active_jobs(&self) -> Vec<Job> {
        self.jobs.lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|j| matches!(j.status, JobStatus::Running))
            .cloned()
            .collect()
    }
    
    pub fn add_job(&self, job: Job) {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).push(job);
    }
    
    pub fn update_job_status(&self, job_id: &str, status: JobStatus) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) {
                job.status = status;
            }
        }
    }

    pub fn update_job_error(&self, job_id: &str, error: String) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) {
                job.error = Some(error);
            }
        }
    }
    
    pub fn add_job_action(&self, job_id: &str, action_type: ActionType, description: impl Into<String>, content: impl Into<String>) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) {
                job.action_log.push(ActionLogEntry {
                    action_type,
                    description: description.into(),
                    content: content.into(),
                    timestamp: chrono::Local::now(),
                });
            }
        }
    }
    
    pub fn update_job_output(&self, job_id: &str, output: impl Into<String>) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) {
                job.output = output.into();
            }
        }
    }

    pub fn cancel_job(&self, job_id: &str) -> bool {
        self.update_job_status(job_id, JobStatus::Cancelled);
        true
    }

    pub fn cancel_all_jobs(&self) -> usize {
        let mut count = 0;
        if let Ok(mut jobs) = self.jobs.lock() {
            for job in jobs.iter_mut() {
                if matches!(job.status, JobStatus::Running) {
                    job.status = JobStatus::Cancelled;
                    count += 1;
                }
            }
        }
        count
    }
    
    pub fn update_job_metrics(&self, job_id: &str, tokens_used: usize, prompt_tokens: usize, completion_tokens: usize) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) {
                job.metrics.tokens_used += tokens_used;
                job.metrics.prompt_tokens += prompt_tokens;
                job.metrics.completion_tokens += completion_tokens;
                job.metrics.total_tokens += prompt_tokens + completion_tokens;
                job.metrics.request_count += 1;
            }
        }
    }
    
    pub fn update_job_cost(&self, job_id: &str, cost: f64) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) {
                job.metrics.cost += cost;
            }
        }
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
    AgentResponse(#[allow(dead_code)] Message, #[allow(dead_code)] TokenUsage),
    /// Tool output event
    ToolOutput(#[allow(dead_code)] String),
    /// Tool error event
    ToolError(#[allow(dead_code)] String),
    /// Configuration update event
    ConfigUpdate(#[allow(dead_code)] String),
    /// Condensed history after memory management
    CondensedHistory(#[allow(dead_code)] Vec<Message>),
    /// Status update from LLM client
    StatusUpdate(String),
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
pub use crate::tui::app::pty::PtyManager;

/// Spawn a new PTY with the given working directory
pub use crate::tui::app::pty::spawn_pty;
