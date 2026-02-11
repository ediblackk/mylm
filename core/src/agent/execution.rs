//! Tool execution logic for the legacy Agent
//!
//! This module handles the execution of tools and processing of their outputs
//! for the legacy Agent's run loop.

use crate::agent::tool::ToolKind;
use crate::agent::tool_registry::ToolRegistry;
use crate::agent::event_bus::{EventBus, CoreEvent};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

/// Process tool arguments from JSON if necessary
/// 
/// Some tools receive their arguments wrapped in a JSON object like `{"args": "..."}`
/// This function extracts the actual arguments.
pub fn process_tool_args(args: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        v.get("args")
            .and_then(|a| a.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| args.to_string())
    } else {
        args.to_string()
    }
}

/// Execute a tool and return the observation
pub async fn execute_tool_with_registry(
    tool_registry: &ToolRegistry,
    tool: &str,
    args: &str,
) -> Result<String, String> {
    let processed_args = process_tool_args(args);
    
    match tool_registry.execute_tool(tool, &processed_args).await {
        Ok(output) => Ok(output.as_string()),
        Err(e) => Err(format!(
            "Tool Error: {}. Analyze the failure and try a different command or approach if possible.",
            e
        )),
    }
}

/// Handle the approval flow for a tool execution
///
/// Returns `Ok(true)` if approved, `Ok(false)` if denied, `Err` if channel closed
pub async fn handle_tool_approval(
    tool: &str,
    args: &str,
    auto_approve: bool,
    event_bus: &Arc<EventBus>,
    approval_rx: &mut Option<Receiver<bool>>,
) -> Result<bool, String> {
    if auto_approve {
        return Ok(true);
    }

    // Provide a human-readable suggestion of what would run
    let suggestion = if tool == "execute_command" {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
            v.get("command")
                .and_then(|c| c.as_str())
                .or_else(|| v.get("args").and_then(|c| c.as_str()))
                .unwrap_or(args)
                .to_string()
        } else {
            args.to_string()
        }
    } else {
        // For non-shell tools, show tool+args (keeps UI event semantics stable)
        format!("{} {}", tool, args)
    };
    
    let _ = event_bus.publish(CoreEvent::SuggestCommand { command: suggestion });

    if let Some(rx) = approval_rx {
        // Wait for approval (one tool execution == one approval)
        let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "Waiting for approval...".to_string() });
        
        match rx.recv().await {
            Some(true) => {
                let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "Approved.".to_string() });
                Ok(true)
            }
            Some(false) => {
                let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "Denied.".to_string() });
                Ok(false)
            }
            None => Err("Approval channel closed.".to_string()),
        }
    } else {
        // Legacy/headless behavior: halt and return control to the caller
        Err(format!(
            "Approval required to run tool '{}' but no approval channel is available (AUTO-APPROVE is OFF).",
            tool
        ))
    }
}

/// Send tool observation to the appropriate output channel
pub fn emit_tool_observation(
    tool: &str,
    observation: &str,
    kind: ToolKind,
    event_bus: &Arc<EventBus>,
) {
    if kind == ToolKind::Internal {
        let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", observation.trim());
        let _ = event_bus.publish(CoreEvent::InternalObservation { data: obs_log.into_bytes() });
    }
    
    let _ = event_bus.publish(CoreEvent::StatusUpdate { message: format!(
        "Tool '{}' completed",
        tool
    ) });
}

/// Check for interruption signal
pub fn check_interrupt(interrupt_flag: &Arc<AtomicBool>) -> bool {
    interrupt_flag.load(Ordering::SeqCst)
}

/// Build a nudge message for when the agent provides a non-action response
/// but we expect it to continue working
pub fn build_continue_nudge() -> String {
    "Please continue your task or provide a Final Answer if you are done.".to_string()
}

/// Build a format error nudge for malformed actions
pub fn build_format_nudge(error: &str) -> String {
    format!(
        "{}\n\n\
        IMPORTANT: You must follow the ReAct format exactly:\n\
        Thought: <your reasoning>\n\
        Action: <tool name>\n\
        Action Input: <tool arguments>\n\n\
        Do not include any other text after Action Input.",
        error
    )
}

/// Determine if a message should terminate the agent loop
/// 
/// Returns true if the message appears to be a final response to the user
pub fn is_final_response(msg: &str) -> bool {
    // Contains explicit final answer markers
    if msg.contains("Final Answer:") || msg.contains("\"f\":") || msg.contains("\"final_answer\":") {
        return true;
    }
    
    // Common indicators that the model is talking to the user
    if msg.trim().ends_with('?')
        || msg.contains("Please")
        || msg.contains("Would you")
        || msg.contains("Acknowledged")
        || msg.contains("I've memorized")
        || msg.contains("Absolutely")
    {
        return true;
    }
    
    // If it's a non-empty message and no tool was called, and it's not a tiny nudge,
    // we assume it's a response to the user.
    if msg.len() > 30 {
        return true;
    }
    
    false
}
