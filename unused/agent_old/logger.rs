//! Debug logging system for agent operations.
//!
//! Provides ring-buffer based logging with optional file output.
//! Used for debugging agent behavior and tracking internal operations.
//! Includes convenience macros: `debug_log!`, `info_log!`, `error_log!`, `warn_log!`, `trace_log!`.
//! Supports structured logging with typed events and components.
//!
//! # Main Types
//! - `DebugLogger`: Ring buffer logger with file output support
//! - `DebugLogEntry`: Individual log entry with timestamp and metadata
//! - `StructuredLog`: Structured log entry with typed fields
//! - `LogComponent`: Component enumeration (Agent, Job, LLM, etc.)
//! - `LogEvent`: Event enumeration (JobCreated, ModelSelected, etc.)
//! - `LogTimer`: Duration tracking utility

use parking_lot::Mutex;
use std::sync::Arc;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use chrono::Local;
use std::sync::OnceLock;
use std::thread;
use serde_json::{json, Value};

pub struct DebugLogEntry {
    pub timestamp: String,
    pub level: String,
    pub module: String,
    pub thread_id: String,
    pub message: String,
}

/// Log level enumeration (must be before StructuredLog for Debug derive)
#[derive(Debug, Clone, Copy, PartialEq, Ord, PartialOrd, Eq)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl LogLevel {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "error" => LogLevel::Error,
            "warn" => LogLevel::Warn,
            "info" => LogLevel::Info,
            "debug" => LogLevel::Debug,
            "trace" => LogLevel::Trace,
            _ => LogLevel::Info, // default
        }
    }
}

/// Component enumeration for structured logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogComponent {
    Agent,
    Job,
    LLM,
    Context,
    Memory,
    Scheduler,
    UI,
    System,
}

impl LogComponent {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogComponent::Agent => "agent",
            LogComponent::Job => "job",
            LogComponent::LLM => "llm",
            LogComponent::Context => "context",
            LogComponent::Memory => "memory",
            LogComponent::Scheduler => "scheduler",
            LogComponent::UI => "ui",
            LogComponent::System => "system",
        }
    }
}

/// Event enumeration for structured logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogEvent {
    // Job events
    JobCreated,
    JobCompleted,
    JobFailed,
    JobCancelled,
    JobScheduled,
    JobStarted,
    JobPaused,
    JobResumed,

    // LLM events
    ModelSelected,
    ModelCall,
    ModelResponse,
    ModelError,
    TokenBudgetExceeded,
    ContextWindowFull,

    // Context events
    ContextPruned,
    ContextAdded,
    ContextRetrieved,
    ContextCompressed,

    // Memory events
    MemoryStored,
    MemoryRetrieved,
    MemorySearch,
    MemoryForgotten,
    MemoryCategorized,

    // Agent events
    AgentInitialized,
    AgentShutdown,
    AgentError,
    AgentStep,
    AgentThinking,
    AgentActing,

    // Scheduler events
    SchedulerTick,
    SchedulerTaskAssigned,
    SchedulerTaskCompleted,

    // UI events
    UIRendered,
    UIInputReceived,
    UINavigation,

    // System events
    SystemStartup,
    SystemShutdown,
    ConfigLoaded,
    ConfigChanged,
    ResourceWarning,
    ResourceError,
}

