//! Legacy driver for AgentV2 (backward-compatible run method)
use crate::llm::{chat::{ChatMessage, MessageRole}, TokenUsage};
use crate::agent::tool::ToolKind;
use crate::agent::event_bus::{EventBus, CoreEvent};
use crate::agent::v2::{AgentV2, AgentDecision};
use crate::agent::permissions::matches_pattern;
use crate::config::v2::types::AgentPermissions;
// DISABLED: Scribe redesign pending - use crate::memory::journal::InteractionType;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Legacy run method for backward compatibility.
pub async fn run_legacy(
    agent: &mut AgentV2,
    mut history: Vec<ChatMessage>,
    event_bus: Arc<EventBus>,
    interrupt_flag: Arc<AtomicBool>,
    auto_approve: bool,
    max_driver_loops: usize,
    mut approval_rx: Option<tokio::sync::mpsc::Receiver<bool>>,
) -> Result<(String, TokenUsage), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Memory Context Injection (if enabled)
    if agent.llm_client.config().memory.auto_context {
        if let Some(store) = &agent.memory_store {
            if let Some(last_user_msg) = history.iter().rev().find(|m| m.role == MessageRole::User) {
                let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "Searching memory...".to_string() });
                let memories = store.search_memory(&last_user_msg.content, 5).await.unwrap_or_default();
                if !memories.is_empty() {
                    let context = agent.build_context_from_memories(&memories);
                    if let Some(user_idx) = history.iter().rposition(|m| m.role == MessageRole::User) {
                        history[user_idx].content.push_str("\n\n");
                        history[user_idx].content.push_str(&context);
                    }
                }
            }
        }
    }

    agent.reset(history).await;
    agent.inject_hot_memory(5).await;

    let mut last_observation = None;
    let mut retry_count = 0;
    let max_retries = 3;

    let mut loop_iteration = 0;
    loop {
        loop_iteration += 1;
        if loop_iteration > max_driver_loops {
            return Ok((format!("Error: Driver-level safety limit reached ({} loops). Potential infinite loop detected.", max_driver_loops), agent.total_usage.clone()));
        }

        if interrupt_flag.load(Ordering::SeqCst) {
            return Ok(("Interrupted by user.".to_string(), agent.total_usage.clone()));
        }

        let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "Thinking...".to_string() });

        match agent.step(last_observation.take()).await? {
            AgentDecision::Message(msg, usage) => {
                retry_count = 0;
                let _ = event_bus.publish(CoreEvent::AgentResponse { content: msg.clone(), usage: usage.clone() });

                if agent.has_pending_decision() {
                    continue;
                }

                if msg.contains("Final Answer:") || msg.contains("\"f\":") || msg.contains("\"final_answer\":") {
                    let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "".to_string() });
                    return Ok((msg, usage));
                }

                if msg.trim().ends_with('?')
                    || msg.contains("Please")
                    || msg.contains("Would you")
                    || msg.contains("Acknowledged")
                    || msg.contains("I've memorized")
                    || msg.contains("Absolutely")
                {
                    let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "".to_string() });
                    return Ok((msg, usage));
                }

                if msg.len() > 30 {
                    let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "".to_string() });
                    return Ok((msg, usage));
                }

                last_observation = Some("Please continue your task or provide a Final Answer if you are done.".to_string());
                continue;
            }
            AgentDecision::Action { tool, args, kind } => {
                retry_count = 0;
                let _ = event_bus.publish(CoreEvent::StatusUpdate { message: format!("Tool: '{}'", tool) });

                // Check if command is auto-approved via permissions (for execute_command)
                let permission_auto_approved = if tool == "execute_command" {
                    check_permission_auto_approve(&args, &agent.permissions)
                } else {
                    false
                };

                // Determine if we need explicit approval
                let needs_approval = !auto_approve && !permission_auto_approved;

                if needs_approval {
                    let suggestion = extract_suggestion(&tool, &args);
                    let _ = event_bus.publish(CoreEvent::SuggestCommand { command: suggestion });

                    if let Some(rx) = &mut approval_rx {
                        let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "Waiting for approval...".to_string() });
                        match rx.recv().await {
                            Some(true) => {}
                            Some(false) => {
                                let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "Denied.".to_string() });
                                last_observation = Some(format!("Error: User denied the execution of tool '{}'.", tool));
                                continue;
                            }
                            None => {
                                return Ok(("Error: Approval channel closed.".to_string(), agent.total_usage.clone()));
                            }
                        }
                    } else {
                        return Ok((format!("Approval required to run tool '{}' but no approval channel is available (AUTO-APPROVE is OFF).", tool), agent.total_usage.clone()));
                    }
                }

                let processed_args = process_args(&args);

                // DISABLED: Scribe redesign pending
                // if let Err(e) = agent.scribe.observe(InteractionType::Tool, &format!("Action: {}\nInput: {}", tool, processed_args)).await {
                //     crate::error_log!("Failed to log tool call to memory: {}", e);
                //     if let Some(bus) = &agent.event_bus {
                //         bus.publish(CoreEvent::StatusUpdate { message: format!("Memory logging error: {}", e) });
                //     }
                // }

                let observation = match execute_tool(agent, &tool, &processed_args, &event_bus).await {
                    Ok(output) => output,
                    Err(e) => {
                        let _ = event_bus.publish(CoreEvent::StatusUpdate { message: format!("❌ Tool '{}' failed", tool) });
                        e
                    }
                };

                if kind == ToolKind::Internal {
                    let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", observation.trim());
                    let _ = event_bus.publish(CoreEvent::InternalObservation { data: obs_log.into_bytes() });
                }

                last_observation = Some(observation);
            }
            AgentDecision::MalformedAction(error) => {
                retry_count += 1;
                if retry_count > max_retries {
                    let fatal_error = format!("Fatal: Failed to parse agent response after {} attempts. Last error: {}", max_retries, error);
                    let _ = event_bus.publish(CoreEvent::StatusUpdate { message: fatal_error.clone() });
                    return Ok((fatal_error, agent.total_usage.clone()));
                }

                let _ = event_bus.publish(CoreEvent::StatusUpdate { message: format!("⚠️ {} Retrying ({}/{})", error, retry_count, max_retries) });

                let nudge = format!(
                    "{}\n\n\
                    IMPORTANT: You must follow the ReAct format exactly:\n\
                    Thought: <your reasoning>\n\
                    Action: <tool name>\n\
                    Action Input: <tool arguments>\n\n\
                    Do not include any other text after Action Input.",
                    error
                );
                last_observation = Some(nudge);
                continue;
            }
            AgentDecision::Error(e) => {
                let _ = event_bus.publish(CoreEvent::StatusUpdate { message: "".to_string() });
                return Err(e.into());
            }
            AgentDecision::Stall { reason, tool_failures } => {
                let message = format!(
                    "Worker stalled after {} consecutive tool failures: {}. Please check the task and retry if needed.",
                    tool_failures, reason
                );
                let _ = event_bus.publish(CoreEvent::StatusUpdate { 
                    message: format!("Worker stalled: {}", reason) 
                });
                return Ok((message, agent.total_usage.clone()));
            }
        }
    }
}

