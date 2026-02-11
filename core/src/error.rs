//! Structured error types for Mylm
//!
//! Provides type-safe error handling with rich context for debugging,
//! user-friendly messages, and telemetry integration.

use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

/// Primary error type for Mylm operations
#[derive(Error, Debug)]
pub enum MylmError {
    // =========================================================================
    // Provider / API Errors
    // =========================================================================
    /// Authentication/authorization errors
    #[error("unauthorized: {message}")]
    Unauthorized { message: String },

    /// Invalid API key or token
    #[error("invalid credentials: {reason}")]
    InvalidCredentials { reason: String },

    /// Token expired
    #[error("authentication token expired")]
    TokenExpired,

    /// Rate limit exceeded (429)
    #[error("rate limit exceeded: {limit_type}")]
    RateLimitExceeded { limit_type: String },

    /// Quota/billing limit reached
    #[error("quota exceeded. Plan: {plan_type}")]
    QuotaExceeded { plan_type: String },

    /// Model context window full
    #[error("context window exceeded. Max: {max_tokens}, Used: {used_tokens}")]
    ContextWindowExceeded {
        max_tokens: usize,
        used_tokens: usize,
    },

    /// Model is at capacity/unavailable
    #[error("model {model} is at capacity")]
    ModelCapacity {
        model: String,
        retry_after: Option<Duration>,
    },

    /// Provider returned an error
    #[error("provider error: {status} - {message}")]
    ProviderError {
        status: u16,
        message: String,
    },

    // =========================================================================
    // Thread / Session Errors (ThreadManager pattern)
    // =========================================================================
    /// Thread not found
    #[error("thread not found: {0}")]
    ThreadNotFound(String),

    /// Agent thread limit reached
    #[error("agent thread limit reached (max {max_threads})")]
    ThreadLimitReached { max_threads: usize },

    /// Session not found
    #[error("session not found: {session_id}")]
    SessionNotFound { session_id: String },

    /// Session expired
    #[error("session expired: {session_id}")]
    SessionExpired { session_id: String },

    // =========================================================================
    // Approval / Safety Errors
    // =========================================================================
    /// Approval request timeout
    #[error("approval request timeout after {duration:?}")]
    ApprovalTimeout { duration: Duration },

    /// Approval denied by user
    #[error("approval denied for: {action}")]
    ApprovalDenied { action: String },

    /// Approval queue full
    #[error("approval queue full (max {max_size})")]
    ApprovalQueueFull { max_size: usize },

    /// Sandbox / permission denied
    #[error("sandbox denied: {reason}")]
    SandboxDenied { reason: String },

    /// Command forbidden by policy
    #[error("command forbidden: {command}")]
    CommandForbidden { command: String },

    // =========================================================================
    // Tool Execution Errors
    // =========================================================================
    /// Tool not found
    #[error("tool not found: {tool_name}")]
    ToolNotFound { tool_name: String },

    /// Tool execution failed
    #[error("tool execution failed: {tool_name} - {error}")]
    ToolExecutionFailed { tool_name: String, error: String },

    /// Tool timeout
    #[error("tool timeout: {tool_name} after {duration:?}")]
    ToolTimeout { tool_name: String, duration: Duration },

    /// Invalid tool arguments
    #[error("invalid tool arguments for {tool_name}: {reason}")]
    InvalidToolArguments { tool_name: String, reason: String },

    // =========================================================================
    // Rollout / Persistence Errors
    // =========================================================================
    /// Rollout log corrupted
    #[error("rollout log corrupted: {path} at line {line}")]
    RolloutCorrupted { path: PathBuf, line: usize },

    /// Rollout write failed
    #[error("rollout write failed: {path}")]
    RolloutWriteFailed { path: PathBuf },

    /// Rollout read failed
    #[error("rollout read failed: {path}")]
    RolloutReadFailed { path: PathBuf },

    // =========================================================================
    // Configuration Errors
    // =========================================================================
    /// Invalid configuration
    #[error("invalid configuration: {message}")]
    InvalidConfig { message: String },