impl LogEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            // Job events
            LogEvent::JobCreated => "job_created",
            LogEvent::JobCompleted => "job_completed",
            LogEvent::JobFailed => "job_failed",
            LogEvent::JobCancelled => "job_cancelled",
            LogEvent::JobScheduled => "job_scheduled",
            LogEvent::JobStarted => "job_started",
            LogEvent::JobPaused => "job_paused",
            LogEvent::JobResumed => "job_resumed",

            // LLM events
            LogEvent::ModelSelected => "model_selected",
            LogEvent::ModelCall => "model_call",
            LogEvent::ModelResponse => "model_response",
            LogEvent::ModelError => "model_error",
            LogEvent::TokenBudgetExceeded => "token_budget_exceeded",
            LogEvent::ContextWindowFull => "context_window_full",

            // Context events
            LogEvent::ContextPruned => "context_pruned",
            LogEvent::ContextAdded => "context_added",
            LogEvent::ContextRetrieved => "context_retrieved",
            LogEvent::ContextCompressed => "context_compressed",

            // Memory events
            LogEvent::MemoryStored => "memory_stored",
            LogEvent::MemoryRetrieved => "memory_retrieved",
            LogEvent::MemorySearch => "memory_search",
            LogEvent::MemoryForgotten => "memory_forgotten",
            LogEvent::MemoryCategorized => "memory_categorized",

            // Agent events
            LogEvent::AgentInitialized => "agent_initialized",
            LogEvent::AgentShutdown => "agent_shutdown",
            LogEvent::AgentError => "agent_error",
            LogEvent::AgentStep => "agent_step",
            LogEvent::AgentThinking => "agent_thinking",
            LogEvent::AgentActing => "agent_acting",

            // Scheduler events
            LogEvent::SchedulerTick => "scheduler_tick",
            LogEvent::SchedulerTaskAssigned => "scheduler_task_assigned",
            LogEvent::SchedulerTaskCompleted => "scheduler_task_completed",

            // UI events
            LogEvent::UIRendered => "ui_rendered",
            LogEvent::UIInputReceived => "ui_input_received",
            LogEvent::UINavigation => "ui_navigation",

            // System events
            LogEvent::SystemStartup => "system_startup",
            LogEvent::SystemShutdown => "system_shutdown",
            LogEvent::ConfigLoaded => "config_loaded",
            LogEvent::ConfigChanged => "config_changed",
            LogEvent::ResourceWarning => "resource_warning",
            LogEvent::ResourceError => "resource_error",
        }
    }
}

/// Session ID management
static SESSION_ID: OnceLock<Arc<str>> = OnceLock::new();

/// Get the current session ID
pub fn get_session_id() -> Option<&'static str> {
    SESSION_ID.get().map(|s| s.as_ref())
}

/// Set the session ID (call once at startup)
pub fn set_session_id(id: &str) -> Result<(), String> {
    SESSION_ID.set(Arc::from(id)).map_err(|_| "Session ID already set".to_string())
}

/// Structured log entry with typed fields
#[derive(Debug, Clone)]
pub struct StructuredLog {
    pub timestamp: String,
    pub level: LogLevel,
    pub component: LogComponent,
    pub event: LogEvent,
    pub session_id: Option<Arc<str>>,
    pub job_id: Option<String>,
    pub parent_job_id: Option<String>,
    pub agent_id: Option<String>,
    pub model: Option<String>,
    pub tokens: Option<u32>,
    pub context_size: Option<usize>,
    pub message: String,
    pub details: Value,
}

impl StructuredLog {
    pub fn new(
        level: LogLevel,
        component: LogComponent,
        event: LogEvent,
        message: impl Into<String>,
    ) -> Self {
        let session_id = SESSION_ID.get().map(|s| s.clone());
        Self {
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            level,
            component,
            event,
            session_id,
            job_id: None,
            parent_job_id: None,
            agent_id: None,
            model: None,
            tokens: None,
            context_size: None,
            message: message.into(),
            details: json!({}),
        }
    }

    pub fn with_job_id(mut self, job_id: impl Into<String>) -> Self {
        self.job_id = Some(job_id.into());
        self
    }

    pub fn with_parent_job_id(mut self, parent_job_id: impl Into<String>) -> Self {
        self.parent_job_id = Some(parent_job_id.into());
        self
    }

    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_tokens(mut self, tokens: u32) -> Self {
        self.tokens = Some(tokens);
        self
    }

    pub fn with_context_size(mut self, context_size: usize) -> Self {
        self.context_size = Some(context_size);
        self
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn to_json(&self) -> String {
        let mut obj = json!({
            "timestamp": self.timestamp,
            "level": format_level(self.level),
            "component": self.component.as_str(),
            "event": self.event.as_str(),
            "message": self.message,
            "details": self.details,
        });

        if let Some(ref session_id) = self.session_id {
            obj["session_id"] = json!(session_id.as_ref());
        }
        if let Some(ref job_id) = self.job_id {
            obj["job_id"] = json!(job_id);
        }
        if let Some(ref parent_job_id) = self.parent_job_id {
            obj["parent_job_id"] = json!(parent_job_id);
        }
        if let Some(ref agent_id) = self.agent_id {
            obj["agent_id"] = json!(agent_id);
        }
        if let Some(ref model) = self.model {
            obj["model"] = json!(model);
        }
        if let Some(tokens) = self.tokens {
            obj["tokens"] = json!(tokens);
        }
        if let Some(context_size) = self.context_size {
            obj["context_size"] = json!(context_size);
        }

        serde_json::to_string(&obj).unwrap_or_else(|_| "{}".to_string())
    }
}

fn format_level(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "error",
        LogLevel::Warn => "warn",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
        LogLevel::Trace => "trace",
    }
}

