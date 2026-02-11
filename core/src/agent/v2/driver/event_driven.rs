//! Event-driven driver for AgentV2
use crate::llm::{chat::ChatMessage, TokenUsage};
// RuntimeEvent import removed - now using EventBus
use crate::agent::event_bus::{CoreEvent, EventBus};
use crate::agent::v2::{AgentV2, AgentDecision};
use crate::agent::v2::jobs::{JobStatus, BackgroundJob};
use crate::agent::v2::execution::execute_parallel_tools;
use crate::agent::v2::protocol::AgentRequest;

use futures_util::FutureExt;
use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug)]
struct LoopState {
    recovery_attempts: u32,
    consecutive_messages: u32,
    action_count: usize,
    last_action_summary: String,
    pending_observation: Option<String>,
    start_time: Instant,
}

impl LoopState {
    fn new() -> Self {
        Self {
            recovery_attempts: 0,
            consecutive_messages: 0,
            action_count: 0,
            last_action_summary: String::new(),
            pending_observation: None,
            start_time: Instant::now(),
        }
    }

    fn reset_recovery(&mut self) {
        self.recovery_attempts = 0;
    }

    fn record_successful_tool_use(&mut self) {
        self.consecutive_messages = 0;
    }
}

enum StepOutcome {
    Continue,
    Done {
        message: String,
        usage: TokenUsage,
    },
}

// Note: MAX_ACTIONS_BEFORE_STALL now comes from agent.max_actions_before_stall (configurable)

/// Event-driven run method with heartbeat loop and budget management.
pub async fn run_event_driven(
    agent: &mut AgentV2,
    history: Vec<ChatMessage>,
    event_bus: Arc<EventBus>,
    mut interrupt_rx: tokio::sync::mpsc::Receiver<()>,
    mut approval_rx: tokio::sync::mpsc::Receiver<bool>,
) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
    agent.reset(history).await;
    crate::info_log!("run_event_driven: after reset, history has {} messages", agent.history.len());
    agent.inject_hot_memory(5).await;
    crate::info_log!("run_event_driven: after inject_hot_memory, history has {} messages", agent.history.len());
    
    // Debug: print message sizes
    for (i, msg) in agent.history.iter().enumerate() {
        crate::info_log!("  Message[{}]: role={:?}, len={} chars", i, msg.role, msg.content.len());
    }

    let heartbeat_interval = agent.heartbeat_interval;
    let safety_timeout = agent.safety_timeout;

    let mut state = LoopState::new();
    let mut heartbeat_timer = tokio::time::interval(heartbeat_interval);
    heartbeat_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        if state.start_time.elapsed() > safety_timeout {
            let message = format!("⚠️ Safety timeout reached ({:?}). Stopping autonomous run.", safety_timeout);
            event_bus.publish(CoreEvent::StatusUpdate { message: message.clone() });
            return Ok((message, agent.total_usage.clone()));
        }

        // Log only every 10 iterations to reduce noise
        if agent.iteration_count % 10 == 0 {
            crate::info_log!("Worker loop: iteration={}/{}, checking limits...", agent.iteration_count, agent.max_steps);
        }
        // ── Guard: step budget ──
        if let Some(outcome) = check_budget(agent, &event_bus, &mut approval_rx).await {
            return outcome;
        }

        // CRITICAL FIX: The heartbeat timer was cancelling agent.step() via tokio::select!
        // This caused LLM calls to be dropped and retried indefinitely, stalling workers
        // Solution: Check heartbeat non-blocking before step, and also include in select 
        // when there's no pending observation (so step would just be waiting anyway)
        
        // Check heartbeat without blocking (if it's ready, handle it)
        if heartbeat_timer.tick().now_or_never().is_some() {
            crate::info_log!("[HEARTBEAT] Main agent heartbeat firing (pre-step check)");
            handle_heartbeat(agent, &event_bus, &mut state.pending_observation).await;
        }
        
        // Run the step with interrupt check. 
        // If there's no pending observation (agent is idle waiting), also check heartbeat
        // so we can detect stalled workers and other async events.
        let pending_obs = state.pending_observation.take();
        let is_idle = pending_obs.is_none();
        
        if is_idle {
            // Agent is idle - include heartbeat in select so we process async events
            // Clone pending_obs since select! might evaluate multiple branches
            let pending_obs_clone = pending_obs.clone();
            tokio::select! {
                biased;
                
                _ = interrupt_rx.recv() => {
                    let message = "🛑 Execution interrupted by user.".to_string();
                    event_bus.publish(CoreEvent::StatusUpdate { message: message.clone() });
                    return Ok((message, agent.total_usage.clone()));
                }
                
                _ = heartbeat_timer.tick() => {
                    // Heartbeat fired while idle - handle it and continue loop
                    crate::info_log!("[HEARTBEAT] Main agent heartbeat firing during idle");
                    handle_heartbeat(agent, &event_bus, &mut state.pending_observation).await;
                    // Put back the observation since we didn't process it
                    state.pending_observation = pending_obs_clone;
                    continue;
                }
                
                result = agent.step(pending_obs) => {
                    match handle_step_result(agent, result, &event_bus, &mut state).await {
                        StepOutcome::Continue => continue,
                        StepOutcome::Done { message, usage } => return Ok((message, usage)),
                    }
                }
            }
        } else {
            // Agent has work to do - don't include heartbeat to avoid cancelling LLM calls
            tokio::select! {
                biased;
                
                _ = interrupt_rx.recv() => {
                    let message = "🛑 Execution interrupted by user.".to_string();
                    event_bus.publish(CoreEvent::StatusUpdate { message: message.clone() });
                    return Ok((message, agent.total_usage.clone()));
                }
                
                result = agent.step(pending_obs) => {
                    match handle_step_result(agent, result, &event_bus, &mut state).await {
                        StepOutcome::Continue => continue,
                        StepOutcome::Done { message, usage } => return Ok((message, usage)),
                    }
                }
            }
        }
    }
}

