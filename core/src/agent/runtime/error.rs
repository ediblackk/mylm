//! Runtime errors and AgencyRuntime trait
//!
//! This module contains both:
//! - The old capability-based error types (LLMError, ToolError, etc.)
//! - The new AgencyRuntime trait and its associated types

use std::fmt;
use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::agent::contract::{Intent, IntentGraph, Observation, IntentId};

// =============================================================================
// Capability Error Types (legacy)
// =============================================================================

/// LLM capability error
#[derive(Debug, Clone)]
pub struct LLMError {
    pub message: String,
}

impl LLMError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for LLMError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LLM error: {}", self.message)
    }
}

impl std::error::Error for LLMError {}

/// Tool capability error
#[derive(Debug, Clone)]
pub struct ToolError {
    pub message: String,
}

impl ToolError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tool error: {}", self.message)
    }
}

impl std::error::Error for ToolError {}

/// Approval capability error
#[derive(Debug, Clone)]
pub struct ApprovalError {
    pub message: String,
}

impl ApprovalError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for ApprovalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Approval error: {}", self.message)
    }
}

impl std::error::Error for ApprovalError {}

/// Worker capability error
#[derive(Debug, Clone)]
pub struct WorkerError {
    pub message: String,
}

impl WorkerError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Worker error: {}", self.message)
    }
}

impl std::error::Error for WorkerError {}

/// Legacy runtime error (aggregates all capability errors)
#[derive(Debug, Clone)]
pub enum RuntimeError {
    LLM(LLMError),
    Tool(ToolError),
    Approval(ApprovalError),
    Worker(WorkerError),
    Cancelled,
    Unknown(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::LLM(e) => write!(f, "{}", e),
            RuntimeError::Tool(e) => write!(f, "{}", e),
            RuntimeError::Approval(e) => write!(f, "{}", e),
            RuntimeError::Worker(e) => write!(f, "{}", e),
            RuntimeError::Cancelled => write!(f, "Operation cancelled"),
            RuntimeError::Unknown(msg) => write!(f, "Unknown error: {}", msg),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<LLMError> for RuntimeError {
    fn from(e: LLMError) -> Self {
        RuntimeError::LLM(e)
    }
}

impl From<ToolError> for RuntimeError {
    fn from(e: ToolError) -> Self {
        RuntimeError::Tool(e)
    }
}

impl From<ApprovalError> for RuntimeError {
    fn from(e: ApprovalError) -> Self {
        RuntimeError::Approval(e)
    }
}

impl From<WorkerError> for RuntimeError {
    fn from(e: WorkerError) -> Self {
        RuntimeError::Worker(e)
    }
}

// =============================================================================
// AgencyRuntime Trait (new contract-based runtime)
// =============================================================================

/// The AgencyRuntime trait - async side effect executor
///
/// Implementors provide the capability to execute intents.
/// This is where async, IO, network calls, and external interactions happen.
#[async_trait]
pub trait AgencyRuntime: Send + Sync {
    /// Execute a single intent
    async fn execute(&self, intent: Intent) -> Result<Observation, AgencyRuntimeError>;

    /// Execute a single intent with a specific intent_id
    async fn execute_with_id(&self, intent_id: IntentId, intent: Intent) -> Result<Observation, AgencyRuntimeError> {
        let mut obs = self.execute(intent).await?;
        obs.set_intent_id(intent_id);
        Ok(obs)
    }

    /// Execute a DAG of intents respecting dependencies
    async fn execute_dag(
        &self,
        graph: &IntentGraph,
    ) -> Result<Vec<(IntentId, Observation)>, AgencyRuntimeError>;

    /// Subscribe to telemetry events
    fn subscribe_telemetry(&self) -> broadcast::Receiver<TelemetryEvent>;

    /// Check if runtime is healthy
    async fn health_check(&self) -> HealthStatus;

    /// Graceful shutdown
    async fn shutdown(&self) -> Result<(), AgencyRuntimeError>;
}

/// Errors that can occur during agency runtime execution
#[derive(Debug, Clone)]
pub enum AgencyRuntimeError {
    ToolNotFound { tool: String },
    ToolExecutionFailed { tool: String, error: String },
    LLMRequestFailed { provider: String, error: String },
    Network { error: String, retryable: bool },
    RateLimited { retry_after_secs: Option<u64> },
    Authentication { provider: String },
    Timeout { intent_id: IntentId, duration_secs: u64 },
    NotAvailable { reason: String },
    Internal { message: String },
}

impl std::fmt::Display for AgencyRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgencyRuntimeError::ToolNotFound { tool } => write!(f, "Tool not found: {}", tool),
            AgencyRuntimeError::ToolExecutionFailed { tool, error } => {
                write!(f, "Tool '{}' failed: {}", tool, error)
            }
            AgencyRuntimeError::LLMRequestFailed { provider, error } => {
                write!(f, "LLM request to {} failed: {}", provider, error)
            }
            AgencyRuntimeError::Network { error, retryable } => {
                write!(f, "Network error (retryable={}): {}", retryable, error)
            }
            AgencyRuntimeError::RateLimited { retry_after_secs } => {
                if let Some(secs) = retry_after_secs {
                    write!(f, "Rate limited. Retry after {}s", secs)
                } else {
                    write!(f, "Rate limited")
                }
            }
            AgencyRuntimeError::Authentication { provider } => {
                write!(f, "Authentication failed for {}", provider)
            }
            AgencyRuntimeError::Timeout { intent_id, duration_secs } => {
                write!(f, "Intent {:?} timed out after {}s", intent_id, duration_secs)
            }
            AgencyRuntimeError::NotAvailable { reason } => write!(f, "Runtime not available: {}", reason),
            AgencyRuntimeError::Internal { message } => write!(f, "Runtime internal error: {}", message),
        }
    }
}