/// Timer for measuring duration with structured logging support
pub struct LogTimer {
    start: Instant,
    component: LogComponent,
    event: LogEvent,
    message: String,
    job_id: Option<String>,
    agent_id: Option<String>,
    model: Option<String>,
}

impl LogTimer {
    pub fn new(component: LogComponent, event: LogEvent, message: impl Into<String>) -> Self {
        Self {
            start: Instant::now(),
            component,
            event,
            message: message.into(),
            job_id: None,
            agent_id: None,
            model: None,
        }
    }

    pub fn with_job_id(mut self, job_id: impl Into<String>) -> Self {
        self.job_id = Some(job_id.into());
        self
    }

    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn stop(self) -> Duration {
        self.stop_with_level(LogLevel::Info)
    }

    pub fn stop_with_level(self, level: LogLevel) -> Duration {
        let duration = self.start.elapsed();
        let ms = duration.as_millis() as u64;

        let mut log = StructuredLog::new(
            level,
            self.component,
            self.event,
            format!("{} ({}ms)", self.message, ms),
        );

        if let Some(job_id) = self.job_id {
            log = log.with_job_id(job_id);
        }
        if let Some(agent_id) = self.agent_id {
            log = log.with_agent_id(agent_id);
        }
        if let Some(model) = self.model {
            log = log.with_model(model);
        }
        log = log.with_details(json!({
            "duration_ms": ms,
            "duration_seconds": duration.as_secs_f64(),
        }));

        let logger = get_logger();
        let mut logger = logger.lock();
        logger.log_structured(log);

        duration
    }
}

pub struct DebugLogger {
    ring_buffer: VecDeque<DebugLogEntry>,
    structured_logs: VecDeque<StructuredLog>,
    max_entries: usize,
    file_path: Option<PathBuf>,
    local_file_path: Option<PathBuf>,
    min_level: LogLevel,
    session_id: OnceLock<Arc<str>>,
}

static LOGGER: OnceLock<Arc<Mutex<DebugLogger>>> = OnceLock::new();

fn get_logger() -> &'static Arc<Mutex<DebugLogger>> {
    LOGGER.get_or_init(|| Arc::new(Mutex::new(DebugLogger::new(1000))))
}

impl DebugLogger {
    pub fn new(max_entries: usize) -> Self {
        Self {
            ring_buffer: VecDeque::with_capacity(max_entries),
            structured_logs: VecDeque::with_capacity(max_entries),
            max_entries,
            file_path: None,
            local_file_path: None,
            min_level: LogLevel::Info,
            session_id: OnceLock::new(),
        }
    }

    pub fn set_file_path(&mut self, path: PathBuf) {
        // Ensure directory exists
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.file_path = Some(path);
    }

    pub fn set_local_file_path(&mut self, path: PathBuf) {
        // Ensure directory exists
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.local_file_path = Some(path);
    }

    pub fn set_min_level(&mut self, level: LogLevel) {
        self.min_level = level;
    }

    fn should_log(&self, level: LogLevel) -> bool {
        level as i32 <= self.min_level as i32
    }

    pub fn log(&mut self, level: LogLevel, module: &str, message: &str) {
        if !self.should_log(level) {
            return;
        }

        let level_str = match level {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        };

        let entry = DebugLogEntry {
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            level: level_str.to_string(),
            module: module.to_string(),
            thread_id: format!("{:?}", thread::current().id()),
            message: message.to_string(),
        };

        // Write to file if configured
        if let Some(path) = &self.file_path {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(
                    file,
                    "[{}] [{}] [{}] [thread {}] {}",
                    entry.timestamp, entry.level, entry.module, entry.thread_id, entry.message
                );
                let _ = file.flush();
            }
        }