/// Stuck job detection timeout (15 seconds no activity + no tokens)
const STUCK_JOB_TIMEOUT_SECONDS: i64 = 15;

async fn handle_heartbeat(
    agent: &mut AgentV2,
    event_bus: &EventBus,
    pending_observation: &mut Option<String>,
) {
    let mut observations = Vec::new();

    // Stuck job detection
    for (job_id, description, last_activity) in
        agent.job_registry.detect_stuck_jobs(STUCK_JOB_TIMEOUT_SECONDS)
    {
        let short_id = &job_id[..job_id.len().min(8)];
        let notification = format!(
            "STUCK JOB: Worker '{description}' (ID: {short_id}) inactive {STUCK_JOB_TIMEOUT_SECONDS}s+. \
             Last activity: {}. Cancel with /jobs cancel {short_id}",
            last_activity.format("%H:%M:%S")
        );
        event_bus.publish(CoreEvent::StatusUpdate { message: notification.clone() });
        observations.push(notification);
        agent.job_registry.mark_stuck_notified(&job_id);
    }

    // Job status polling
    let active_jobs = agent.job_registry.poll_updates();
    if !active_jobs.is_empty() {
        for job in active_jobs {
            if let Some((message, observation)) = format_job_update(&job) {
                event_bus.publish(CoreEvent::StatusUpdate { message: message.clone() });
                if let Some(obs) = observation {
                    observations.push(obs);
                }
            }
        }
    }

    // Combine all observations and append to existing pending_observation
    if !observations.is_empty() {
        let combined = observations.join("\n\n");
        match pending_observation {
            Some(existing) => {
                existing.push_str("\n\n");
                existing.push_str(&combined);
            }
            None => *pending_observation = Some(combined),
        }
    }
}

