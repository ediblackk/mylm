//! Observations returned from the Runtime
//!
//! Observations are the results of executing intents.
//! They flow back to the kernel as inputs for the next step.

use serde::{Deserialize, Serialize};
use super::ids::IntentId;
use super::events::{WorkerId, ToolResult, LLMResponse, WorkerError, TokenUsage, ApprovalOutcome};

/// Observation - result of intent execution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Observation {
    /// A tool was executed
    ToolCompleted {
        intent_id: IntentId,
        result: ToolResult,
        execution_time_ms: u64,
    },

    /// An LLM request completed
    LLMCompleted {
        intent_id: IntentId,
        response: LLMResponse,
    },

    /// User approval was given/denied
    ApprovalCompleted {
        intent_id: IntentId,
        outcome: ApprovalOutcome,
    },

    /// A worker was spawned
    WorkerSpawned {
        intent_id: IntentId,
        worker_id: WorkerId,
    },

    /// A worker completed its task
    WorkerCompleted {
        worker_id: WorkerId,
        result: Result<String, WorkerError>,
        usage: TokenUsage,
    },

    /// A response was emitted
    ResponseEmitted {
        intent_id: IntentId,
        content: String,
        is_partial: bool,
    },

    /// Execution was halted
    Halted {
        intent_id: IntentId,
        reason: HaltReason,
    },

    /// An error occurred during execution
    RuntimeError {
        intent_id: IntentId,
        error: ExecutionError,
    },

    /// Intent timed out
    Timeout {
        intent_id: IntentId,
        timeout_secs: u64,
    },

    /// Intent was cancelled
    Cancelled {
        intent_id: IntentId,
    },
}

impl Observation {
    /// Set the intent_id for this observation
    pub fn set_intent_id(&mut self, id: IntentId) {
        match self {
            Observation::ToolCompleted { intent_id, .. } => *intent_id = id,
            Observation::LLMCompleted { intent_id, .. } => *intent_id = id,
            Observation::ApprovalCompleted { intent_id, .. } => *intent_id = id,
            Observation::WorkerSpawned { intent_id, .. } => *intent_id = id,
            Observation::WorkerCompleted { .. } => {}
            Observation::ResponseEmitted { intent_id, .. } => *intent_id = id,
            Observation::Halted { intent_id, .. } => *intent_id = id,
            Observation::RuntimeError { intent_id, .. } => *intent_id = id,
            Observation::Timeout { intent_id, .. } => *intent_id = id,
            Observation::Cancelled { intent_id, .. } => *intent_id = id,
        }
    }

    /// Convert observation to kernel event
    pub fn into_event(self) -> super::events::KernelEvent {
        use super::events::KernelEvent;
        match self {
            Observation::ToolCompleted { intent_id, result, .. } => {
                KernelEvent::ToolCompleted {
                    intent_id,
                    tool: "unknown".to_string(), // Tool name not stored in observation
                    result,
                }
            }
            Observation::LLMCompleted { intent_id, response } => {
                KernelEvent::LLMCompleted { intent_id, response }
            }
            Observation::ApprovalCompleted { intent_id, outcome } => {
                KernelEvent::ApprovalGiven { intent_id, outcome }
            }
            Observation::WorkerSpawned { .. } => {
                // WorkerSpawned doesn't map directly to KernelEvent
                KernelEvent::Tick { time: 0 }
            }
            Observation::WorkerCompleted { worker_id, result, .. } => {
                KernelEvent::WorkerCompleted { worker_id, result }
            }
            Observation::ResponseEmitted { .. } => {
                // ResponseEmitted doesn't map directly to KernelEvent
                KernelEvent::Tick { time: 0 }
            }
            Observation::Halted { .. } => {
                KernelEvent::Interrupt
            }
            Observation::RuntimeError { intent_id, error } => {
                KernelEvent::RuntimeError { intent_id, error: error.message }
            }
            Observation::Timeout { .. } => {
                KernelEvent::Tick { time: 0 }
            }
            Observation::Cancelled { .. } => {
                KernelEvent::Tick { time: 0 }
            }
        }
    }
}

/// Reason for halting execution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HaltReason {
    Completed,
    UserRequest,
    StepLimitReached { max_steps: usize },
    Error(String),
    Interrupted,
}

/// Execution error details
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionError {
    pub message: String,
    pub code: Option<String>,
    pub retryable: bool,
}

impl ExecutionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
            retryable: false,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }
}

/// Summary of execution results
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionSummary {
    pub total_intents: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub total_execution_time_ms: u64,
}

impl ExecutionSummary {
    pub fn new(total: usize) -> Self {
        Self {
            total_intents: total,
            ..Default::default()
        }
    }

    pub fn from_observations(observations: &[Observation]) -> Self {
        let mut summary = Self::new(observations.len());
        for obs in observations {
            match obs {
                Observation::ToolCompleted { execution_time_ms, .. } => {
                    summary.record_completed(*execution_time_ms);
                }
                Observation::LLMCompleted { .. } => {
                    summary.record_completed(0);
                }
                Observation::ResponseEmitted { .. } => {
                    summary.record_completed(0);
                }
                Observation::WorkerSpawned { .. } => {
                    summary.record_completed(0);
                }
                Observation::WorkerCompleted { .. } => {
                    summary.record_completed(0);
                }
                Observation::ApprovalCompleted { .. } => {
                    summary.record_completed(0);
                }
                Observation::Halted { .. } => {
                    summary.record_completed(0);
                }
                Observation::RuntimeError { .. } => {
                    summary.record_failed();
                }
                Observation::Timeout { .. } => {
                    summary.record_failed();
                }
                Observation::Cancelled { .. } => {
                    summary.record_cancelled();
                }
            }
        }
        summary
    }

    pub fn record_completed(&mut self, time_ms: u64) {
        self.completed += 1;
        self.total_execution_time_ms += time_ms;
    }

    pub fn record_failed(&mut self) {
        self.failed += 1;
    }

    pub fn record_cancelled(&mut self) {
        self.cancelled += 1;
    }

    pub fn all_completed(&self) -> bool {
        self.completed + self.failed + self.cancelled == self.total_intents
    }

    pub fn avg_execution_time_ms(&self) -> u64 {
        if self.completed == 0 {
            0
        } else {
            self.total_execution_time_ms / self.completed as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_summary() {
        let mut summary = ExecutionSummary::new(10);
        summary.record_completed(100);
        summary.record_completed(200);
        summary.record_failed();

        assert_eq!(summary.completed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.avg_execution_time_ms(), 150);
    }

    #[test]
    fn test_execution_error() {
        let err = ExecutionError::new("tool failed")
            .with_code("TOOL_ERROR")
            .retryable();

        assert_eq!(err.message, "tool failed");
        assert_eq!(err.code, Some("TOOL_ERROR".to_string()));
        assert!(err.retryable);
    }
}