        // Write to local debug.log in current working directory if configured
        if let Some(path) = &self.local_file_path {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(
                    file,
                    "[{}] [{}] [{}] [thread {}] {}",
                    entry.timestamp, entry.level, entry.module, entry.thread_id, entry.message
                );
                let _ = file.flush();
            }
        }

        // Add to ring buffer
        if self.ring_buffer.len() >= self.max_entries {
            self.ring_buffer.pop_front();
        }
        self.ring_buffer.push_back(entry);
    }

    /// Log a structured event
    pub fn log_structured(&mut self, mut log: StructuredLog) {
        if !self.should_log(log.level) {
            return;
        }

        // Ensure session ID is set from logger's session_id if not already set
        if log.session_id.is_none() {
            if let Some(session_id) = self.session_id.get() {
                log.session_id = Some(session_id.clone());
            }
        }

        // Write to file if configured (as JSON line)
        if let Some(path) = &self.file_path {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{}", log.to_json());
                let _ = file.flush();
            }
        }

        // Write to local debug.log in current working directory if configured
        if let Some(path) = &self.local_file_path {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{}", log.to_json());
                let _ = file.flush();
            }
        }

        // Add to structured logs buffer
        if self.structured_logs.len() >= self.max_entries {
            self.structured_logs.pop_front();
        }
        self.structured_logs.push_back(log);
    }

    /// Get recent structured logs as JSON strings
    pub fn get_recent_structured(&self, n: usize) -> Vec<String> {
        self.structured_logs
            .iter()
            .rev()
            .take(n)
            .map(|log| log.to_json())
            .collect()
    }

    pub fn get_recent(&self, n: usize) -> Vec<String> {
        self.ring_buffer
            .iter()
            .rev()
            .take(n)
            .map(|e| {
                format!(
                    "[{}] [{}] [{}] [thread {}] {}",
                    e.timestamp, e.level, e.module, e.thread_id, e.message
                )
            })
            .collect::<Vec<_>>()
    }
}

pub fn init(data_dir: PathBuf) {
    let logger = get_logger();
    let mut logger = logger.lock();
    logger.set_file_path(data_dir.join("debug.log"));
    // Also set local debug.log in current working directory for development
    logger.set_local_file_path(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("debug.log"));

    // Read log level from RUST_LOG env var, default to "info"
    let env_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let level = LogLevel::from_str(&env_level);
    logger.set_min_level(level);
}

pub fn log(level: LogLevel, module: &str, message: impl Into<String>) {
    let logger = get_logger();
    let mut logger = logger.lock();
    logger.log(level, module, &message.into());
}

/// Log with structured event (used by macros)
/// Writes only ONE line to log file (human-readable format)
pub fn log_with_event(
    level: LogLevel,
    component: LogComponent,
    event: LogEvent,
    message: impl Into<String>,
) {
    let logger = get_logger();
    let mut logger = logger.lock();

    // Convert message to String once
    let message_string = message.into();

    // Single line to file: human-readable format with component
    let module = match component {
        LogComponent::Agent => "agent",
        LogComponent::Job => "job",
        LogComponent::LLM => "llm",
        LogComponent::Context => "context",
        LogComponent::Memory => "memory",
        LogComponent::Scheduler => "scheduler",
        LogComponent::UI => "ui",
        LogComponent::System => "system",
    };
    logger.log(level, module, &message_string);

    // Store structured log in memory only (not written to file)
    let log = StructuredLog::new(level, component, event, message_string);
    if logger.structured_logs.len() >= logger.max_entries {
        logger.structured_logs.pop_front();
    }
    logger.structured_logs.push_back(log);
}

/// Infer LogComponent from module path
pub fn infer_component_from_module(module: &str) -> LogComponent {
    if module.contains("agent::v2") || module.contains("agent::v1") || module.contains("agent::core") || module.contains("agent::") {
        LogComponent::Agent
    } else if module.contains("job") || module.contains("jobs") {
        LogComponent::Job
    } else if module.contains("llm") || module.contains("model") {
        LogComponent::LLM
    } else if module.contains("context") {
        LogComponent::Context
    } else if module.contains("memory") {
        LogComponent::Memory
    } else if module.contains("scheduler") {
        LogComponent::Scheduler
    } else if module.contains("ui") || module.contains("terminal") {
        LogComponent::UI
    } else {
        LogComponent::System
    }
}

pub fn get_recent_logs(n: usize) -> Vec<String> {
    let logger = get_logger();
    let logger = logger.lock();
    logger.get_recent(n)
}

pub fn get_recent_structured_logs(n: usize) -> Vec<String> {
    let logger = get_logger();
    let logger = logger.lock();
    logger.get_recent_structured(n)
}
