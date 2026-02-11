//! Orchestrator Agent Loops
//!
//! Main execution loops for V1 and V2 agents, including chat session loop.

use super::helpers::{execute_terminal_tool, execute_tool, handle_error, poll_jobs, record_to_memory};
use super::types::{ChatSessionMessage, OrchestratorConfig};
use crate::agent::event_bus::{CoreEvent, EventBus};
use crate::agent::traits::TerminalExecutor;
use crate::agent::v2::jobs::{ActionType, JobRegistry};
use crate::agent::v2::protocol::AgentDecision as V2AgentDecision;
use crate::agent::v2::AgentV2;
use crate::agent::{Agent, AgentDecision as V1AgentDecision, ToolKind};
use crate::llm::chat::ChatMessage;
use crate::llm::TokenUsage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;
use tokio::time::Duration;

/// The main agent loop implementation (V1 agent)
pub async fn run_agent_loop_v1(
    agent: Arc<Mutex<Agent>>,
    event_bus: Arc<EventBus>,
    interrupt_flag: Arc<AtomicBool>,
    config: OrchestratorConfig,
    job_registry: JobRegistry,
    _job_id: Option<String>,
    terminal_delegate: Option<Arc<dyn TerminalExecutor>>,
    _task: String,
    history: Vec<ChatMessage>,
    mut approval_rx: Option<Receiver<bool>>,
) -> Result<(), String> {
    let mut loop_iteration = 0;
    let mut retry_count = 0;
    let _smart_wait_iterations = 0;
    let mut last_observation: Option<String> = None;
    
    // Reset agent with history
    {
        let mut agent_lock = agent.lock().await;
        agent_lock.reset(history).await;
    }
    
    loop {
        loop_iteration += 1;
        
        // Safety limit check
        if loop_iteration > config.max_driver_loops {
            event_bus.publish(CoreEvent::AgentResponse {
                content: format!(
                    "Error: Driver-level safety limit reached ({} loops). Potential infinite loop detected.",
                    config.max_driver_loops
                ),
                usage: TokenUsage::default(),
            });
            return Err("Max driver loops exceeded".to_string());
        }
        
        // Poll jobs
        let (job_observations, _has_new) = match poll_jobs(&interrupt_flag, &config, &job_registry, &None, &event_bus) {
            Ok(v) => v,
            Err(e) if e == "interrupted" => return Ok(()),
            Err(e) => return Err(e),
        };
        
        if !job_observations.is_empty() {
            let observation_text = job_observations.join("\n");
            last_observation = Some(match last_observation {
                Some(existing) => format!("{}\n{}", existing, observation_text),
                None => observation_text,
            });
        }
        
        // Execute agent step
        let mut agent_lock = agent.lock().await;
        let model = agent_lock.llm_client.config().model.clone();
        
        event_bus.publish(CoreEvent::AgentThinking { model });
        
        let step_res = agent_lock.step(last_observation.take()).await;
        
        match step_res {
            Ok(V1AgentDecision::MalformedAction(error)) => {
                retry_count += 1;
                if retry_count > config.max_retries {
                    let fatal_error = format!(
                        "Fatal: Failed to parse agent response after {} attempts. Last error: {}",
                        config.max_retries, error
                    );
                    event_bus.publish(CoreEvent::StatusUpdate {
                        message: fatal_error.clone(),
                    });
                    event_bus.publish(CoreEvent::AgentResponse {
                        content: fatal_error,
                        usage: TokenUsage::default(),
                    });
                    return Err("Max retries exceeded for malformed action".to_string());
                }
                
                event_bus.publish(CoreEvent::StatusUpdate {
                    message: format!("⚠️ {} Retrying ({}/{})", error, retry_count, config.max_retries),
                });
                
                let nudge = format!(
                    "{}\n\nIMPORTANT: You must follow the ReAct format exactly:\n\
                    Thought: <your reasoning>\n\
                    Action: <tool name>\n\
                    Action Input: <tool arguments>\n\n\
                    Do not include any other text after Action Input.",
                    error
                );
                last_observation = Some(nudge);
                continue;
            }
            Ok(V1AgentDecision::Message(msg, usage)) => {
                let has_pending = agent_lock.has_pending_decision();
                if has_pending {
                    retry_count = 0;
                    event_bus.publish(CoreEvent::AgentResponse {
                        content: msg,
                        usage,
                    });
                    continue;
                }
                event_bus.publish(CoreEvent::AgentResponse {
                    content: msg,
                    usage,
                });
                return Ok(());
            }
            Ok(V1AgentDecision::Action { tool, args, kind }) => {
                retry_count = 0;
                
                // Check approval for ALL tools when auto_approve is off
                if !config.auto_approve {
                    event_bus.publish(CoreEvent::ToolAwaitingApproval {
                        tool: tool.clone(),
                        args: args.clone(),
                        approval_id: format!("{}-{}", tool, std::time::SystemTime::now().elapsed().unwrap_or_default().as_millis()),
                    });
                    
                    if let Some(ref mut rx) = approval_rx {
                        crate::info_log!("Tool '{}' waiting for user approval...", tool);
                        match rx.recv().await {
                            Some(true) => {
                                crate::info_log!("Tool '{}' approved by user, executing...", tool);
                            }
                            Some(false) => {
                                last_observation = Some(format!("❌ Tool '{}' was rejected by user", tool));
                                continue;
                            }
                            None => {
                                last_observation = Some(format!("❌ Approval channel closed for tool '{}'", tool));
                                continue;
                            }
                        }
                    } else {
                        last_observation = Some(format!("❌ Approval required for tool '{}' but no approval channel available", tool));
                        continue;
                    }
                }
                
                event_bus.publish(CoreEvent::ToolExecuting {
                    tool: tool.clone(),
                    args: args.clone(),
                });
                
                if kind == ToolKind::Terminal {
                    drop(agent_lock);
                    
                    match execute_terminal_tool(&tool, &args, true, event_bus.clone(), &terminal_delegate, &mut None).await {
                        Ok(output) => {
                            let agent_lock = agent.lock().await;
                            let _ = record_to_memory(&*agent_lock, &tool, &args, &output, &TokenUsage::default()).await;
                            drop(agent_lock);
                            
                            last_observation = Some(format!(
                                "--- TERMINAL CONTEXT ---\nCMD_OUTPUT:\n{}",
                                output
                            ));
                        }
                        Err(e) => {
                            last_observation = Some(e);
                        }
                    }
                } else {
                    let tool_registry = agent_lock.tool_registry.clone();
                    drop(agent_lock);
                    
                    match execute_tool(&tool, &args, Some(&tool_registry), None, &event_bus, &None, &job_registry).await {
                        Ok(output) => {
                            let agent_lock = agent.lock().await;
                            let _ = record_to_memory(&*agent_lock, &tool, &args, &output, &TokenUsage::default()).await;
                            drop(agent_lock);
                            
                            last_observation = Some(output);
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            if err_str.contains("allowlist") || err_str.contains("dangerous") {
                                event_bus.publish(CoreEvent::AgentResponse {
                                    content: format!("⛔ Terminal command blocked: {}", err_str),
                                    usage: TokenUsage::default(),
                                });
                                return Ok(());
                            }
                            last_observation = Some(format!("Error: {}", e));
                        }
                    }
                }
                continue;
            }
            Ok(V1AgentDecision::Error(e)) => handle_error(&event_bus, &format!("Agent Error: {}", e))?,
            Ok(V1AgentDecision::Stall { reason, tool_failures }) => {
                let message = format!(
                    "Worker stalled after {} consecutive tool failures: {}. Please check the task and retry if needed.",
                    tool_failures, reason
                );
                event_bus.publish(CoreEvent::StatusUpdate {
                    message: format!("Worker stalled: {}", reason),
                });
                event_bus.publish(CoreEvent::AgentResponse {
                    content: message,
                    usage: TokenUsage::default(),
                });
                return Ok(());
            }
            Err(e) => handle_error(&event_bus, &format!("Agent Loop Error: {}", e))?,
        }
    }
}