    /// Profile not found
    #[error("profile not found: {profile_name}")]
    ProfileNotFound { profile_name: String },

    /// Missing required config
    #[error("missing required configuration: {key}")]
    MissingConfig { key: String },

    // =========================================================================
    // User Input Errors
    // =========================================================================
    /// Invalid user input
    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    /// Invalid file path
    #[error("invalid file path: {path}")]
    InvalidPath { path: String },

    /// File not found
    #[error("file not found: {path}")]
    FileNotFound { path: PathBuf },

    // =========================================================================
    // Network / System Errors
    // =========================================================================
    /// Network/connection error
    #[error("connection failed: {message}")]
    ConnectionFailed { message: String },

    /// Timeout
    #[error("operation timed out after {duration:?}")]
    Timeout { duration: Duration },

    /// Stream disconnected (retryable)
    #[error("stream disconnected: {reason}")]
    StreamDisconnected { reason: String },

    /// Service unavailable (maintenance, 503)
    #[error("service temporarily unavailable")]
    ServiceUnavailable,

    // =========================================================================
    // Internal Errors
    // =========================================================================
    /// Internal system error
    #[error("internal error: {message}")]
    Internal { message: String },

    /// Unexpected state
    #[error("unexpected state: {description}")]
    UnexpectedState { description: String },

    /// Not implemented
    #[error("not implemented: {feature}")]
    NotImplemented { feature: String },

    // =========================================================================
    // External Error Wrappers (transparent)
    // =========================================================================
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("serialization error: {0}")]
    Serialization(String),
}

impl MylmError {
    /// Check if error is retryable (transient)
    pub fn is_retryable(&self) -> bool {
        match self {
            // Network/connection errors - retryable
            Self::ConnectionFailed { .. } => true,
            Self::Timeout { .. } => true,
            Self::StreamDisconnected { .. } => true,
            Self::ServiceUnavailable => true,
            Self::ModelCapacity { .. } => true,
            Self::RateLimitExceeded { .. } => true,
            Self::ToolTimeout { .. } => true,

            // Provider errors - depends on status
            Self::ProviderError { status, .. } => matches!(status, 429 | 500 | 502 | 503 | 504),

            // Rollout errors - retryable (might be temporary IO issue)
            Self::RolloutWriteFailed { .. } => true,

            // IO errors - some are retryable
            Self::Io(io_err) => matches!(
                io_err.kind(),
                std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
            ),

            // Never retry these
            Self::Unauthorized { .. }
            | Self::InvalidCredentials { .. }
            | Self::TokenExpired
            | Self::QuotaExceeded { .. }
            | Self::ContextWindowExceeded { .. }
            | Self::ApprovalDenied { .. }
            | Self::CommandForbidden { .. }
            | Self::SandboxDenied { .. }
            | Self::ToolNotFound { .. }
            | Self::InvalidToolArguments { .. }
            | Self::InvalidConfig { .. }
            | Self::InvalidInput { .. }
            | Self::InvalidPath { .. }
            | Self::FileNotFound { .. }
            | Self::ProfileNotFound { .. }
            | Self::MissingConfig { .. }
            | Self::ThreadNotFound { .. }
            | Self::ThreadLimitReached { .. }
            | Self::SessionNotFound { .. }
            | Self::SessionExpired { .. }
            | Self::ApprovalQueueFull { .. }
            | Self::ApprovalTimeout { .. }
            | Self::ToolExecutionFailed { .. }
            | Self::RolloutCorrupted { .. }
            | Self::RolloutReadFailed { .. }
            | Self::Internal { .. }
            | Self::UnexpectedState { .. }
            | Self::NotImplemented { .. }
            | Self::Json { .. }
            | Self::Http { .. }
            | Self::Serialization { .. } => false,
        }
    }

    /// Get suggested retry delay for retryable errors
    pub fn retry_delay(&self) -> Option<Duration> {
        match self {
            Self::RateLimitExceeded { .. } => Some(Duration::from_secs(5)),
            Self::ModelCapacity { retry_after, .. } => *retry_after,
            Self::Timeout { .. } => Some(Duration::from_secs(1)),
            Self::ConnectionFailed { .. } => Some(Duration::from_secs(2)),
            Self::ServiceUnavailable => Some(Duration::from_secs(10)),
            _ => None,
        }
    }