/// Check if the step budget has been exceeded and request approval if needed.
/// Returns Some if we should exit (denied/channel closed), None to continue.
async fn check_budget(
    agent: &mut AgentV2,
    event_bus: &EventBus,
    approval_rx: &mut tokio::sync::mpsc::Receiver<bool>,
) -> Option<Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>>> {
    if agent.iteration_count < agent.max_steps {
        return None;
    }

    let msg = format!(
        "Step budget exceeded ({}/{}). Requesting approval...",
        agent.iteration_count, agent.max_steps
    );
    event_bus.publish(CoreEvent::StatusUpdate { message: msg });

    match approval_rx.recv().await {
        Some(true) => {
            agent.max_steps = (agent.max_steps as f64 * 1.5) as usize;
            event_bus.publish(CoreEvent::StatusUpdate {
                message: format!("Budget increased to {} steps.", agent.max_steps),
            });
            None // continue
        }
        Some(false) => {
            let msg = "User denied budget increase. Stopping.".to_string();
            event_bus.publish(CoreEvent::StatusUpdate { message: msg.clone() });
            Some(Ok((msg, agent.total_usage.clone())))
        }
        None => {
            let msg = "Approval channel closed. Stopping.".to_string();
            event_bus.publish(CoreEvent::StatusUpdate { message: msg.clone() });
            Some(Ok((msg, agent.total_usage.clone())))
        }
    }
}

/// Reminder message for worker agents to use tools
const WORKER_TOOL_USE_REMINDER: &str = "CRITICAL: You are a WORKER AGENT. You MUST use tools to complete your task. \
DO NOT just describe what you will do. \
EXECUTE the tool call using the Short-Key JSON format: {\"t\": \"thought\", \"a\": \"tool_name\", \"i\": \"arguments\"}\
If you have completed the task, provide a Final Answer using: {\"f\": \"your result\"}";

/// Check if a message indicates a final answer
fn is_final_answer(msg: &str) -> bool {
    msg.contains("Final Answer:")
        || msg.contains("\"f\":")
        || msg.contains("\"final_answer\":")
}

/// Check if a message is a question directed at the user
fn is_user_question(msg: &str) -> bool {
    let trimmed = msg.trim();
    trimmed.ends_with('?')
        && (trimmed.contains("you")
            || trimmed.contains("your")
            || trimmed.contains("please provide"))
}

/// Format a job update into user-facing messages
fn format_job_update(job: &BackgroundJob) -> Option<(String, Option<String>)> {
    match job.status {
        JobStatus::Completed => {
            let result = job.result.as_ref()
                .map(|r| r.to_string())
                .unwrap_or_else(|| "completed successfully".into());
            Some((
                format!("Job '{}' completed: {result}", job.description),
                Some(format!("Background job '{}' result: {result}", job.description)),
            ))
        }
        JobStatus::Failed => {
            let err = job.error.as_deref().unwrap_or("Unknown error");
            Some((
                format!("Job '{}' failed: {err}", job.description),
                Some(format!("Background job '{}' failed: {err}", job.description)),
            ))
        }
        JobStatus::Running => {
            Some((format!("Job '{}' still running...", job.description), None))
        }
        JobStatus::Cancelled => {
            Some((
                format!("Job '{}' cancelled", job.description),
                Some(format!("Background job '{}' cancelled by user", job.description)),
            ))
        }
        JobStatus::TimeoutPending => {
            Some((
                format!("Job '{}' timed out, cleaning up...", job.description),
                Some(format!("Background job '{}' timed out", job.description)),
            ))
        }
        JobStatus::Stalled => {
            Some((
                format!("Job '{}' STALLED", job.description),
                Some(format!("Background job '{}' stalled, needs intervention", job.description)),
            ))
        }
    }
}

async fn handle_step_result(
    agent: &mut AgentV2,
    result: Result<AgentDecision, Box<dyn StdError + Send + Sync>>,
    event_bus: &EventBus,
    state: &mut LoopState,
) -> StepOutcome {
    match result {
        Ok(decision) => {
            state.reset_recovery();
            process_decision(agent, decision, event_bus, state).await
        }
        Err(e) => process_error(agent, e, event_bus, state).await,
    }
}