/// The main agent loop implementation (V2 agent)
pub async fn run_agent_loop_v2(
    agent: Arc<Mutex<AgentV2>>,
    event_bus: Arc<EventBus>,
    interrupt_flag: Arc<AtomicBool>,
    config: OrchestratorConfig,
    job_registry: JobRegistry,
    job_id: Option<String>,
    terminal_delegate: Option<Arc<dyn TerminalExecutor>>,
    _task: String,
    history: Vec<ChatMessage>,
    mut approval_rx: Option<Receiver<bool>>,
) -> Result<(), String> {
    crate::info_log!("run_agent_loop_v2: Starting with history len={}", history.len());
    
    let mut loop_iteration = 0;
    let mut smart_wait_iterations = 0;
    let mut last_observation: Option<String> = None;
    let mut has_executed_step = false;
    
    // Reset agent with history
    {
        let mut agent_lock = agent.lock().await;
        agent_lock.reset(history).await;
        crate::info_log!("run_agent_loop_v2: Agent reset complete, execute_tools_internally={}", agent_lock.execute_tools_internally);
    }
    
    loop {
        loop_iteration += 1;
        
        // Get current limits from agent
        let (_max_iterations, max_steps) = {
            let agent_lock = agent.lock().await;
            (agent_lock.max_iterations, agent_lock.max_steps)
        };
        
        // Safety limit check
        if loop_iteration > config.max_driver_loops || loop_iteration > max_steps {
            event_bus.publish(CoreEvent::AgentResponse {
                content: format!(
                    "Error: Maximum step limit reached (loop: {}, steps: {}).",
                    loop_iteration, max_steps
                ),
                usage: TokenUsage::default(),
            });
            return Err("Max steps exceeded".to_string());
        }
        
        // Poll jobs
        let (job_observations, has_new_observations) = match poll_jobs(&interrupt_flag, &config, &job_registry, &job_id, &event_bus) {
            Ok(v) => v,
            Err(e) if e == "interrupted" => return Ok(()),
            Err(e) => return Err(e),
        };
        
        if !job_observations.is_empty() {
            let observation_text = job_observations.join("\n");
            last_observation = Some(match last_observation {
                Some(existing) => format!("{}\n{}", existing, observation_text),
                None => observation_text,
            });
        }
        
        // Active worker count for smart wait
        let active_worker_count = job_registry.list_active_jobs().len();
        
        // SMART WAIT: If no new observations and workers are running, wait
        if has_executed_step && last_observation.is_none() && active_worker_count > 0 && !has_new_observations {
            smart_wait_iterations += 1;
            
            event_bus.publish(CoreEvent::StatusUpdate {
                message: format!(
                    "Smart waiting for {} workers ({}/{})",
                    active_worker_count, smart_wait_iterations, config.max_smart_wait_iterations
                ),
            });
            
            if smart_wait_iterations >= config.max_smart_wait_iterations {
                event_bus.publish(CoreEvent::AgentResponse {
                    content: format!(
                        "⏳ {} worker(s) still running in background. You can send new messages while waiting.",
                        active_worker_count
                    ),
                    usage: TokenUsage::default(),
                });
                return Ok(());
            }
            
            tokio::time::sleep(tokio::time::Duration::from_secs(config.smart_wait_interval_secs)).await;
            continue;
        } else {
            smart_wait_iterations = 0;
        }
        
        // Execute agent step
        let mut agent_lock = agent.lock().await;
        let model = agent_lock.llm_client.config().model.clone();
        let tools_count = agent_lock.tools.len();
        let execute_internally = agent_lock.execute_tools_internally;
        
        crate::info_log!("Orchestrator V2: step starting - model={}, tools={}, execute_internally={}, observation={:?}",
            model, tools_count, execute_internally, last_observation);
        
        event_bus.publish(CoreEvent::AgentThinking { model });
        
        let step_res = agent_lock.step(last_observation.take()).await;
        drop(agent_lock);
        
        has_executed_step = true;
        
        match step_res {
            Ok(V2AgentDecision::Message(msg, usage)) => {
                event_bus.publish(CoreEvent::AgentResponse {
                    content: msg,
                    usage,
                });
                return Ok(());
            }
            Ok(V2AgentDecision::Action { tool, args, kind }) => {
                // Check approval for ALL tools when auto_approve is off
                if !config.auto_approve {
                    event_bus.publish(CoreEvent::ToolAwaitingApproval {
                        tool: tool.clone(),
                        args: args.clone(),
                        approval_id: format!("{}-{}", tool, std::time::SystemTime::now().elapsed().unwrap_or_default().as_millis()),
                    });
                    
                    if let Some(ref mut rx) = approval_rx {
                        crate::info_log!("V2: Tool '{}' waiting for user approval...", tool);
                        match rx.recv().await {
                            Some(true) => {
                                crate::info_log!("V2: Tool '{}' approved by user, executing...", tool);
                            }
                            Some(false) => {
                                last_observation = Some(format!("❌ Tool '{}' was rejected by user", tool));
                                continue;
                            }
                            None => {
                                last_observation = Some(format!("❌ Approval channel closed for tool '{}'", tool));
                                continue;
                            }
                        }
                    } else {
                        last_observation = Some(format!("❌ Approval required for tool '{}' but no approval channel available", tool));
                        continue;
                    }
                }
                
                event_bus.publish(CoreEvent::ToolExecuting {
                    tool: tool.clone(),
                    args: args.clone(),
                });
                
                if let Some(ref jid) = job_id {
                    let action_desc = if args.len() > 50 {
                        format!("{}: {}...", tool, &args[..47])
                    } else {
                        format!("{}: {}", tool, args)
                    };
                    job_registry.add_action(jid, ActionType::ToolCall, &action_desc);
                }
                
                if kind == ToolKind::Terminal {
                    match execute_terminal_tool(&tool, &args, true, event_bus.clone(), &terminal_delegate, &mut None).await {
                        Ok(output) => {
                            if let Some(ref jid) = job_id {
                                job_registry.add_action(jid, ActionType::ToolResult, &format!("{} completed", tool));
                            }
                            
                            let agent_lock = agent.lock().await;
                            let _ = record_to_memory(&*agent_lock, &tool, &args, &output, &TokenUsage::default()).await;
                            drop(agent_lock);
                            
                            last_observation = Some(format!(
                                "--- TERMINAL CONTEXT ---\nCMD_OUTPUT:\n{}",
                                output
                            ));
                        }
                        Err(e) => {
                            if let Some(ref jid) = job_id {
                                let err_str = e.to_string();
                                job_registry.add_action(jid, ActionType::Error, &format!("{} failed: {}", tool, &err_str[..err_str.len().min(50)]));
                            }
                            last_observation = Some(format!("Error executing command: {}", e));
                        }
                    }
                } else {
                    let agent_lock = agent.lock().await;
                    let tools_map = agent_lock.tools.clone();
                    drop(agent_lock);
                    
                    match execute_tool(&tool, &args, None, Some(&tools_map), &event_bus, &job_id, &job_registry).await {
                        Ok(output) => {
                            if let Some(ref jid) = job_id {
                                job_registry.add_action(jid, ActionType::ToolResult, &format!("{} completed", tool));
                            }
                            
                            let agent_lock = agent.lock().await;
                            let _ = record_to_memory(&*agent_lock, &tool, &args, &output, &TokenUsage::default()).await;
                            drop(agent_lock);
                            
                            last_observation = Some(output);
                        }
                        Err(e) => {
                            crate::error_log!("Orchestrator V2: Tool '{}' failed with error: {}", tool, e);
                            if let Some(ref jid) = job_id {
                                let err_str = e.to_string();
                                job_registry.add_action(jid, ActionType::Error, &format!("{} failed: {}", tool, &err_str[..err_str.len().min(50)]));
                            }
                            
                            let err_str = e.to_string();
                            if err_str.contains("allowlist") || err_str.contains("dangerous") {
                                event_bus.publish(CoreEvent::AgentResponse {
                                    content: format!("⛔ Terminal command blocked: {}", err_str),
                                    usage: TokenUsage::default(),
                                });
                                return Ok(());
                            }
                            last_observation = Some(format!("Error: {}", e));
                        }
                    }
                }
                continue;
            }
            Ok(V2AgentDecision::MalformedAction(error)) => {
                event_bus.publish(CoreEvent::StatusUpdate {
                    message: format!("⚠️ Malformed action: {}", error),
                });
                last_observation = Some(format!(
                    "Error: Malformed action. Please use proper format.\nDetails: {}",
                    error
                ));
                continue;
            }
            Ok(V2AgentDecision::Error(e)) => handle_error(&event_bus, &format!("Agent Error: {}", e))?,
            Ok(V2AgentDecision::Stall { reason, tool_failures }) => {
                let message = format!(
                    "Worker stalled after {} consecutive tool failures: {}. Please check the task and retry if needed.",
                    tool_failures, reason
                );
                event_bus.publish(CoreEvent::StatusUpdate {
                    message: format!("Worker stalled: {}", reason),
                });
                event_bus.publish(CoreEvent::AgentResponse {
                    content: message,
                    usage: TokenUsage::default(),
                });
                return Ok(());
            }
            Err(e) => handle_error(&event_bus, &format!("Agent Loop Error: {}", e))?,
        }
    }
}

