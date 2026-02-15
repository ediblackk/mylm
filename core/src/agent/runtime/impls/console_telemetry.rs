//! Console Telemetry
//!
//! Logs decisions and results to console/file for debugging.

use crate::agent::runtime::{
    capability::{Capability, TelemetryCapability},
    context::RuntimeContext,
};
use crate::agent::cognition::{AgentDecision, InputEvent};
use std::sync::Arc;
use tokio::sync::Mutex;
use chrono::Local;

/// Console telemetry - logs to stdout/file
pub struct ConsoleTelemetry {
    verbose: bool,
    log_file: Option<Arc<Mutex<tokio::fs::File>>>,
}

impl ConsoleTelemetry {
    pub fn new() -> Self {
        Self {
            verbose: true,
            log_file: None,
        }
    }
    
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }
    
    /// Create with file logging
    pub async fn with_file(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        
        Ok(Self {
            verbose: true,
            log_file: Some(Arc::new(Mutex::new(file))),
        })
    }
    
    async fn log(&self, message: impl AsRef<str>) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let line = format!("[{}] {}\n", timestamp, message.as_ref());
        
        if self.verbose {
            eprintln!("{}", line.trim_end());
        }
        
        if let Some(ref file) = self.log_file {
            use tokio::io::AsyncWriteExt;
            let mut file = file.lock().await;
            let _ = file.write_all(line.as_bytes()).await;
        }
    }
}

impl Default for ConsoleTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Capability for ConsoleTelemetry {
    fn name(&self) -> &'static str {
        "console-telemetry"
    }
}

#[async_trait::async_trait]
impl TelemetryCapability for ConsoleTelemetry {
    async fn record_decision(&self, ctx: &RuntimeContext, decision: &AgentDecision) {
        let trace_id = &ctx.trace_id;
        let decision_str = format_decision(decision);
        self.log(format!("[{:?}] DECISION: {}", trace_id, decision_str)).await;
    }
    
    async fn record_result(&self, ctx: &RuntimeContext, event: &InputEvent) {
        let trace_id = &ctx.trace_id;
        let event_str = format_event(event);
        self.log(format!("[{:?}] RESULT: {}", trace_id, event_str)).await;
    }
}

fn format_decision(decision: &AgentDecision) -> String {
    match decision {
        AgentDecision::CallTool(call) => format!(
            "CallTool(tool={}, args={})", 
            call.name, 
            truncate(&call.arguments.to_string(), 50)
        ),
        AgentDecision::RequestLLM(req) => format!(
            "RequestLLM(scratchpad={})", 
            truncate(&req.context.scratchpad, 50)
        ),
        AgentDecision::RequestApproval(req) => format!(
            "RequestApproval(tool={})", 
            req.tool
        ),
        AgentDecision::SpawnWorker(spec) => format!(
            "SpawnWorker(objective={})", 
            truncate(&spec.objective, 30)
        ),
        AgentDecision::EmitResponse(resp) => format!(
            "EmitResponse({})", 
            truncate(resp, 50)
        ),
        AgentDecision::Exit(reason) => format!("Exit({:?})", reason),
        AgentDecision::None => "None".to_string(),
    }
}

fn format_event(event: &InputEvent) -> String {
    match event {
        InputEvent::UserMessage(msg) => format!("UserMessage({})", truncate(msg, 50)),
        InputEvent::ToolResult { tool, result } => {
            let success = match result {
                crate::agent::types::events::ToolResult::Success { .. } => true,
                crate::agent::types::events::ToolResult::Error { .. } => false,
                crate::agent::types::events::ToolResult::Cancelled => false,
            };
            format!("ToolResult(tool={}, success={})", tool, success)
        }
        InputEvent::LLMResponse(resp) => format!(
            "LLMResponse(tokens={})", 
            resp.usage.total_tokens
        ),
        InputEvent::ApprovalResult(outcome) => format!("ApprovalResult({:?})", outcome),
        InputEvent::WorkerResult(id, result) => format!(
            "WorkerResult(id={:?}, success={})", 
            id, 
            result.is_ok()
        ),
        InputEvent::Shutdown => "Shutdown".to_string(),
        InputEvent::Tick => "Tick".to_string(),
        InputEvent::RuntimeError { intent_id, error } => {
            format!("RuntimeError(id={:?}, error={})", intent_id, truncate(error, 50))
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// Metrics collector for performance tracking
pub struct MetricsCollector {
    decision_count: std::sync::atomic::AtomicU64,
    tool_count: std::sync::atomic::AtomicU64,
    llm_count: std::sync::atomic::AtomicU64,
    start_time: std::time::Instant,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            decision_count: std::sync::atomic::AtomicU64::new(0),
            tool_count: std::sync::atomic::AtomicU64::new(0),
            llm_count: std::sync::atomic::AtomicU64::new(0),
            start_time: std::time::Instant::now(),
        }
    }
    
    pub fn record_decision(&self) {
        self.decision_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    
    pub fn record_tool(&self) {
        self.tool_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    
    pub fn record_llm(&self) {
        self.llm_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    
    pub fn report(&self) -> MetricsReport {
        MetricsReport {
            total_decisions: self.decision_count.load(std::sync::atomic::Ordering::Relaxed),
            tool_calls: self.tool_count.load(std::sync::atomic::Ordering::Relaxed),
            llm_calls: self.llm_count.load(std::sync::atomic::Ordering::Relaxed),
            elapsed_secs: self.start_time.elapsed().as_secs(),
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics report
#[derive(Debug, Clone)]
pub struct MetricsReport {
    pub total_decisions: u64,
    pub tool_calls: u64,
    pub llm_calls: u64,
    pub elapsed_secs: u64,
}

impl std::fmt::Display for MetricsReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,
            "Session Metrics:\n\
             - Total decisions: {}\n\
             - Tool calls: {}\n\
             - LLM calls: {}\n\
             - Elapsed: {}s",
            self.total_decisions,
            self.tool_calls,
            self.llm_calls,
            self.elapsed_secs
        )
    }
}