    /// Check if error requires user action
    pub fn requires_user_action(&self) -> bool {
        matches!(
            self,
            Self::Unauthorized { .. }
                | Self::InvalidCredentials { .. }
                | Self::TokenExpired
                | Self::QuotaExceeded { .. }
                | Self::ApprovalDenied { .. }
                | Self::ApprovalTimeout { .. }
                | Self::InvalidConfig { .. }
                | Self::MissingConfig { .. }
        )
    }

    /// Get a user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            Self::Unauthorized { .. } => {
                "Authentication failed. Please check your API key.".to_string()
            }
            Self::QuotaExceeded { .. } => {
                "Usage limit reached. Please upgrade your plan or wait for quota reset.".to_string()
            }
            Self::ContextWindowExceeded { .. } => {
                "The conversation is too long. Please start a new session.".to_string()
            }
            Self::ApprovalDenied { action } => {
                format!("Action '{}' was not approved.", action)
            }
            Self::ToolExecutionFailed { tool_name, .. } => {
                format!("Failed to execute tool '{}'.", tool_name)
            }
            _ => self.to_string(),
        }
    }
}

/// Convert from anyhow::Error to MylmError
impl From<anyhow::Error> for MylmError {
    fn from(err: anyhow::Error) -> Self {
        // Try to downcast to specific error types
        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
            return Self::Io(std::io::Error::new(io_err.kind(), io_err.to_string()));
        }

        // Generic internal error
        Self::Internal {
            message: err.to_string(),
        }
    }
}

/// Convert from serde_json::Error to MylmError
impl From<serde_json::Error> for MylmError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err.to_string())
    }
}

/// Convert HTTP errors to MylmError
/// Note: reqwest feature can be added later for automatic conversion
impl From<String> for MylmError {
    fn from(err: String) -> Self {
        Self::Http(err)
    }
}

/// Result type alias using MylmError
pub type Result<T> = std::result::Result<T, MylmError>;

/// Extension trait for converting Option to Result with MylmError
pub trait OptionExt<T> {
    fn ok_or_not_found(self, path: impl Into<PathBuf>) -> Result<T>;
    fn ok_or_missing(self, key: impl Into<String>) -> Result<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_not_found(self, path: impl Into<PathBuf>) -> Result<T> {
        self.ok_or_else(|| MylmError::FileNotFound {
            path: path.into(),
        })
    }

    fn ok_or_missing(self, key: impl Into<String>) -> Result<T> {
        self.ok_or_else(|| MylmError::MissingConfig { key: key.into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retryable_errors() {
        assert!(MylmError::Timeout {
            duration: Duration::from_secs(30)
        }
        .is_retryable());

        assert!(MylmError::ConnectionFailed {
            message: "timeout".to_string()
        }
        .is_retryable());

        assert!(!MylmError::Unauthorized {
            message: "bad token".to_string()
        }
        .is_retryable());

        assert!(!MylmError::ContextWindowExceeded {
            max_tokens: 8192,
            used_tokens: 9000
        }
        .is_retryable());
    }

    #[test]
    fn test_user_messages() {
        let err = MylmError::QuotaExceeded {
            plan_type: "pro".to_string(),
        };
        assert!(err.user_message().contains("Usage limit"));

        let err = MylmError::ContextWindowExceeded {
            max_tokens: 100,
            used_tokens: 200,
        };
        assert!(err.user_message().contains("conversation is too long"));
    }

    #[test]
    fn test_option_ext() {
        let opt: Option<i32> = None;
        let result = opt.ok_or_not_found("/tmp/test");
        assert!(matches!(result, Err(MylmError::FileNotFound { .. })));

        let opt: Option<i32> = None;
        let result = opt.ok_or_missing("api_key");
        assert!(matches!(result, Err(MylmError::MissingConfig { .. })));
    }
}
