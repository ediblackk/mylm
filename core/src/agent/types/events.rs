//! Events that flow into the Kernel
//!
//! These are the inputs to the pure state machine.
//! All events are serializable for transport and replay.

use serde::{Deserialize, Serialize};
use super::ids::IntentId;

/// Worker identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub u64);

impl WorkerId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Events that the kernel processes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KernelEvent {
    /// User sent a message
    UserMessage {
        content: String,
    },

    /// A tool was executed and returned a result
    ToolCompleted {
        /// The intent ID that requested this tool
        intent_id: IntentId,
        /// The tool name
        tool: String,
        /// The result (success or error)
        result: ToolResult,
    },

    /// An LLM request completed
    LLMCompleted {
        /// The intent ID that requested this
        intent_id: IntentId,
        /// The LLM response
        response: LLMResponse,
    },

    /// User approved or denied a tool execution
    ApprovalGiven {
        /// The intent ID that requested approval
        intent_id: IntentId,
        /// The approval outcome
        outcome: ApprovalOutcome,
    },

    /// A worker completed its task
    WorkerCompleted {
        /// The worker ID
        worker_id: WorkerId,
        /// The result of the worker's work
        result: Result<String, WorkerError>,
    },

    /// A worker failed or stalled
    WorkerFailed {
        /// The worker ID
        worker_id: WorkerId,
        /// The error message
        error: String,
        /// Whether this is a stall (recoverable) or fatal error
        is_stall: bool,
    },

    /// User requested interruption
    Interrupt,

    /// System tick (for timeouts, periodic checks)
    Tick {
        /// Logical time of the tick
        time: u64,
    },

    /// Session-level event
    Session {
        action: SessionAction,
    },

    /// Runtime error occurred (e.g., LLM request failed)
    RuntimeError {
        /// The intent ID that caused the error
        intent_id: IntentId,
        /// The error message
        error: String,
    },
}

/// Result of a tool execution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolResult {
    Success {
        /// Output data from the tool
        output: String,
        /// Optional structured data
        structured: Option<serde_json::Value>,
    },
    Error {
        /// Error message
        message: String,
        /// Error code if applicable
        code: Option<String>,
        /// Whether this error is retryable
        retryable: bool,
    },
    Cancelled,
}

/// Approval decision outcome
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalOutcome {
    Granted,
    Denied { reason: Option<String> },
}

/// LLM response data
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LLMResponse {
    /// The text content
    pub content: String,
    /// Token usage information
    pub usage: TokenUsage,
    /// The model used
    pub model: String,
    /// Provider information
    pub provider: String,
    /// Finish reason
    pub finish_reason: FinishReason,
    /// Optional structured output
    pub structured: Option<serde_json::Value>,
}

/// Token usage from an LLM call
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl TokenUsage {
    pub fn new(prompt: u32, completion: u32) -> Self {
        Self {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
        }
    }

    pub fn add(&mut self, other: &TokenUsage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.total_tokens += other.total_tokens;
    }
}

/// Why the LLM finished generating
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    Length,
    TokenLimit,
    ContentFilter,
    ToolCall,
    Other(String),
}

/// Error from a worker
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerError {
    pub message: String,
    pub code: Option<String>,
}

/// Session-level actions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionAction {
    Start,
    Resume { session_id: String },
    Pause,
    Save,
    Load { session_id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage() {
        let usage = TokenUsage::new(100, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::Success {
            output: "file contents".to_string(),
            structured: None,
        };
        assert!(matches!(result, ToolResult::Success { .. }));
    }
}