async fn process_decision(
    agent: &mut AgentV2,
    decision: AgentDecision,
    event_bus: &EventBus,
    state: &mut LoopState,
) -> StepOutcome {
    match decision {
        AgentDecision::Message(msg, usage) => {
            process_message(msg, usage, event_bus, state, agent).await
        }
        AgentDecision::Action { tool, args, kind: _ } => {
            process_action(agent, tool, args, event_bus, state).await
        }
        AgentDecision::MalformedAction(error) => {
            event_bus.publish(CoreEvent::StatusUpdate {
                message: format!("Malformed action: {error}. Retrying..."),
            });
            state.pending_observation =
                Some(format!("Error: {error}. Please follow the correct format."));
            StepOutcome::Continue
        }
        AgentDecision::Error(e) => {
            event_bus.publish(CoreEvent::StatusUpdate {
                message: format!("Agent error: {e}"),
            });
            StepOutcome::Done {
                message: format!("Error: {e}"),
                usage: agent.total_usage.clone(),
            }
        }
        AgentDecision::Stall { reason, tool_failures } => {
            event_bus.publish(CoreEvent::StatusUpdate {
                message: format!("Worker stalled: {} ({} consecutive failures)", reason, tool_failures),
            });
            StepOutcome::Done {
                message: format!("Worker stalled: {}. Please check the task and retry if needed.", reason),
                usage: agent.total_usage.clone(),
            }
        }
    }
}

async fn process_message(
    msg: String,
    usage: TokenUsage,
    event_bus: &EventBus,
    state: &mut LoopState,
    agent: &AgentV2,
) -> StepOutcome {
    let job_id = agent.llm_client.get_job_id();
    if let Some(jid) = &job_id {
         crate::info_log!("process_message: [DIAGNOSTIC] Worker {} publishing AgentResponse to global bus (content len={})", jid, msg.len());
    }
    
    // WORKER GUARD: Workers should NOT publish AgentResponse to global EventBus.
    // Only main agent (no job_id) publishes to chat pane.
    if job_id.is_none() {
        event_bus.publish(CoreEvent::AgentResponse {
            content: msg.clone(),
            usage: usage.clone(),
        });
    } else {
        crate::info_log!("Worker {}: suppressing AgentResponse (content len={})",
            job_id.as_ref().unwrap(), msg.len());
    }

    // Check for final answer indicators
    if is_final_answer(&msg) {
        return StepOutcome::Done { message: msg, usage };
    }

    // For worker agents (sub-agents spawned via delegate), we should NOT return early
    // on conversational messages. Instead, we push back with a strong reminder to use tools.
    // Only return early if it's clearly a question to the user (ends with ?) or
    // if the message explicitly indicates waiting for user input.
    if is_user_question(&msg) {
        event_bus.publish(CoreEvent::StatusUpdate {
            message: "Agent is asking for clarification.".to_string()
        });
        return StepOutcome::Done { message: msg, usage };
    }

    // Track consecutive non-tool, non-final-answer messages to prevent infinite loops
    state.consecutive_messages += 1;
    if state.consecutive_messages >= agent.max_consecutive_messages {
        let msg_preview: String = msg.chars().take(100).collect();
        let error_msg = format!(
            "Worker aborted after {} consecutive conversational messages without tool execution. \
             Workers must use tools to complete tasks. Final message: {}",
            state.consecutive_messages, msg_preview
        );
        crate::error_log!("[WORKER] {}", error_msg);
        event_bus.publish(CoreEvent::StatusUpdate {
            message: error_msg.clone()
        });
        return StepOutcome::Done { message: error_msg, usage };
    }

    // For conversational filler like "I'll do that", push back with strong reminder
    state.pending_observation = Some(WORKER_TOOL_USE_REMINDER.to_string());
    StepOutcome::Continue
}