fn check_permission_auto_approve(args: &str, permissions: &Option<AgentPermissions>) -> bool {
    if let Some(perms) = permissions {
        if let Some(auto_approve_list) = &perms.auto_approve_commands {
            let cmd_str = if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
                v.get("command").and_then(|c| c.as_str())
                    .or_else(|| v.get("args").and_then(|c| c.as_str()))
                    .unwrap_or(args)
                    .to_string()
            } else {
                args.to_string()
            };
            return auto_approve_list.iter().any(|pattern| matches_pattern(&cmd_str, pattern));
        }
    }
    false
}

fn extract_suggestion(tool: &str, args: &str) -> String {
    if tool == "execute_command" {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
            v.get("command").and_then(|c| c.as_str())
                .or_else(|| v.get("args").and_then(|c| c.as_str()))
                .unwrap_or(args)
                .to_string()
        } else {
            args.to_string()
        }
    } else {
        format!("{} {}", tool, args)
    }
}

fn process_args(args: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        v.get("args")
            .and_then(|a| a.as_str())
            .map(|s| s.to_string())
            .unwrap_or(args.to_string())
    } else {
        args.to_string()
    }
}

async fn execute_tool(
    agent: &mut AgentV2,
    tool: &str,
    args: &str,
    _event_bus: &Arc<EventBus>,
) -> Result<String, String> {
    match agent.tools.get(tool) {
        Some(t) => match t.call(args).await {
            Ok(output) => {
                let output_str = output.as_string();
                // DISABLED: Scribe redesign pending
                // if let Err(log_err) = agent.scribe.observe(InteractionType::Output, &output_str).await {
                //     crate::error_log!("Failed to log tool output to memory: {}", log_err);
                //     event_bus.publish(CoreEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                // }
                Ok(output_str)
            }
            Err(e) => {
                let error_msg = format!("Tool Error: {}. Analyze the failure and try a different command or approach if possible.", e);
                // DISABLED: Scribe redesign pending
                // if let Err(log_err) = agent.scribe.observe(InteractionType::Output, &error_msg).await {
                //     crate::error_log!("Failed to log tool error to memory: {}", log_err);
                //     event_bus.publish(CoreEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                // }
                Err(error_msg)
            }
        },
        None => {
            let error_msg = format!("Error: Tool '{}' not found. Check the available tools list.", tool);
            // DISABLED: Scribe redesign pending
            // if let Err(log_err) = agent.scribe.observe(InteractionType::Output, &error_msg).await {
            //     crate::error_log!("Failed to log tool-not-found error to memory: {}", log_err);
            //     event_bus.publish(CoreEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
            // }
            Err(error_msg)
        }
    }
}