/// Run the chat session loop for V2 agent
/// 
/// This loop:
/// 1. Waits for user messages or worker events
/// 2. Processes them through the agent
/// 3. Handles tool calls (including delegate for spawning workers)
/// 4. Injects job status into context
/// 5. Runs continuously until interrupted
pub async fn run_chat_session_loop_v2(
    agent: Arc<Mutex<AgentV2>>,
    event_bus: Arc<EventBus>,
    interrupt_flag: Arc<AtomicBool>,
    config: OrchestratorConfig,
    job_registry: JobRegistry,
    terminal_delegate: Option<Arc<dyn TerminalExecutor>>,
    initial_history: Vec<ChatMessage>,
    mut receiver: Receiver<ChatSessionMessage>,
) -> Result<(), String> {
    // Simplified: removed chat session start log
    
    // Reset agent with initial history
    {
        let mut agent_lock = agent.lock().await;
        agent_lock.reset(initial_history).await;
        // Simplified: removed redundant reset complete log
    }
    
    let mut last_observation: Option<String> = None;
    let mut step_count = 0;
    // Track consecutive auto-confirmations to prevent infinite loops
    let mut auto_confirm_count = 0;
    let max_auto_confirm = config.max_driver_loops; // Use configured limit
    
    // CRITICAL FIX: Track delegate calls per user request to prevent duplicate spawning
    // Reset when we receive a new user message
    let mut delegate_call_count = 0;
    let max_delegate_per_user_request = 10; // Safety limit per user request
    
    // Track if graceful shutdown was requested
    let mut shutdown_requested = false;
    
    // Subscribe to EventBus for worker events
    let mut event_rx = event_bus.subscribe();
    
    loop {
        // Check for interruption at start of each iteration
        // Note: We check before starting new work, but let current operations complete
        if !shutdown_requested && interrupt_flag.load(Ordering::SeqCst) {
            crate::info_log!("ChatSession: Graceful shutdown requested. Will stop after current operation completes.");
            shutdown_requested = true;
            event_bus.publish(CoreEvent::StatusUpdate {
                message: "⛔ Stop requested. Completing current operation...".to_string(),
            });
        }
        
        // Check for messages from channel (non-blocking)
        let mut got_user_message = false;
        match receiver.try_recv() {
            Ok(msg) => {
                crate::info_log!("ChatSession: Received message from channel");
                match msg {
                    ChatSessionMessage::UserMessage(chat_msg) => {
                        crate::info_log!("ChatSession: Processing user message from channel ({} chars)", chat_msg.content.len());
                        let mut agent_lock = agent.lock().await;
                        agent_lock.history.push(chat_msg);
                        got_user_message = true;
                    }
                    ChatSessionMessage::WorkerEvent(event) => {
                        crate::info_log!("ChatSession: Processing worker event from channel");
                        last_observation = Some(event);
                    }
                    ChatSessionMessage::Interrupt => {
                        crate::info_log!("ChatSession: Received interrupt message");
                        return Ok(());
                    }
                }
            }
            Err(_) => {}
        }
        
        // Reset iteration counter for each new user message turn
        if got_user_message {
            let mut agent_lock = agent.lock().await;
            agent_lock.reset_iteration_counter();
            agent_lock.set_iteration_limit(200);
            auto_confirm_count = 0; // Reset auto-confirm counter on user input
            delegate_call_count = 0; // Reset delegate counter on new user message
            crate::info_log!("ChatSession: Reset iteration counter and set limit to 200 for new user message");
        }
        
        // Poll for completed worker jobs and add as observation
        let (job_observations, has_new) = match poll_jobs(&interrupt_flag, &config, &job_registry, &None, &event_bus) {
            Ok(v) => v,
            Err(e) if e == "interrupted" => return Ok(()),
            Err(e) => return Err(e),
        };
        // Reduced logging - only log if there are observations
        if !job_observations.is_empty() {
            crate::info_log!("ChatSession: poll_jobs returned {} observations, has_new={}", job_observations.len(), has_new);
        }
        
        if !job_observations.is_empty() {
            let obs_text = job_observations.join("\n");
            crate::info_log!("ChatSession: Adding job observation: {}", obs_text);
            last_observation = Some(match last_observation {
                Some(existing) => format!("{}\n{}", existing, obs_text),
                None => obs_text,
            });
        }
        
        // Check if channel has pending messages (non-blocking)
        // NOTE: We don't consume the message here, just check if the channel is not empty
        let has_pending_channel_message = !receiver.is_empty();
        let has_observation = last_observation.is_some();
        
        // Check if there's a pending user message in history that needs response
        let has_pending_user_message = {
            let agent_lock = agent.lock().await;
            agent_lock.history.last()
                .map(|m| m.role == crate::llm::chat::MessageRole::User)
                .unwrap_or(false)
        };
        
        // REMOVED: Proactive job status injection was causing the agent to waste tokens
        // checking job status when it should just wait for events.
        // Job completions are now handled purely via WorkerCompleted events.
        if !has_pending_channel_message && !has_observation && !has_pending_user_message {
            // Nothing to do - check if we should shutdown first
            if shutdown_requested {
                crate::info_log!("ChatSession: Shutdown requested during idle, exiting.");
                return Ok(());
            }
            // Block efficiently until a message OR EventBus event arrives
            crate::info_log!("ChatSession: Nothing to process, waiting for events...");
            loop {
                let mut got_user_message = false;
                let mut got_observation = false;
                
                tokio::select! {
                    msg = receiver.recv() => {
                        match msg {
                            Some(ChatSessionMessage::UserMessage(chat_msg)) => {
                                let mut agent_lock = agent.lock().await;
                                agent_lock.history.push(chat_msg);
                                got_user_message = true;
                            }
                            Some(ChatSessionMessage::WorkerEvent(event)) => {
                                last_observation = Some(event);
                                got_observation = true;
                            }
                            Some(ChatSessionMessage::Interrupt) => {
                                return Ok(());
                            }
                            None => {
                                crate::info_log!("ChatSession: Channel closed, exiting");
                                return Ok(());
                            }
                        }
                    }
                    event = event_rx.recv() => {
                        match event {
                            Ok(CoreEvent::WorkerCompleted { job_id, result }) => {
                                crate::info_log!("ChatSession: Worker {} completed while idle", job_id);
                                // Format observation with explicit instruction
                                let obs = format!(
                                    "✅ WORKER TASK COMPLETED: Worker {} finished successfully.\nResult: {}\n\nThe delegated task is now COMPLETE. Report the result to the user and ask if they need anything else.",
                                    job_id, result
                                );
                                last_observation = Some(obs);
                                got_observation = true;
                            }
                            Ok(CoreEvent::WorkerSpawned { .. }) |
                            Ok(CoreEvent::StatusUpdate { .. }) |
                            Ok(CoreEvent::ToolExecuting { .. }) => {
                                // Informational events - ignore and keep waiting
                                // Simplified: removed verbose wait loop log
                            }
                            Err(_) => {
                                // Channel closed or lagging, resubscribe
                                crate::info_log!("ChatSession: EventBus lagging, resubscribing");
                                event_rx = event_bus.subscribe();
                            }
                            _ => {
                                // Unknown event - ignore and keep waiting
                            }
                        }
                    }
                }
                
                // Only break out of the inner loop if we got a meaningful event
                if got_user_message || got_observation {
                    break;
                }
                // Otherwise, continue waiting in the inner loop
            }
        }
        
        // Safety limit - only count actual work iterations
        step_count += 1;
        if step_count > config.max_driver_loops {
            event_bus.publish(CoreEvent::AgentResponse {
                content: format!("Error: Maximum iteration limit ({}) reached.", config.max_driver_loops),
                usage: TokenUsage::default(),
            });
            return Err("Max iterations exceeded".to_string());
        }
        
        // Execute agent step
        let mut agent_lock = agent.lock().await;
        let model = agent_lock.llm_client.config().model.clone();
        
        event_bus.publish(CoreEvent::AgentThinking { model });
        
        // Simplified: removed step tracing logs
        let step_res = agent_lock.step(last_observation.take()).await;
        drop(agent_lock);
        
        match step_res {
            Ok(V2AgentDecision::Message(msg, usage)) => {
                // Simplified: removed message returned log
                event_bus.publish(CoreEvent::AgentResponse {
                    content: msg.clone(),
                    usage: usage.clone(),
                });
                
                // Add assistant response to history
                let mut agent_lock = agent.lock().await;
                agent_lock.history.push(ChatMessage::assistant(&msg));
                
                // Check if there's a pending decision (ReAct-style: chat first, act after approval)
                let has_pending = agent_lock.has_pending_decision();
                drop(agent_lock);
                
                if has_pending {
                    auto_confirm_count += 1;
                    if auto_confirm_count > max_auto_confirm {
                        crate::warn_log!("ChatSession: Auto-confirm limit reached ({}). Breaking potential infinite loop.", max_auto_confirm);
                        event_bus.publish(CoreEvent::AgentResponse {
                            content: format!("⚠️ Auto-confirm limit ({}). Agent will stop and wait for your response.", max_auto_confirm),
                            usage: TokenUsage::default(),
                        });
                        // Clear the pending decision and action queue to stop the loop
                        let mut agent_lock = agent.lock().await;
                        agent_lock.pending_decision = None;
                        agent_lock.pending_action_queue.clear();
                        drop(agent_lock);
                        auto_confirm_count = 0;
                        // Fall through to wait for user input
                    } else {
                        crate::info_log!("ChatSession: Agent has pending decision after Message - auto-confirming ({}/{})", auto_confirm_count, max_auto_confirm);
                        last_observation = Some("User confirmed: proceed with the action".to_string());
                        continue;
                    }
                } else {
                    // Reset counter when agent returns Message without pending decision
                    auto_confirm_count = 0;
                }
                
                // Check if shutdown was requested before blocking on recv
                if shutdown_requested {
                    crate::info_log!("ChatSession: Shutdown requested, exiting without waiting for user input.");
                    return Ok(());
                }
                
                // Wait for next user message or job events
                // Simplified: active_jobs count removed - let _active_jobs = job_registry.list_active_jobs().len();
                // Event-driven: wait for user messages OR EventBus events
                // No more polling - we wake up only when something happens
                // CRITICAL FIX: Loop until we get a meaningful event (not just informational)
                loop {
                    let mut got_user_message = false;
                    let mut got_worker_completion = false;
                    
                    
                    tokio::select! {
                        msg = receiver.recv() => {
                            match msg {
                                Some(ChatSessionMessage::UserMessage(chat_msg)) => {
                                    let mut agent_lock = agent.lock().await;
                                    agent_lock.history.push(chat_msg);
                                    got_user_message = true;
                                }
                                Some(ChatSessionMessage::WorkerEvent(event)) => {
                                    last_observation = Some(event);
                                    got_user_message = true; // Treat worker events as requiring processing
                                }
                                Some(ChatSessionMessage::Interrupt) => return Ok(()),
                                None => {
                                    crate::warn_log!("ChatSession: Channel closed");
                                    return Ok(());
                                }
                            }
                        }
                        event = event_rx.recv() => {
                            match event {
                                Ok(CoreEvent::WorkerCompleted { job_id, result }) => {
                                    crate::info_log!("ChatSession: Worker {} completed", job_id);
                                    // Format observation with explicit instruction
                                    let obs = format!(
                                        "✅ WORKER TASK COMPLETED: Worker {} finished successfully.\nResult: {}\n\nThe delegated task is now COMPLETE. Report the result to the user and ask if they need anything else.",
                                        job_id, result
                                    );
                                    last_observation = Some(obs);
                                    got_worker_completion = true;
                                }
                                Ok(CoreEvent::WorkerSpawned { .. }) |
                                Ok(CoreEvent::ToolExecuting { .. }) |
                                Ok(CoreEvent::StatusUpdate { .. }) => {
                                    // Informational events - ignore and keep waiting
                                    // Simplified: removed verbose wait loop logs
                                }
                                Err(_) => {
                                    // Channel closed or lagging, resubscribe and keep waiting
                                    crate::info_log!("ChatSession: EventBus lagging, resubscribing");
                                    event_rx = event_bus.subscribe();
                                }
                                _ => {
                                    // Unknown event - keep waiting
                                }
                            }
                        }
                    }
                    
                    // Only break out of the inner loop if we got a meaningful event
                    if got_user_message || got_worker_completion {
                        break;
                    }
                    // Otherwise, continue waiting in the inner loop
                }
            }
            Ok(V2AgentDecision::Action { tool, args, kind }) => {
                crate::info_log!("ChatSession: Agent returned Action(tool={}, kind={:?}) at step_count {}", tool, kind, step_count);
                
                // Check approval for ALL tools when auto_approve is off
                let mut tool_executing_already_published = false;
                if !config.auto_approve {
                    event_bus.publish(CoreEvent::ToolAwaitingApproval {
                        tool: tool.clone(),
                        args: args.clone(),
                        approval_id: format!("{}-{}", tool, std::time::SystemTime::now().elapsed().unwrap_or_default().as_millis()),
                    });
                    
                    // Wait for approval via event channel
                    let approved = loop {
                        tokio::select! {
                            msg = receiver.recv() => {
                                match msg {
                                    Some(ChatSessionMessage::UserMessage(_)) => {
                                        // Treat new user message as implicit approval
                                        crate::info_log!("ChatSession: User sent new message, treating as approval");
                                        break true;
                                    }
                                    Some(ChatSessionMessage::Interrupt) => return Ok(()),
                                    None => {
                                        crate::warn_log!("ChatSession: Channel closed during approval wait");
                                        return Ok(());
                                    }
                                    _ => {}
                                }
                            }
                            event = event_rx.recv() => {
                                match event {
                                    Ok(CoreEvent::ToolExecuting { .. }) => {
                                        // UI signaled approval by publishing ToolExecuting
                                        crate::info_log!("ChatSession: ToolExecuting event received, proceeding with execution");
                                        tool_executing_already_published = true;
                                        break true;
                                    }
                                    Ok(CoreEvent::StatusUpdate { message }) if message.contains("rejected") || message.contains("denied") => {
                                        crate::info_log!("ChatSession: Tool was rejected by user");
                                        break false;
                                    }
                                    Err(_) => {
                                        event_rx = event_bus.subscribe();
                                    }
                                    _ => {}
                                }
                            }
                        }
                    };
                    
                    if !approved {
                        last_observation = Some(format!("❌ Tool '{}' was rejected by user", tool));
                        continue;
                    }
                }
                
                // Publish tool executing event (only if not already published by UI)
                if !tool_executing_already_published {
                    event_bus.publish(CoreEvent::ToolExecuting {
                        tool: tool.clone(),
                        args: args.clone(),
                    });
                }
                
                // CRITICAL FIX: Block excessive worker spawning per user request
                if tool == "delegate" {
                    delegate_call_count += 1;
                    
                    // Safety limit check - only block if exceeding reasonable limit per request
                    if delegate_call_count > max_delegate_per_user_request {
                        crate::warn_log!("ChatSession: BLOCKING delegate call - exceeded safety limit of {} calls", max_delegate_per_user_request);
                        event_bus.publish(CoreEvent::AgentResponse {
                            content: format!("⚠️ Safety limit reached: Cannot spawn more than {} sets of workers per request.", max_delegate_per_user_request),
                            usage: TokenUsage::default(),
                        });
                        last_observation = Some(format!("ERROR: Exceeded maximum of {} delegate calls. Report current status to user.", max_delegate_per_user_request));
                        continue;
                    }
                }
                
                if kind == ToolKind::Terminal {
                    event_bus.publish(CoreEvent::StatusUpdate {
                        message: format!("Executing: {}", tool),
                    });
                    
                    if let Some(ref delegate) = terminal_delegate {
                        let command = if tool == "execute_command" {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                                v.get("command")
                                    .and_then(|c| c.as_str())
                                    .or_else(|| v.get("args").and_then(|c| c.as_str()))
                                    .unwrap_or(&args)
                                    .to_string()
                            } else {
                                args.clone()
                            }
                        } else {
                            format!("{} {}", tool, args)
                        };
                        
                        match delegate.execute_command(command, Some(Duration::from_secs(30))).await {
                            Ok(output) => {
                                last_observation = Some(output);
                            }
                            Err(e) => {
                                last_observation = Some(format!("Error: {}", e));
                            }
                        }
                    } else {
                        last_observation = Some("Error: Terminal delegate not available".to_string());
                    }
                } else {
                    let agent_lock = agent.lock().await;
                    let tools_map = agent_lock.tools.clone();
                    let execute_internally = agent_lock.execute_tools_internally;
                    drop(agent_lock);
                    
                    if execute_internally {
                        // Agent handles internally
                    } else {
                        if let Some(tool_impl) = tools_map.get(&tool) {
                            match tool_impl.call(&args).await {
                                Ok(output) => {
                                    last_observation = Some(output.as_string());
                                    if tool == "delegate" {
                                        event_bus.publish(CoreEvent::StatusUpdate {
                                            message: "Workers spawned".to_string(),
                                        });
                                        // CRITICAL FIX: After spawning workers, return to user
                                        // Don't immediately continue - let user see the result
                                        event_bus.publish(CoreEvent::AgentResponse {
                                            content: "Background workers have been spawned. They will continue running while you chat.".to_string(),
                                            usage: TokenUsage::default(),
                                        });
                                        // Skip to waiting for events, don't call agent.step again immediately
                                        last_observation = None;
                                        continue;
                                    }
                                }
                                Err(e) => {
                                    last_observation = Some(format!("Error: {}", e));
                                }
                            }
                        } else {
                            last_observation = Some(format!("Error: Tool '{}' not found", tool));
                        }
                    }
                }
            }
            Ok(V2AgentDecision::MalformedAction(error)) => {
                crate::info_log!("ChatSession: Agent returned MalformedAction at step_count {}: {}", step_count, error);
                event_bus.publish(CoreEvent::StatusUpdate {
                    message: format!("⚠️ Malformed action: {}", error),
                });
                last_observation = Some(format!(
                    "Error: Malformed action. Please use proper format.\nDetails: {}",
                    error
                ));
            }
            Ok(V2AgentDecision::Error(e)) => {
                event_bus.publish(CoreEvent::AgentResponse {
                    content: format!("Error: {}", e),
                    usage: TokenUsage::default(),
                });
            }
            Ok(V2AgentDecision::Stall { reason, tool_failures }) => {
                let message = format!(
                    "Worker stalled after {} consecutive tool failures: {}. Please check the task and retry if needed.",
                    tool_failures, reason
                );
                event_bus.publish(CoreEvent::StatusUpdate {
                    message: format!("Worker stalled: {}", reason),
                });
                event_bus.publish(CoreEvent::AgentResponse {
                    content: message,
                    usage: TokenUsage::default(),
                });
            }
            Err(e) => {
                crate::error_log!("ChatSession: Agent step error: {}", e);
                event_bus.publish(CoreEvent::StatusUpdate {
                    message: format!("Agent error: {}", e),
                });
            }
        }
        
        // Check if graceful shutdown was requested after current operation completes
        if shutdown_requested {
            crate::info_log!("ChatSession: Graceful shutdown - current operation complete, exiting.");
            event_bus.publish(CoreEvent::AgentResponse {
                content: "⛔ Chat session stopped. Current operation completed.".to_string(),
                usage: TokenUsage::default(),
            });
            return Ok(());
        }
    }
}
