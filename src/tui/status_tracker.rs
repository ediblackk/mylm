//! Status Tracker - Event Stream Aggregator for UI Status
//!
//! This module provides a centralized status tracker that infers the current
//! application state from output events, rather than requiring tools/actions
//! to explicitly declare their status.
//!
//! ## Design Philosophy
//!
//! The StatusTracker acts as an event stream aggregator. It listens to
//! OutputEvents and maintains derived state about what the system is doing.
//! This approach is:
//!
//! - **Decoupled**: Tools don't need to know about UI status
//! - **Simple**: Single source of truth for UI status
//! - **Extensible**: Easy to add new status patterns
//!
//! ## Alternative: State Polling
//!
//! If further detailed status is required (e.g., per-tool progress, elapsed time,
//! retry counts), state polling (querying the session's in_flight intents and
//! intent_results) can show more detailed progress. This would involve:
//!
//! - Periodically polling the session for current intents
//! - Tracking tool execution duration
//! - Showing granular progress for long-running operations
//!
//! The event aggregator approach is preferred for simplicity, but state polling
//! can be added as a complementary mechanism for advanced use cases.

use std::time::{Duration, Instant};

/// Current status information for the UI
#[derive(Debug, Clone)]
pub enum StatusInfo {
    /// System is idle
    Idle,
    /// A tool is being executed
    Executing { tool: String, args: String },
    /// Agent is thinking/processing
    Thinking,
    /// An error occurred
    Error { message: String },
    /// Waiting for user approval
    AwaitingApproval { tool: String, #[allow(dead_code)] args: String },
}

#[allow(dead_code)]
impl StatusInfo {
    /// Get a human-readable status message
    pub fn message(&self) -> String {
        match self {
            StatusInfo::Idle => "Ready".to_string(),
            StatusInfo::Executing { tool, args } => {
                let args_preview = if args.len() > 40 {
                    format!("{}...", &args[..40])
                } else {
                    args.clone()
                };
                format!("Executing: {} {}", tool, args_preview)
            }
            StatusInfo::Thinking => "Thinking...".to_string(),
            StatusInfo::Error { message } => {
                let msg_preview = if message.len() > 50 {
                    format!("{}...", &message[..50])
                } else {
                    message.clone()
                };
                format!("Error: {}", msg_preview)
            }
            StatusInfo::AwaitingApproval { tool, args } => {
                let args_preview = if args.len() > 30 {
                    format!("{}...", &args[..30])
                } else {
                    args.clone()
                };
                format!("Approve: {} {}? (y/n)", tool, args_preview)
            }
        }
    }

    /// Check if this is an error status
    pub fn is_error(&self) -> bool {
        matches!(self, StatusInfo::Error { .. })
    }

    /// Check if this is an executing status
    pub fn is_executing(&self) -> bool {
        matches!(self, StatusInfo::Executing { .. })
    }
}

/// Tracks status by aggregating output events
#[derive(Debug)]
pub struct StatusTracker {
    current_status: StatusInfo,
    last_activity: Instant,
    /// Timestamp when current tool started executing
    tool_start_time: Option<Instant>,
    /// Recent error message (cleared on new activity)
    last_error: Option<String>,
}

#[allow(dead_code)]
impl StatusTracker {
    /// Create a new status tracker
    pub fn new() -> Self {
        Self {
            current_status: StatusInfo::Idle,
            last_activity: Instant::now(),
            tool_start_time: None,
            last_error: None,
        }
    }