impl std::error::Error for AgencyRuntimeError {}

/// Telemetry event for monitoring/logging
#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    IntentStarted {
        intent_id: IntentId,
        intent_type: String,
        timestamp: std::time::SystemTime,
    },
    IntentCompleted {
        intent_id: IntentId,
        duration_ms: u64,
        success: bool,
    },
    TokenUsage {
        intent_id: IntentId,
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    ToolExecuted {
        tool: String,
        duration_ms: u64,
        success: bool,
    },
    WorkerSpawned {
        worker_id: crate::agent::types::events::WorkerId,
        objective: String,
    },
    WorkerCompleted {
        worker_id: crate::agent::types::events::WorkerId,
        duration_ms: u64,
    },
    Error {
        intent_id: Option<IntentId>,
        error: String,
    },
    Metrics {
        active_executions: usize,
        queued_executions: usize,
        memory_usage_mb: usize,
    },
}

/// Health status of the runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded { reason: &'static str },
    Unhealthy { reason: &'static str },
}

impl HealthStatus {
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }
}

/// Configuration for runtime execution
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub max_concurrent_tools: usize,
    pub max_concurrent_llm: usize,
    pub default_tool_timeout_secs: u64,
    pub default_llm_timeout_secs: u64,
    pub enable_parallelism: bool,
    pub retry_config: RetryConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tools: 10,
            max_concurrent_llm: 3,
            default_tool_timeout_secs: 60,
            default_llm_timeout_secs: 120,
            enable_parallelism: true,
            retry_config: RetryConfig::default(),
        }
    }
}

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub retryable_errors: Vec<RetryableErrorType>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 1000,
            max_delay_ms: 30000,
            retryable_errors: vec![
                RetryableErrorType::Network,
                RetryableErrorType::RateLimit,
                RetryableErrorType::Timeout,
            ],
        }
    }
}

/// Types of errors that can be retried
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryableErrorType {
    Network,
    RateLimit,
    Timeout,
    ServiceUnavailable,
}

/// Capability provider traits for runtime
#[async_trait]
pub trait ToolProvider: Send + Sync {
    fn list_tools(&self) -> Vec<crate::agent::types::intents::ToolSchema>;
    async fn execute(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<crate::agent::types::events::ToolResult, AgencyRuntimeError>;
    fn has_tool(&self, name: &str) -> bool;
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn list_models(&self) -> Result<Vec<String>, AgencyRuntimeError>;
    async fn complete(
        &self,
        request: crate::agent::types::intents::LLMRequest,
    ) -> Result<crate::agent::types::events::LLMResponse, AgencyRuntimeError>;
    async fn complete_stream(
        &self,
        request: crate::agent::types::intents::LLMRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<String>, AgencyRuntimeError>;
}

#[async_trait]
pub trait WorkerProvider: Send + Sync {
    async fn spawn(
        &self,
        spec: crate::agent::types::intents::WorkerSpec,
    ) -> Result<crate::agent::types::events::WorkerId, AgencyRuntimeError>;
    async fn status(
        &self,
        worker_id: crate::agent::types::events::WorkerId,
    ) -> Result<WorkerStatus, AgencyRuntimeError>;
    async fn cancel(&self, worker_id: crate::agent::types::events::WorkerId) -> Result<(), AgencyRuntimeError>;
}

/// Worker status
#[derive(Debug, Clone)]
pub enum WorkerStatus {
    Pending,
    Running { started_at: std::time::SystemTime },
    Completed { result: Result<String, crate::agent::types::events::WorkerError> },
    Failed { error: String },
    Cancelled,
}
