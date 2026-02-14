//! AgencyRuntime trait - executes side effects
//!
//! The runtime is where all async operations, IO, and side effects live.
//! It executes intents produced by the kernel and returns observations.

use async_trait::async_trait;
use tokio::sync::broadcast;

use super::{
    intents::Intent,
    graph::IntentGraph,
    observations::Observation,
    ids::IntentId,
};

/// The AgencyRuntime trait - async side effect executor
///
/// Implementors provide the capability to execute intents.
/// This is where async, IO, network calls, and external interactions happen.
///
/// # Async Operations
/// All methods are async because execution involves:
/// - Network calls (LLM APIs)
/// - File system operations
/// - Process execution
/// - Timer/sleep for timeouts
///
/// # Swappable Implementations
/// - LocalRuntime: Execute on local machine
/// - RemoteRuntime: Delegate to remote service
/// - MockRuntime: For testing, returns canned responses
/// - RecordingRuntime: Records calls for replay
#[async_trait]
pub trait AgencyRuntime: Send + Sync {
    /// Execute a single intent
    ///
    /// # Arguments
    /// * `intent` - The intent to execute
    ///
    /// # Returns
    /// Observation result of execution
    async fn execute(&self, intent: Intent) -> Result<Observation, RuntimeError>;

    /// Execute a single intent with a specific intent_id
    ///
    /// This allows the runtime to properly track and report telemetry
    /// with the correct intent identifier.
    ///
    /// Default implementation delegates to `execute` and patches the intent_id.
    ///
    /// # Arguments
    /// * `intent_id` - The unique identifier for this intent
    /// * `intent` - The intent to execute
    ///
    /// # Returns
    /// Observation result of execution (intent_id is included in the observation)
    async fn execute_with_id(&self, intent_id: IntentId, intent: Intent) -> Result<Observation, RuntimeError> {
        let mut obs = self.execute(intent).await?;
        obs.set_intent_id(intent_id);
        Ok(obs)
    }

    /// Execute a DAG of intents respecting dependencies
    ///
    /// This is the primary execution method.
    /// The runtime:
    /// 1. Identifies ready intents (dependencies met)
    /// 2. Executes them (possibly in parallel)
    /// 3. Streams back observations as they complete
    /// 4. Continues until all intents complete
    ///
    /// # Arguments
    /// * `graph` - The intent DAG to execute
    ///
    /// # Returns
    /// Stream of (intent_id, observation) as they complete
    async fn execute_dag(
        &self,
        graph: &IntentGraph,
    ) -> Result<Vec<(IntentId, Observation)>, RuntimeError>;

    /// Subscribe to telemetry events
    ///
    /// These are for logging, metrics, monitoring - NOT control flow.
    /// Control flow happens through execute() return values.
    fn subscribe_telemetry(&self) -> broadcast::Receiver<TelemetryEvent>;

    /// Check if runtime is healthy
    async fn health_check(&self) -> HealthStatus;

    /// Graceful shutdown
    async fn shutdown(&self) -> Result<(), RuntimeError>;
}

/// Errors that can occur during runtime execution
#[derive(Debug, Clone)]
pub enum RuntimeError {
    /// Tool not found
    ToolNotFound { tool: String },

    /// Tool execution failed
    ToolExecutionFailed { tool: String, error: String },

    /// LLM request failed
    LLMRequestFailed { provider: String, error: String },

    /// Network error
    Network { error: String, retryable: bool },

    /// Rate limited
    RateLimited { retry_after_secs: Option<u64> },

    /// Authentication failed
    Authentication { provider: String },

    /// Timeout
    Timeout { intent_id: IntentId, duration_secs: u64 },

    /// Runtime not available
    NotAvailable { reason: String },

