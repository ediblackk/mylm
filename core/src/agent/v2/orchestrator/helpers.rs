//! Orchestrator Helpers
//!
//! Helper functions for job polling, tool execution, and error handling.

use crate::agent::event_bus::{CoreEvent, EventBus};
use crate::agent::traits::TerminalExecutor;
use crate::agent::v2::jobs::{JobRegistry, JobStatus};
use crate::agent::tool::Tool;
use crate::agent::ToolRegistry;
use crate::llm::TokenUsage;
use serde_json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::time::Duration;

/// Trait to unify access to common agent fields between V1 and V2
pub(crate) trait AgentLike: Send + Sync {
    fn memory_store(&self) -> Option<&crate::memory::store::VectorStore>;
    fn session_id(&self) -> &str;
}

impl AgentLike for crate::agent::Agent {
    fn memory_store(&self) -> Option<&crate::memory::store::VectorStore> {
        self.memory_store.as_deref()
    }
    fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl AgentLike for crate::agent::v2::AgentV2 {
    fn memory_store(&self) -> Option<&crate::memory::store::VectorStore> {
        self.memory_store.as_deref()
    }
    fn session_id(&self) -> &str {
        &self.session_id
    }
}

/// Helper: Poll jobs and accumulate observations
pub fn poll_jobs(
    interrupt_flag: &Arc<AtomicBool>,
    _config: &super::types::OrchestratorConfig,
    job_registry: &JobRegistry,
    _job_id: &Option<String>,
    event_bus: &Arc<EventBus>,
) -> Result<(Vec<String>, bool), String> {
    // Check for interruption
    if interrupt_flag.load(Ordering::SeqCst) {
        event_bus.publish(CoreEvent::AgentResponse {
            content: "⛔ Task interrupted by user.".to_string(),
            usage: TokenUsage::default(),
        });
        return Err("interrupted".to_string());
    }

    let completed_jobs = job_registry.poll_updates();
    // Only log if we found completed jobs to reduce noise
    if !completed_jobs.is_empty() {
        crate::info_log!("poll_jobs: found {} completed jobs", completed_jobs.len());
    }
    
    let mut has_new_observations = false;
    let mut observations = Vec::new();

    if !completed_jobs.is_empty() {
        for job in completed_jobs {
            match job.status {
                JobStatus::Completed => {
                    // Strip token usage from worker result to avoid counting worker tokens in main agent context
                    let (result_str, scratchpad_str) = if let Some(result) = job.result.as_ref() {
                        // Create a clean version without the usage field
                        let mut clean_result = serde_json::Map::new();
                        if let Some(worker_id) = result.get("worker_id") {
                            clean_result.insert("worker_id".to_string(), worker_id.clone());
                        }
                        if let Some(output) = result.get("result") {
                            clean_result.insert("result".to_string(), output.clone());
                        }
                        let scratch = result.get("scratchpad")
                            .and_then(|s| s.as_str())
                            .unwrap_or("");
                        // Exclude "usage" field to prevent token counting blowup
                        (serde_json::Value::Object(clean_result).to_string(), scratch.to_string())
                    } else {
                        ("Job completed successfully".to_string(), String::new())
                    };
                    
                    let scratchpad_section = if scratchpad_str.is_empty() {
                        String::new()
                    } else {
                        format!("\n\nWorker Coordination Log:\n```\n{}\n```", scratchpad_str)
                    };
                    
                    observations.push(format!(
                        "✅ WORKER TASK COMPLETED: '{}' finished successfully.\nResult: {}\n\nThe delegated task is now COMPLETE. Report the result to the user and ask if they need anything else.{}",
                        job.description, result_str, scratchpad_section
                    ));
                    has_new_observations = true;
                }
                JobStatus::Failed => {
                    let error_msg = job
                        .error
                        .as_ref()
                        .map(|e| e.as_str())
                        .unwrap_or("Unknown error");
                    observations.push(format!(
                        "❌ WORKER TASK FAILED: '{}' failed with error: {}\n\nThe delegated task failed. Report the failure to the user.",
                        job.description, error_msg
                    ));
                    has_new_observations = true;
                }
                JobStatus::Stalled => {
                    let stall_msg = format!(
                        "⚠️ WORKER TASK STALLED: '{}' STALLED after exceeding action budget without returning a final answer.\n\nThe worker did not complete. You can ask the user if they want to retry or modify the task.",
                        job.description
                    );
                    observations.push(stall_msg);
                    has_new_observations = true;
                }
                _ => {}
            }
        }
    }

    Ok((observations, has_new_observations))
}

/// Helper: Execute terminal tool
pub async fn execute_terminal_tool(
    tool: &str,
    args: &str,
    auto_approve: bool,
    event_bus: Arc<EventBus>,
    terminal_delegate: &Option<Arc<dyn TerminalExecutor>>,
    approval_rx: &mut Option<Receiver<bool>>,
) -> Result<String, String> {
    if let Some(ref delegate) = terminal_delegate {
        // Parse command from args
        let command = if tool == "execute_command" {
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
            format!("{} {}", tool, args)
        };

        // Check auto-approve
        if !auto_approve {
            // Publish event to UI to show approval dialog
            event_bus.publish(CoreEvent::ToolAwaitingApproval {
                tool: tool.to_string(),
                args: command.clone(),
                approval_id: format!("{}-{}", tool, std::time::SystemTime::now().elapsed().unwrap_or_default().as_millis()),
            });
            
            // Wait for approval from UI via the provided channel
            if let Some(ref mut rx) = approval_rx {
                crate::info_log!("Tool '{}' waiting for user approval...", tool);
                match rx.recv().await {
                    Some(true) => {
                        // Approved - continue to execute
                        crate::info_log!("Tool '{}' approved by user, executing...", tool);
                    }
                    Some(false) => {
                        // Rejected by user
                        return Err(format!("❌ Tool '{}' was rejected by user", tool));
                    }
                    None => {
                        // Channel closed - treat as rejected
                        return Err(format!("❌ Approval channel closed for tool '{}'", tool));
                    }
                }
            } else {
                // No approval channel available but auto-approve is off
                return Err(format!("❌ Approval required for tool '{}' but no approval channel available (auto-approve is OFF)", tool));
            }
        }
        
        match delegate.execute_command(command, Some(Duration::from_secs(30))).await {
            Ok(output) => Ok(output),
            Err(e) => Err(format!("Error executing command: {}", e)),
        }
    } else {
        Err("Error: Terminal delegate not available".to_string())
    }
}

/// Helper: Execute non-terminal tool
pub async fn execute_tool(
    tool: &str,
    args: &str,
    tool_registry: Option<&ToolRegistry>,
    tools: Option<&HashMap<String, Arc<dyn Tool>>>,
    _event_bus: &Arc<EventBus>,
    _job_id: &Option<String>,
    _job_registry: &JobRegistry,
) -> Result<String, String> {
    if let Some(registry) = tool_registry {
        // V1 path
        match registry.execute_tool(tool, args).await {
            Ok(output) => Ok(output.as_string()),
            Err(e) => Err(e.to_string()),
        }
    } else if let Some(tools_map) = tools {
        // V2 path
        if let Some(tool_impl) = tools_map.get(tool) {
            match tool_impl.call(args).await {
                Ok(output) => Ok(output.as_string()),
                Err(e) => Err(e.to_string()),
            }
        } else {
            Err(format!("Error: Tool '{}' not found.", tool))
        }
    } else {
        Err("No tool execution mechanism available".to_string())
    }
}

/// Helper: Record to memory
pub async fn record_to_memory(
    agent: &impl AgentLike,
    tool: &str,
    _args: &str,
    observation: &str,
    _usage: &TokenUsage,
) -> Result<(), String> {
    if let Some(store) = agent.memory_store() {
        let _ = store.record_command(tool, observation, 0, Some(agent.session_id().to_string())).await;
    }
    Ok(())
}

/// Helper: Handle error
pub fn handle_error(event_bus: &Arc<EventBus>, error: &str) -> Result<(), String> {
    event_bus.publish(CoreEvent::StatusUpdate { message: format!("Error: {}", error) });
    Err(error.to_string())
}

/// Format job status for injection into agent context - TODO 
#[allow(dead_code)]
pub fn format_job_status(job_registry: &JobRegistry) -> String {
    let active_jobs = job_registry.list_active_jobs();
    let all_jobs = job_registry.list_all_jobs();
    let completed_jobs: Vec<_> = all_jobs.iter()
        .filter(|j| matches!(j.status, JobStatus::Completed | JobStatus::Failed))
        .cloned()
        .collect();
    
    if active_jobs.is_empty() && completed_jobs.is_empty() {
        return String::new();
    }
    
    let mut status = String::new();
    
    if !active_jobs.is_empty() {
        status.push_str(&format!("Active Jobs ({}):\n", active_jobs.len()));
        for job in active_jobs.iter().take(5) {
            status.push_str(&format!("  • {}: {}\n", job.id, job.description));
        }
        if active_jobs.len() > 5 {
            status.push_str(&format!("  ... and {} more\n", active_jobs.len() - 5));
        }
    }
    
    if !completed_jobs.is_empty() {
        if !status.is_empty() {
            status.push('\n');
        }
        status.push_str(&format!("Recently Completed ({}):\n", completed_jobs.len().min(3)));
        for job in completed_jobs.iter().rev().take(3) {
            let status_icon = match job.status {
                JobStatus::Completed => "✓",
                JobStatus::Failed => "✗",
                _ => "?",
            };
            status.push_str(&format!("  {} {}: {}\n", status_icon, job.id, job.description));
        }
    }
    
    status
}