    /// Process an output event and update status
    pub fn on_event(&mut self, event: &mylm_core::agent::contract::session::OutputEvent) {
        use mylm_core::agent::contract::session::OutputEvent;

        match event {
            OutputEvent::Thinking { .. } => {
                self.current_status = StatusInfo::Thinking;
                self.last_activity = Instant::now();
                self.last_error = None;
            }

            OutputEvent::ToolExecuting { tool, args, .. } => {
                self.current_status = StatusInfo::Executing {
                    tool: tool.clone(),
                    args: args.clone(),
                };
                self.tool_start_time = Some(Instant::now());
                self.last_activity = Instant::now();
                self.last_error = None;
            }

            OutputEvent::ToolCompleted { result, .. } => {
                // Check if the result indicates an error
                // Tool results that start with error markers should show error status
                let is_error = result.starts_with("❌ Error:")
                    || result.starts_with("Error:")
                    || result.contains("rate limited")
                    || result.contains("API key not configured")
                    || result.contains("timed out");

                if is_error {
                    // Extract the error message from the result
                    let error_msg = if result.starts_with("❌ Error:") {
                        result.trim_start_matches("❌ Error:").trim().to_string()
                    } else if result.starts_with("Error:") {
                        result.trim_start_matches("Error:").trim().to_string()
                    } else {
                        result.clone()
                    };

                    self.current_status = StatusInfo::Error {
                        message: error_msg.clone(),
                    };
                    self.last_error = Some(error_msg);
                } else {
                    self.current_status = StatusInfo::Idle;
                }

                self.tool_start_time = None;
                self.last_activity = Instant::now();
            }

            OutputEvent::Error { message } => {
                self.current_status = StatusInfo::Error {
                    message: message.clone(),
                };
                self.last_error = Some(message.clone());
                self.tool_start_time = None;
                self.last_activity = Instant::now();
            }

            OutputEvent::ResponseChunk { .. } => {
                // During streaming response, we're effectively "thinking/processing"
                if !matches!(self.current_status, StatusInfo::Executing { .. }) {
                    self.current_status = StatusInfo::Thinking;
                }
                self.last_activity = Instant::now();
            }

            OutputEvent::ResponseComplete => {
                // Only clear status if we're not showing an error
                if !matches!(self.current_status, StatusInfo::Error { .. }) {
                    self.current_status = StatusInfo::Idle;
                }
                self.tool_start_time = None;
                self.last_activity = Instant::now();
            }

            OutputEvent::Halted { reason } => {
                // Halt is typically an error condition
                self.current_status = StatusInfo::Error {
                    message: reason.clone(),
                };
                self.last_error = Some(reason.clone());
                self.tool_start_time = None;
                self.last_activity = Instant::now();
            }

            OutputEvent::ApprovalRequested { tool, args, .. } => {
                // Waiting for user approval
                self.current_status = StatusInfo::AwaitingApproval {
                    tool: tool.clone(),
                    args: args.clone(),
                };
                self.last_error = None;
                self.last_activity = Instant::now();
            }

            OutputEvent::WorkerSpawned { objective, .. } => {
                // Worker spawned - treat as executing
                self.current_status = StatusInfo::Executing {
                    tool: "worker".to_string(),
                    args: objective.clone(),
                };
                self.last_error = None;
                self.last_activity = Instant::now();
            }

            OutputEvent::WorkerCompleted { .. } => {
                // Worker completed - go back to idle if not in error state
                if !matches!(self.current_status, StatusInfo::Error { .. }) {
                    self.current_status = StatusInfo::Idle;
                }
                self.last_activity = Instant::now();
            }

            OutputEvent::Status { message: _message } => {
                // Generic status message - update activity timestamp
                // but don't change current status
                self.last_activity = Instant::now();
            }
            
            OutputEvent::ContextPruned { summary, message_count, tokens_saved, .. } => {
                // Context was pruned - this is informational, not an error
                // Just update activity timestamp
                self.last_activity = Instant::now();
                mylm_core::info_log!(
                    "[STATUS_TRACKER] Context pruned: {} messages, ~{} tokens saved. {}",
                    message_count, tokens_saved, summary
                );
            }
        }
    }

    /// Get the current status
    pub fn current(&self) -> &StatusInfo {
        &self.current_status
    }

    /// Get the current status message
    pub fn message(&self) -> String {
        self.current_status.message()
    }

    /// Get elapsed time since current tool started
    pub fn tool_elapsed(&self) -> Option<Duration> {
        self.tool_start_time.map(|start| start.elapsed())
    }

    /// Get the last error message if any
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Get elapsed time since last activity
    pub fn idle_duration(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// Clear error status (call when user acknowledges error)
    pub fn clear_error(&mut self) {
        if matches!(self.current_status, StatusInfo::Error { .. }) {
            self.current_status = StatusInfo::Idle;
        }
    }

    /// Force set a specific status (for manual overrides)
    pub fn set_status(&mut self, status: StatusInfo) {
        self.current_status = status;
        self.last_activity = Instant::now();
    }
}

impl Default for StatusTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mylm_core::agent::contract::session::OutputEvent;
    use mylm_core::agent::types::ids::IntentId;

    #[test]
    fn test_tool_execution_flow() {
        let mut tracker = StatusTracker::new();

        // Initial state is idle
        assert!(matches!(tracker.current(), StatusInfo::Idle));

        // Tool starts executing
        tracker.on_event(&OutputEvent::ToolExecuting {
            intent_id: IntentId::new(1),
            tool: "web_search".to_string(),
            args: "test query".to_string(),
        });
        assert!(tracker.current().is_executing());
        assert_eq!(
            tracker.message(),
            "Executing: web_search test query"
        );

        // Tool completes successfully
        tracker.on_event(&OutputEvent::ToolCompleted {
            intent_id: IntentId::new(1),
            result: "Found results".to_string(),
        });
        assert!(matches!(tracker.current(), StatusInfo::Idle));
    }

    #[test]
    fn test_tool_error_detection() {
        let mut tracker = StatusTracker::new();

        // Tool starts
        tracker.on_event(&OutputEvent::ToolExecuting {
            intent_id: IntentId::new(1),
            tool: "web_search".to_string(),
            args: "test".to_string(),
        });

        // Tool completes with error
        tracker.on_event(&OutputEvent::ToolCompleted {
            intent_id: IntentId::new(1),
            result: "❌ Error: Rate limited (429)".to_string(),
        });

        assert!(tracker.current().is_error());
        assert!(tracker.message().contains("Rate limited"));
    }

    #[test]
    fn test_error_event() {
        let mut tracker = StatusTracker::new();

        tracker.on_event(&OutputEvent::Error {
            message: "Something went wrong".to_string(),
        });

        assert!(tracker.current().is_error());
        assert_eq!(tracker.message(), "Error: Something went wrong");
    }
}
