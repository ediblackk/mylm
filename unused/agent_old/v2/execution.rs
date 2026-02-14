//! Parallel tool execution for AgentV2
use crate::agent_old::event_bus::RuntimeEvent;
use crate::agent_old::permissions::check_tool_permission;
use crate::agent_old::tool::{Tool, ToolOutput};
use crate::agent_old::v2::protocol::{AgentError, AgentRequest, AgentResponse};
use crate::agent_old::v2::jobs::{JobRegistry, ActionType};
use crate::config::types::AgentPermissions;
// DISABLED: Scribe redesign pending - use crate::memory::journal::InteractionType;
use crate::memory::scribe::Scribe;
use std::collections::HashMap;
use std::sync::Arc;

/// Execute multiple tool requests in parallel.
pub async fn execute_parallel_tools(
    requests: Vec<AgentRequest>,
    tools: &HashMap<String, Arc<dyn Tool>>,
    scribe: Arc<Scribe>,
    permissions: &Option<AgentPermissions>,
    event_tx: &Option<tokio::sync::mpsc::UnboundedSender<RuntimeEvent>>,
    job_registry: &Option<Arc<JobRegistry>>,
    job_id: Option<&str>,
) -> Result<Vec<AgentResponse>, Box<dyn std::error::Error + Send + Sync>> {
    let mut futures = Vec::new();

    for req in requests {
        let event_tx = event_tx.clone();
        let tool = tools.get(&req.action).cloned();
        let _scribe = scribe.clone();
        let permissions = permissions.clone();
        let job_registry = job_registry.clone();
        let job_id = job_id.map(|s| s.to_string());
        let action_name = req.action.clone();

        futures.push(async move {
            if let Some(tx) = &event_tx {
                let _: Result<(), _> = tx.send(RuntimeEvent::Step { request: req.clone() });
            }

            let response = match tool {
                Some(t) => {
                    let args = if let Some(s) = req.input.as_str() {
                        s.to_string()
                    } else {
                        req.input.to_string()
                    };

                    // Log action to job registry if available (includes args for visibility)
                    if let (Some(registry), Some(jid)) = (&job_registry, &job_id) {
                        let action_with_args = if args.len() > 50 {
                            format!("{}: {}...", action_name, &args[..47])
                        } else {
                            format!("{}: {}", action_name, args)
                        };
                        registry.add_action(jid, ActionType::ToolCall, &action_with_args);
                    }

                    // DISABLED: Scribe redesign pending
                    // if let Err(e) = scribe.observe(InteractionType::Tool, &format!("Action: {}\nInput: {}", req.action, args)).await {
                    //     crate::error_log!("Failed to log tool call to memory: {}", e);
                    //     if let Some(tx) = &event_tx {
                    //         let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", e) });
                    //     }
                    // }

                    // Check tool permission
                    if let Some(ref perms) = permissions {
                        if let Err(e) = check_tool_permission(&req.action, perms) {
                            return AgentResponse {
                                result: None,
                                error: Some(AgentError {
                                    message: format!("{}", e),
                                    code: Some("PERMISSION_DENIED".to_string()),
                                    context: None,
                                }),
                            };
                        }
                    }

                    match t.call(&args).await {
                        Ok(output) => {
                            let output_str = output.as_string();
                            
                            // Log tool result to job registry
                            if let (Some(registry), Some(jid)) = (&job_registry, &job_id) {
                                registry.add_action(jid, ActionType::ToolResult, &format!("{} completed", action_name));
                            }
                            
                            // DISABLED: Scribe redesign pending
                            // if let Err(log_err) = scribe.observe(InteractionType::Output, &output_str).await {
                            //     crate::error_log!("Failed to log tool output to memory: {}", log_err);
                            //     if let Some(tx) = &event_tx {
                            //         let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                            //     }
                            // }
                            AgentResponse {
                                result: Some(serde_json::json!({
                                    "output": output_str,
                                    "id": req.id.clone().unwrap_or_default(),
                                    "status": match output {
                                        ToolOutput::Immediate(_) => "immediate",
                                        ToolOutput::Background { .. } => "background",
                                    }
                                })),
                                error: None,
                            }
                        }
                        Err(e) => {
                            let error_msg = e.to_string();
                            
                            // Log error to job registry
                            if let (Some(registry), Some(jid)) = (&job_registry, &job_id) {
                                registry.add_action(jid, ActionType::Error, &format!("{} failed: {}", action_name, &error_msg[..error_msg.len().min(50)]));
                            }
                            
                            // DISABLED: Scribe redesign pending
                            // if let Err(log_err) = scribe.observe(InteractionType::Output, &format!("Error: {}", error_msg)).await {
                            //     crate::error_log!("Failed to log tool error to memory: {}", log_err);
                            //     if let Some(tx) = &event_tx {
                            //         let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                            //     }
                            // }
                            AgentResponse {
                                result: None,
                                error: Some(AgentError {
                                    message: error_msg,
                                    code: Some("TOOL_ERROR".to_string()),
                                    context: None,
                                }),
                            }
                        }
                    }
                }
                None => AgentResponse {
                    result: None,
                    error: Some(AgentError {
                        message: format!("Tool '{}' not found", req.action),
                        code: Some("NOT_FOUND".to_string()),
                        context: None,
                    }),
                },
            };

            if let Some(tx) = &event_tx {
                let _: Result<(), _> = tx.send(RuntimeEvent::ToolOutput { response: response.clone() });
            }

            response
        });
    }

    let results = futures::future::join_all(futures).await;
    Ok(results)
}

/// Execute a single tool and return the result string.
pub async fn execute_single_tool(
    tool_name: &str,
    args: &str,
    tools: &HashMap<String, Arc<dyn Tool>>,
    _scribe: Arc<Scribe>,
    _event_tx: &Option<tokio::sync::mpsc::UnboundedSender<RuntimeEvent>>,
) -> Result<String, String> {
    match tools.get(tool_name) {
        Some(t) => match t.call(args).await {
            Ok(output) => {
                let output_str = output.as_string();
                // DISABLED: Scribe redesign pending
                // if let Err(log_err) = scribe.observe(InteractionType::Output, &output_str).await {
                //     crate::error_log!("Failed to log tool output to memory: {}", log_err);
                //     if let Some(tx) = event_tx {
                //         let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                //     }
                // }
                Ok(output_str)
            }
            Err(e) => {
                let error_msg = format!("Tool Error: {}. Analyze the failure and try a different command or approach if possible.", e);
                // DISABLED: Scribe redesign pending
                // if let Err(log_err) = scribe.observe(InteractionType::Output, &error_msg).await {
                //     crate::error_log!("Failed to log tool error to memory: {}", log_err);
                //     if let Some(tx) = event_tx {
                //         let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                //     }
                // }
                Err(error_msg)
            }
        },
        None => {
            let error_msg = format!("Error: Tool '{}' not found. Check the available tools list.", tool_name);
            // DISABLED: Scribe redesign pending
            // if let Err(log_err) = scribe.observe(InteractionType::Output, &error_msg).await {
            //     crate::error_log!("Failed to log tool-not-found error to memory: {}", log_err);
            //     if let Some(tx) = event_tx {
            //         let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
            //     }
            // }
            Err(error_msg)
        }
    }
}