    /// Internal error
    Internal { message: String },
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::ToolNotFound { tool } => write!(f, "Tool not found: {}", tool),
            RuntimeError::ToolExecutionFailed { tool, error } => {
                write!(f, "Tool '{}' failed: {}", tool, error)
            }
            RuntimeError::LLMRequestFailed { provider, error } => {
                write!(f, "LLM request to {} failed: {}", provider, error)
            }
            RuntimeError::Network { error, retryable } => {
                write!(f, "Network error (retryable={}): {}", retryable, error)
            }
            RuntimeError::RateLimited { retry_after_secs } => {
                if let Some(secs) = retry_after_secs {
                    write!(f, "Rate limited. Retry after {}s", secs)
                } else {
                    write!(f, "Rate limited")
                }
            }
            RuntimeError::Authentication { provider } => {
                write!(f, "Authentication failed for {}", provider)
            }
            RuntimeError::Timeout { intent_id, duration_secs } => {
                write!(f, "Intent {:?} timed out after {}s", intent_id, duration_secs)
            }
            RuntimeError::NotAvailable { reason } => write!(f, "Runtime not available: {}", reason),
            RuntimeError::Internal { message } => write!(f, "Runtime internal error: {}", message),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Telemetry event for monitoring/logging
#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    /// Intent execution started
    IntentStarted {
        intent_id: IntentId,
        intent_type: String,
        timestamp: std::time::SystemTime,
    },

    /// Intent execution completed
    IntentCompleted {
        intent_id: IntentId,
        duration_ms: u64,
        success: bool,
    },

    /// Token usage for LLM call
    TokenUsage {
        intent_id: IntentId,
        prompt_tokens: u32,
        completion_tokens: u32,
    },

    /// Tool executed
    ToolExecuted {
        tool: String,
        duration_ms: u64,
        success: bool,
    },

    /// Worker spawned
    WorkerSpawned {
        worker_id: super::events::WorkerId,
        objective: String,
    },

    /// Worker completed
    WorkerCompleted {
        worker_id: super::events::WorkerId,
        duration_ms: u64,
    },

    /// Error occurred
    Error {
        intent_id: Option<IntentId>,
        error: String,
    },

    /// Metric snapshot
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
    /// Maximum concurrent tool executions
    pub max_concurrent_tools: usize,

    /// Maximum concurrent LLM requests
    pub max_concurrent_llm: usize,

    /// Default timeout for tool execution
    pub default_tool_timeout_secs: u64,

    /// Default timeout for LLM requests
    pub default_llm_timeout_secs: u64,

    /// Whether to enable parallel execution of independent intents
    pub enable_parallelism: bool,

    /// Retry configuration
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
    /// Maximum retry attempts
    pub max_attempts: u32,

    /// Base delay between retries (exponential backoff)
    pub base_delay_ms: u64,

    /// Maximum delay between retries
    pub max_delay_ms: u64,

    /// Which errors are retryable
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

/// Capability provider for runtime
///
/// Runtimes implement these to provide specific capabilities
#[async_trait]
pub trait ToolProvider: Send + Sync {
    /// List available tools
    fn list_tools(&self) -> Vec<super::events::ToolSchema>;

    /// Execute a tool
    async fn execute(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<super::events::ToolResult, RuntimeError>;

    /// Check if tool exists
    fn has_tool(&self, name: &str) -> bool;
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Get available models
    async fn list_models(&self) -> Result<Vec<String>, RuntimeError>;

    /// Complete a prompt
    async fn complete(
        &self,
        request: super::intents::LLMRequest,
    ) -> Result<super::events::LLMResponse, RuntimeError>;

    /// Stream completion (if supported)
    async fn complete_stream(
        &self,
        request: super::intents::LLMRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<String>, RuntimeError>;
}

#[async_trait]
pub trait WorkerProvider: Send + Sync {
    /// Spawn a worker
    async fn spawn(
        &self,
        spec: super::intents::WorkerSpec,
    ) -> Result<super::events::WorkerId, RuntimeError>;

    /// Check worker status
    async fn status(
        &self,
        worker_id: super::events::WorkerId,
    ) -> Result<WorkerStatus, RuntimeError>;

    /// Cancel a worker
    async fn cancel(&self, worker_id: super::events::WorkerId) -> Result<(), RuntimeError>;
}

/// Worker status
#[derive(Debug, Clone)]
pub enum WorkerStatus {
    Pending,
    Running { started_at: std::time::SystemTime },
    Completed { result: Result<String, super::events::WorkerError> },
    Failed { error: String },
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(!HealthStatus::Degraded { reason: "test" }.is_healthy());
        assert!(!HealthStatus::Unhealthy { reason: "test" }.is_healthy());
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.base_delay_ms, 1000);
    }

    #[test]
    fn test_runtime_config_default() {
        let config = RuntimeConfig::default();
        assert_eq!(config.max_concurrent_tools, 10);
        assert!(config.enable_parallelism);
    }
}
