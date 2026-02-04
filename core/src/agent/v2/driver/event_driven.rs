//! Event-driven driver for AgentV2
use crate::llm::{chat::ChatMessage, TokenUsage};
use crate::agent::event::RuntimeEvent;
use crate::agent::v2::{AgentV2, AgentDecision};
use crate::agent::v2::jobs::JobStatus;
use crate::agent::v2::execution::execute_parallel_tools;
use crate::agent::v2::protocol::AgentRequest;

use std::error::Error as StdError;

/// Event-driven run method with heartbeat loop and budget management.
pub async fn run_event_driven(
    agent: &mut AgentV2,
    history: Vec<ChatMessage>,
    event_tx: tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
    mut interrupt_rx: tokio::sync::mpsc::Receiver<()>,
    mut approval_rx: tokio::sync::mpsc::Receiver<bool>,
) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
    agent.reset(history).await;
    agent.inject_hot_memory(5).await;

    let start_time = std::time::Instant::now();
    let heartbeat_interval = agent.heartbeat_interval;
    let safety_timeout = agent.safety_timeout;

    let mut last_observation = None;
    let mut heartbeat_timer = tokio::time::interval(heartbeat_interval);
    heartbeat_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut recovery_attempts = 0;

    loop {
        if start_time.elapsed() > safety_timeout {
            let message = format!("⚠️ Safety timeout reached ({:?}). Stopping autonomous run.", safety_timeout);
            let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });
            return Ok((message, agent.total_usage.clone()));
        }

        if agent.iteration_count >= agent.max_steps {
            let message = format!("⚠️ Step budget exceeded ({}/{}). Requesting permission to continue...",
                agent.iteration_count, agent.max_steps);
            let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });

            match approval_rx.recv().await {
                Some(true) => {
                    agent.max_steps = (agent.max_steps as f64 * 1.5) as usize;
                    let continue_msg = format!("✅ Budget increased to {} steps. Continuing...", agent.max_steps);
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: continue_msg });
                }
                Some(false) => {
                    let stop_msg = "🛑 User denied budget increase. Stopping execution.".to_string();
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: stop_msg.clone() });
                    return Ok((stop_msg, agent.total_usage.clone()));
                }
                None => {
                    let error_msg = "❌ Approval channel closed. Stopping execution.".to_string();
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: error_msg.clone() });
                    return Ok((error_msg, agent.total_usage.clone()));
                }
            }
        }

        tokio::select! {
            _ = heartbeat_timer.tick() => {
                handle_heartbeat(agent, &event_tx, &mut last_observation).await;
            }

            result = agent.step(last_observation.take()) => {
                match handle_step_result(agent, result, &event_tx, &mut last_observation, &mut recovery_attempts).await {
                    Some(result) => return result,
                    None => continue,
                }
            }

            _ = interrupt_rx.recv() => {
                let message = "🛑 Execution interrupted by user.".to_string();
                let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });
                return Ok((message, agent.total_usage.clone()));
            }
        }
    }
}

/// Stuck job detection timeout (15 seconds no activity + no tokens)
const STUCK_JOB_TIMEOUT_SECONDS: i64 = 15;

async fn handle_heartbeat(
    agent: &mut AgentV2,
    event_tx: &tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
    last_observation: &mut Option<String>,
) {
    // Check for stuck jobs
    let stuck_jobs = agent.job_registry.detect_stuck_jobs(STUCK_JOB_TIMEOUT_SECONDS);
    for (job_id, description, last_activity) in stuck_jobs {
        // Notify main LLM about stuck job (don't auto-cancel)
        let notification = format!(
            "⚠️ STUCK JOB DETECTED: Worker '{}' (ID: {}) has been inactive for {}+ seconds with no token usage. \
             Last activity: {}. This worker may be waiting on a long-running command or stuck in an error loop. \
             You can: 1) Wait longer if it's expected, 2) Cancel it with /jobs cancel {}",
            description,
            &job_id[..8.min(job_id.len())],
            STUCK_JOB_TIMEOUT_SECONDS,
            last_activity.format("%H:%M:%S"),
            &job_id[..8.min(job_id.len())]
        );
        
        let _ = event_tx.send(RuntimeEvent::StatusUpdate { 
            message: notification.clone() 
        });
        
        // Add to observation so main agent sees it
        *last_observation = Some(notification);
        
        // Mark as notified to prevent spam
        agent.job_registry.mark_stuck_notified(&job_id);
    }

    let active_jobs = agent.job_registry.poll_updates();
    if !active_jobs.is_empty() {
        for job in active_jobs {
            match job.status {
                JobStatus::Completed => {
                    let result_str = job.result
                        .as_ref()
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| "Job completed successfully".to_string());

                    let message = format!("✅ Background job '{}' completed: {}", job.description, result_str);
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });

                    let observation = format!("Background job '{}' result: {}", job.description, result_str);
                    *last_observation = Some(observation);
                }
                JobStatus::Failed => {
                    let error_msg = job.error
                        .as_ref()
                        .map(|e| e.as_str())
                        .unwrap_or("Unknown error");

                    let message = format!("❌ Background job '{}' failed: {}", job.description, error_msg);
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });

                    let observation = format!("Background job '{}' failed: {}", job.description, error_msg);
                    *last_observation = Some(observation);
                }
                JobStatus::Running => {
                    let message = format!("⏳ Background job '{}' is still running...", job.description);
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message });
                }
                JobStatus::Cancelled => {
                    let message = format!("🛑 Background job '{}' was cancelled", job.description);
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });
                    *last_observation = Some(format!("Background job '{}' was cancelled by user", job.description));
                }
            }
        }
    }
}

