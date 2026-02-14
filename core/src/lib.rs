//! Core library for mylm - AI agent system
//!
//! # Architecture
//! - `agent`: New capability-graph architecture (clean, deterministic)
//! - `agent_legacy`: Old V1/V2 code (quarantined, being replaced)

#![deny(unsafe_code)]

use std::sync::Mutex;

/// Global log file handle for debug.log
static DEBUG_LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Initialize debug.log file logging
pub fn init_debug_log(path: Option<std::path::PathBuf>) -> std::io::Result<()> {
    let log_path = path.unwrap_or_else(|| std::path::PathBuf::from("debug.log"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    *DEBUG_LOG.lock().unwrap() = Some(file);
    Ok(())
}

/// Write to debug.log if initialized
pub fn write_to_debug_log(level: &str, message: &str) {
    use std::io::Write;
    if let Ok(mut guard) = DEBUG_LOG.lock() {
        if let Some(ref mut file) = *guard {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let _ = writeln!(file, "[{}] [{}] {}", timestamp, level, message);
            let _ = file.flush();
        }
    }
}

// Logging macros - write ONLY to debug.log (not stderr to avoid TUI pollution)
#[macro_export]
macro_rules! info_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            $crate::write_to_debug_log("INFO", &msg);
        }
    };
}

#[macro_export]
macro_rules! error_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            $crate::write_to_debug_log("ERROR", &msg);
        }
    };
}

#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            $crate::write_to_debug_log("DEBUG", &msg);
        }
    };
}

#[macro_export]
macro_rules! warn_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            $crate::write_to_debug_log("WARN", &msg);
        }
    };
}

#[macro_export]
macro_rules! trace_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            $crate::write_to_debug_log("TRACE", &msg);
        }
    };
}

// NEW ARCHITECTURE
pub mod agent;

// Other modules
pub mod error;
pub mod terminal;
pub mod config;
pub mod context;
pub mod executor;
pub mod llm;
pub mod memory;
pub mod output;
pub mod scheduler;
pub mod state;
pub mod protocol;
pub mod util;
pub mod rate_limiter;

// TODO: Restore factory module or migrate to new architecture
// pub mod factory;

// TODO: Restore PaCoRe module if needed
// pub mod pacore;

// Re-exports from new architecture
pub use agent::{
    AgentState, AgentDecision, InputEvent, Transition, WorkerId,
    CognitiveEngine, CognitiveError, ApprovalOutcome,
    AgentRuntime, RuntimeContext, RuntimeError, CapabilityGraph, TraceId,
    LLMCapability, ToolCapability, ApprovalCapability, WorkerCapability, TelemetryCapability,
    Session, SessionConfig, SessionError, SessionInput, WorkerEvent,
    TaskId, JobId, SessionId,
    TokenUsage, ToolResult, Approval,
    // Cognitive engines
    LLMBasedEngine, ResponseParser,
    // Builder
    AgentBuilder, presets,
};

// Legacy re-exports (will be removed)
pub use error::{MylmError, Result};
pub use memory::store::VectorStore as MemoryStore;