async fn process_action(
    agent: &mut AgentV2,
    tool: String,
    args: String,
    event_bus: &EventBus,
    state: &mut LoopState,
) -> StepOutcome {
    // WORKER GUARD: Only main agent publishes tool events to UI status bar.
    // Workers (background jobs) should not leak their tool executions to main chat.
    let is_worker = agent.llm_client.get_job_id().is_some();

    if !is_worker {
        event_bus.publish(CoreEvent::ToolExecuting {
            tool: tool.clone(),
            args: args.clone(),
        });
        event_bus.publish(CoreEvent::StatusUpdate {
            message: format!("Executing tool: '{tool}'"),
        });
    }

    // Stall detection - check BEFORE incrementing to stay within budget
    if state.action_count >= agent.max_actions_before_stall {
        let stall_reason = format!(
            "Worker exceeded action budget ({} actions) without returning a final answer. \
             Last action: {}. \
             This job is now STALLED and requires main agent intervention.",
            state.action_count,
            state.last_action_summary
        );
        
        // Mark job as stalled in registry
        if let Some(job_id) = agent.llm_client.get_job_id() {
            agent.job_registry.stall_job(&job_id, &stall_reason, state.action_count);
            
            // Publish WorkerStalled event so UI updates immediately
            event_bus.publish(CoreEvent::WorkerStalled {
                job_id: job_id.clone(),
                reason: stall_reason.clone(),
            });
        }
        
        event_bus.publish(CoreEvent::StatusUpdate {
            message: format!("⚠️ Job stalled: {}", stall_reason)
        });
        
        return StepOutcome::Done { message: format!("STALLED: {}", stall_reason), usage: agent.total_usage.clone() };
    }
    
    // Increment action count AFTER stall check
    state.action_count += 1;
    state.last_action_summary =
        format!("{tool} with args: {}", args.chars().take(100).collect::<String>());
    
    // Publish action progress so UI can show it (main agent only)
    if !is_worker {
        event_bus.publish(CoreEvent::StatusUpdate {
            message: format!("Action {}/{}: {}", state.action_count, agent.max_actions_before_stall, tool),
        });
    }

    // Execute tool
    match execute_single_tool(agent, &tool, &args, &event_bus).await {
        Ok(observation) => {
            state.record_successful_tool_use();
            state.pending_observation = Some(observation);
        }
        Err(e) => {
            state.pending_observation = Some(format!("Tool execution error: {e}"));
        }
    }

    StepOutcome::Continue
}

async fn process_error(
    agent: &mut AgentV2,
    error: Box<dyn StdError + Send + Sync>,
    event_bus: &EventBus,
    state: &mut LoopState,
) -> StepOutcome {
    state.recovery_attempts += 1;

    if state.recovery_attempts > agent.max_recovery_attempts {
        let msg = format!("Recovery failed after {} attempts. Last error: {error}", agent.max_recovery_attempts);
        event_bus.publish(CoreEvent::StatusUpdate { message: msg.clone() });
        return StepOutcome::Done {
            message: msg,
            usage: agent.total_usage.clone(),
        };
    }

    event_bus.publish(CoreEvent::StatusUpdate {
        message: format!(
            "Error: {error}. Recovery attempt {}/{}",
            state.recovery_attempts,
            agent.max_recovery_attempts
        ),
    });

    // TODO: The 60s cooldown was removed. Add back with:
    // tokio::time::sleep(RECOVERY_COOLDOWN).await;
    state.pending_observation = Some(format!("Recovery mode after error: {error}"));
    StepOutcome::Continue
}

/// Execute a single tool and return the observation string
async fn execute_single_tool(
    agent: &AgentV2,
    tool_name: &str,
    args: &str,
    _event_bus: &EventBus,
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

    // Get job_id for action logging
    let job_id_opt = agent.llm_client.get_job_id();
    let job_id = job_id_opt.as_deref();
    
    // Execute using the parallel execution function (works for single tools too)
    let results = execute_parallel_tools(
        vec![request],
        &agent.tools,
        agent.scribe.clone(),
        &agent.permissions,
        &None, // No internal event channel
        &Some(agent.job_registry.clone()),
        job_id,
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