async fn handle_step_result(
    agent: &mut AgentV2,
    result: Result<AgentDecision, Box<dyn StdError + Send + Sync>>,
    event_tx: &tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
    last_observation: &mut Option<String>,
    recovery_attempts: &mut usize,
) -> Option<Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>>> {
    match result {
        Ok(decision) => {
            *recovery_attempts = 0;
            match decision {
                AgentDecision::Message(msg, usage) => {
                    let _ = event_tx.send(RuntimeEvent::AgentResponse {
                        content: msg.clone(),
                        usage: usage.clone()
                    });

                    // Check for final answer indicators
                    if msg.contains("Final Answer:") || msg.contains("\"f\":") || msg.contains("\"final_answer\":") {
                        let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                            message: "Task completed successfully.".to_string()
                        });
                        return Some(Ok((msg, usage)));
                    }

                    // For worker agents (sub-agents spawned via delegate), we should NOT return early
                    // on conversational messages. Instead, we push back with a strong reminder to use tools.
                    // Only return early if it's clearly a question to the user (ends with ?) or
                    // if the message explicitly indicates waiting for user input.
                    let is_question_to_user = msg.trim().ends_with('?') && 
                        (msg.contains("you") || msg.contains("your") || msg.contains("please provide"));
                    
                    if is_question_to_user {
                        let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                            message: "Agent is asking for clarification.".to_string()
                        });
                        return Some(Ok((msg, usage)));
                    }

                    // For conversational filler like "I'll do that", push back with strong reminder
                    *last_observation = Some(
                        "CRITICAL: You are a WORKER AGENT. You MUST use tools to complete your task. \
                         DO NOT just describe what you will do. \
                         EXECUTE the tool call using the Short-Key JSON format: {\"t\": \"thought\", \"a\": \"tool_name\", \"i\": \"arguments\"}\
                         If you have completed the task, provide a Final Answer using: {\"f\": \"your result\"}".to_string()
                    );
                }
                AgentDecision::Action { tool, args, kind: _ } => {
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                        message: format!("Executing tool: '{}'", tool)
                    });

                    // Execute the tool and set the observation for the next step
                    match execute_single_tool(agent, &tool, &args, event_tx).await {
                        Ok(observation) => {
                            *last_observation = Some(observation);
                        }
                        Err(e) => {
                            *last_observation = Some(format!("Tool execution error: {}", e));
                        }
                    }
                }
                AgentDecision::MalformedAction(error) => {
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                        message: format!("⚠️ Malformed action: {}. Retrying...", error)
                    });
                    *last_observation = Some(format!("Error: {}. Please follow the correct format.", error));
                }
                AgentDecision::Error(e) => {
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                        message: format!("❌ Agent error: {}", e)
                    });
                    return Some(Ok((format!("Error: {}", e), agent.total_usage.clone())));
                }
            }
        }
        Err(e) => {
            *recovery_attempts += 1;
            if *recovery_attempts > 3 {
                let error_msg = format!("❌ Recovery failed after 3 attempts. Last error: {}", e);
                let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                    message: error_msg.clone()
                });
                return Some(Ok((error_msg, agent.total_usage.clone())));
            }

            let recovery_msg = format!("⚠️ Hard Error detected: {}. Entering Recovery Mode (60s cooldown). Attempt {}/3...", e, recovery_attempts);
            let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                message: recovery_msg.clone()
            });

            // Note: The sleep and interrupt check would need to be handled at the caller level
            // For now, we'll just set the observation and continue
            *last_observation = Some(format!("Recovery mode after error: {}", e));
        }
    }
    None
}

/// Execute a single tool and return the observation string
async fn execute_single_tool(
    agent: &AgentV2,
    tool_name: &str,
    args: &str,
    event_tx: &tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
) -> Result<String, Box<dyn StdError + Send + Sync>> {
    // Parse the arguments
    let input: serde_json::Value = if let Ok(v) = serde_json::from_str(args) {
        v
    } else {
        serde_json::Value::String(args.to_string())
    };

    // Create an AgentRequest for the tool
    let request = AgentRequest {
        id: Some(format!("call_{}", agent.iteration_count)),
        thought: format!("Executing {}", tool_name),
        action: tool_name.to_string(),
        input,
    };

    // Execute using the parallel execution function (works for single tools too)
    let results = execute_parallel_tools(
        vec![request],
        &agent.tools,
        agent.scribe.clone(),
        &agent.permissions,
        &Some(event_tx.clone()),
    ).await?;

    // Extract the observation from the result
    if let Some(result) = results.first() {
        if let Some(error) = &result.error {
            return Ok(format!("Tool error: {}", error.message));
        }
        if let Some(res) = &result.result {
            return Ok(res.to_string());
        }
    }

    Ok("Tool executed successfully but returned no output.".to_string())
}
