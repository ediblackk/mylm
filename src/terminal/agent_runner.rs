//! Agent loop execution and PaCoRe reasoning
use mylm_core::agent::v2::jobs::JobStatus;
use mylm_core::agent::{Agent, AgentDecision, ToolKind};
use mylm_core::llm::chat::ChatMessage;
use mylm_core::llm::TokenUsage;
use mylm_core::pacore::exp::Exp;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;

pub use mylm_core::terminal::app::{AppState, TuiEvent};

/// Run the main agent loop with the given configuration
pub async fn run_agent_loop(
    agent: Arc<Mutex<Agent>>,
    event_tx: UnboundedSender<TuiEvent>,
    interrupt_flag: Arc<AtomicBool>,
    auto_approve_flag: Arc<AtomicBool>,
    max_driver_loops: usize,
    mut last_observation: Option<String>,
) {
    let mut loop_iteration = 0;
    let mut retry_count = 0;
    let max_retries = 3;

    loop {
        loop_iteration += 1;
        if loop_iteration > max_driver_loops {
            let _ = event_tx.send(TuiEvent::AgentResponse(
                format!(
                    "Error: Driver-level safety limit reached ({} loops). Potential infinite loop detected.",
                    max_driver_loops
                ),
                TokenUsage::default(),
            ));
            let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
            break;
        }

        if interrupt_flag.load(Ordering::SeqCst) {
            let _ = event_tx.send(TuiEvent::AgentResponse(
                "⛔ Task interrupted by user.".to_string(),
                TokenUsage::default(),
            ));
            let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
            break;
        }

        // Poll for completed background jobs
        mylm_core::debug_log!("Polling for completed jobs...");
        let completed_jobs = {
            let agent_lock = agent.lock().await;
            agent_lock.job_registry.poll_updates()
        };
        mylm_core::debug_log!("Poll returned {} jobs", completed_jobs.len());
        if !completed_jobs.is_empty() {
            let mut observations = Vec::new();
            for job in completed_jobs {
                mylm_core::debug_log!(
                    "Job {}: status={:?}, has_result={}, has_error={}",
                    job.id,
                    job.status,
                    job.result.is_some(),
                    job.error.is_some()
                );
                match job.status {
                    JobStatus::Completed => {
                        let result_str = job
                            .result
                            .as_ref()
                            .map(|r| r.to_string())
                            .unwrap_or_else(|| "Job completed successfully".to_string());
                        observations.push(format!(
                            "Background job '{}' result: {}",
                            job.description, result_str
                        ));
                    }
                    JobStatus::Failed => {
                        let error_msg = job
                            .error
                            .as_ref()
                            .map(|e| e.as_str())
                            .unwrap_or("Unknown error");
                        observations.push(format!(
                            "Background job '{}' failed: {}",
                            job.description, error_msg
                        ));
                    }
                    _ => {}
                }
            }
            if !observations.is_empty() {
                mylm_core::debug_log!(
                    "Setting last_observation with {} job messages",
                    observations.len()
                );
                if let Some(existing) = last_observation.take() {
                    last_observation = Some(format!("{}\n{}", existing, observations.join("\n")));
                } else {
                    last_observation = Some(observations.join("\n"));
                }
            }
        }

        let mut agent_lock = agent.lock().await;
        let provider = agent_lock.llm_client.config().provider.to_string();
        let model = agent_lock.llm_client.config().model.clone();
        let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Thinking(format!(
            "{} ({})",
            model, provider
        ))));
        let _ = event_tx.send(TuiEvent::ActivityUpdate {
            summary: "Thinking".to_string(),
            detail: Some(format!("Model: {} | Provider: {}", model, provider)),
        });

        mylm_core::debug_log!(
            "Calling step() with last_observation: {:?}",
            last_observation.as_deref().map(|s| &s[..50.min(s.len())])
        );
        let step_res = agent_lock.step(last_observation.take()).await;
        match step_res {
            Ok(AgentDecision::MalformedAction(error)) => {
                retry_count += 1;
                if retry_count > max_retries {
                    let fatal_error = format!(
                        "Fatal: Failed to parse agent response after {} attempts. Last error: {}",
                        max_retries, error
                    );
                    let _ = event_tx.send(TuiEvent::StatusUpdate(fatal_error.clone()));
                    let _ = event_tx.send(TuiEvent::AgentResponseFinal(fatal_error, TokenUsage::default()));
                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
                    break;
                }

                let _ = event_tx.send(TuiEvent::StatusUpdate(format!(
                    "⚠️ {} Retrying ({}/{})",
                    error, retry_count, max_retries
                )));

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
                drop(agent_lock);
                continue;
            }
            Ok(AgentDecision::Message(msg, usage)) => {
                let has_pending = agent_lock.has_pending_decision();
                if has_pending {
                    retry_count = 0;
                    let _ = event_tx.send(TuiEvent::AgentResponse(msg, usage));
                    drop(agent_lock);
                    continue;
                }
                let _ = event_tx.send(TuiEvent::AgentResponseFinal(msg, usage));
                break;
            }
            Ok(AgentDecision::Action { tool, args, kind }) => {
                retry_count = 0;
                let _ = event_tx.send(TuiEvent::StatusUpdate(format!("Tool: '{}'", tool)));
                let _ = event_tx.send(TuiEvent::ActivityUpdate {
                    summary: format!("Tool: {}", tool),
                    detail: Some(args.clone()),
                });

                if kind == ToolKind::Terminal {
                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::ExecutingTool(tool.clone())));

                    let cmd = if tool == "execute_command" {
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

                    if !auto_approve_flag.load(Ordering::SeqCst) {
                        let _ = event_tx.send(TuiEvent::SuggestCommand(cmd));
                        let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::WaitingForUser));
                        let _ = event_tx.send(TuiEvent::ActivityUpdate {
                            summary: "Waiting for approval".to_string(),
                            detail: Some("Auto-approve is OFF".to_string()),
                        });
                        break;
                    }

                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = event_tx.send(TuiEvent::ExecuteTerminalCommand(cmd.clone(), tx));
                    drop(agent_lock);

                    match rx.await {
                        Ok(output) => {
                            last_observation = Some(output.clone());
                            let agent_lock = agent.lock().await;
                            if let Some(store) = &agent_lock.memory_store {
                                if let Ok(memory_id) = store
                                    .record_command(&cmd, &output, 0, Some(agent_lock.session_id.clone()))
                                    .await
                                {
                                    let content = format!("Command: {}\nOutput: {}", cmd, output);
                                    let _ = agent_lock.auto_categorize(memory_id, &content).await;
                                }
                            }
                        }
                        Err(_) => {
                            mylm_core::error_log!(
                                "run_agent_loop: Terminal command execution failed (channel closed) for cmd: {}",
                                cmd
                            );
                            last_observation = Some(
                                "Error: Terminal command execution failed (channel closed)".to_string(),
                            );
                        }
                    }
                } else {
                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::ExecutingTool(tool.clone())));

                    let mut observation = String::new();
                    let mut success = false;
                    let mut retry_count = 0;

                    while !success && retry_count < 2 {
                        let t_args = if retry_count == 0 {
                            args.clone()
                        } else {
                            args.clone()
                        };

                        let call_res = match agent_lock.tool_registry.execute_tool(&tool, &t_args).await {
                            Ok(output) => Ok(output),
                            Err(e) => Err(Box::new(std::io::Error::other(e))
                                as Box<dyn std::error::Error + Send + Sync>),
                        };

                        match call_res {
                            Ok(out) => {
                                observation = out.as_string();
                                success = true;
                            }
                            Err(e) => {
                                let err_str: String = e.to_string();
                                if err_str.contains("allowlist") || err_str.contains("dangerous") {
                                    let _ = event_tx.send(TuiEvent::AgentResponseFinal(
                                        format!("⛔ Terminal command blocked: {}", err_str),
                                        TokenUsage::default(),
                                    ));
                                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::WaitingForUser));
                                    let _ = event_tx.send(TuiEvent::ActivityUpdate {
                                        summary: "Action Blocked".to_string(),
                                        detail: Some(err_str),
                                    });
                                    return;
                                }

                                if err_str.contains("not found in registry") || err_str.contains("not available") {
                                    let _ = event_tx.send(TuiEvent::AgentResponseFinal(
                                        format!("❌ Tool Error: {}", err_str),
                                        TokenUsage::default(),
                                    ));
                                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::WaitingForUser));
                                    let _ = event_tx.send(TuiEvent::ActivityUpdate {
                                        summary: "Tool Not Found".to_string(),
                                        detail: Some(err_str.clone()),
                                    });
                                    return;
                                }

                                observation = format!("Error: {}", e);
                                retry_count += 1;
                            }
                        }
                    }

                    let detail = if observation.len() > 1200 {
                        Some(format!("{}… [truncated]", &observation[..1200]))
                    } else {
                        Some(observation.clone())
                    };
                    if tool == "web_search" {
                        let _ = event_tx.send(TuiEvent::ActivityUpdate {
                            summary: "Web search results".to_string(),
                            detail,
                        });
                    } else if tool == "crawl" {
                        let _ = event_tx.send(TuiEvent::ActivityUpdate {
                            summary: "Crawl results".to_string(),
                            detail,
                        });
                    }

                    last_observation = Some(observation);
                    drop(agent_lock);
                }
            }
            Ok(AgentDecision::Error(e)) => {
                mylm_core::error_log!("Agent Decision Error: {}", e);
                let _ = event_tx.send(TuiEvent::AgentResponseFinal(
                    format!("❌ Agent Error: {}", e),
                    TokenUsage::default(),
                ));
                let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Error(e)));
                break;
            }
            Err(e) => {
                mylm_core::error_log!("Agent Loop Error: {}", e);
                let _ = event_tx.send(TuiEvent::AgentResponseFinal(
                    format!("❌ System Error: {}", e),
                    TokenUsage::default(),
                ));
                let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Error(e.to_string())));
                break;
            }
        }
    }
}

/// Run PaCoRe reasoning task
pub async fn run_pacore_task(
    agent: Arc<Mutex<Agent>>,
    history: Vec<ChatMessage>,
    event_tx: UnboundedSender<TuiEvent>,
    interrupt_flag: Arc<AtomicBool>,
    pacore_rounds: &str,
    config: mylm_core::config::Config,
) {
    use futures::StreamExt;

    let rounds_vec: Vec<usize> = pacore_rounds
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if rounds_vec.is_empty() {
        let _ = event_tx.send(TuiEvent::AgentResponseFinal(
            "Error: No valid rounds configured. Use /pacore rounds <comma-separated numbers>"
                .to_string(),
            TokenUsage::default(),
        ));
        let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
        return;
    }

    let (llm_client, resolved_config) = {
        let agent_lock = agent.lock().await;
        (agent_lock.llm_client.clone(), config.resolve_profile())
    };

    let base_url = resolved_config
        .base_url
        .unwrap_or_else(|| resolved_config.provider.default_url());
    let api_key = match resolved_config.api_key {
        Some(key) => key,
        None => {
            let _ = event_tx.send(TuiEvent::AgentResponseFinal(
                "Error: No API key configured for PaCoRe".to_string(),
                TokenUsage::default(),
            ));
            let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
            return;
        }
    };

    let chat_client = mylm_core::pacore::ChatClient::new(base_url, api_key);
    let model_name = llm_client.config().model.clone();

    let rounds_display = rounds_vec.clone();
    let total_calls: usize = rounds_vec.iter().sum();

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel(100);
    let event_tx_clone = event_tx.clone();

    let num_rounds = rounds_vec.len();
    let rounds_for_progress = rounds_vec.clone();
    tokio::spawn(async move {
        let mut completed_calls = 0usize;
        while let Some(event) = progress_rx.recv().await {
            use mylm_core::pacore::PaCoReProgressEvent;
            let status = match &event {
                PaCoReProgressEvent::RoundStarted {
                    round,
                    total_rounds,
                    calls_in_round,
                } => format!(
                    "PaCoRe Round {}/{} • {} calls starting...",
                    round + 1,
                    total_rounds,
                    calls_in_round
                ),
                PaCoReProgressEvent::CallCompleted {
                    round,
                    call_index: _,
                    total_calls,
                } => {
                    completed_calls += 1;
                    let prev_rounds_total: usize = (0..*round)
                        .map(|r| rounds_for_progress.get(r).copied().unwrap_or(0))
                        .sum();
                    let current_round_completed = completed_calls.saturating_sub(prev_rounds_total);
                    format!(
                        "PaCoRe R{} • {}/{} ✓ [{}/{} total]",
                        round + 1,
                        current_round_completed,
                        total_calls,
                        completed_calls,
                        total_calls
                    )
                }
                PaCoReProgressEvent::SynthesisStarted { round } => {
                    format!("PaCoRe synthesizing round {}...", round + 1)
                }
                PaCoReProgressEvent::StreamingStarted => {
                    "PaCoRe streaming final response...".to_string()
                }
                PaCoReProgressEvent::RoundCompleted {
                    round,
                    responses_received,
                } => format!(
                    "PaCoRe Round {} completed ({} responses)",
                    round + 1,
                    responses_received
                ),
                PaCoReProgressEvent::Error { round, error } => {
                    format!("PaCoRe error in round {}: {}", round + 1, error)
                }
                _ => continue,
            };

            let _ = event_tx_clone.send(TuiEvent::StatusUpdate(status));

            if let PaCoReProgressEvent::CallCompleted { round, .. } = event {
                let _ = event_tx_clone.send(TuiEvent::PaCoReProgress {
                    completed: completed_calls,
                    total: total_calls,
                    current_round: round + 1,
                    total_rounds: num_rounds,
                });
            }
        }
    });

    let exp = Exp::new(model_name, rounds_vec, 10, chat_client).with_progress_callback(move |e| {
        let _ = progress_tx.try_send(e);
    });

    let _ = event_tx.send(TuiEvent::StatusUpdate(format!(
        "PaCoRe reasoning with rounds: {:?} ({} total calls)",
        rounds_display, total_calls
    )));

    let pacore_messages: Vec<mylm_core::pacore::model::Message> = history
        .iter()
        .map(|msg| mylm_core::pacore::model::Message {
            role: match msg.role {
                mylm_core::llm::chat::MessageRole::User => "user",
                mylm_core::llm::chat::MessageRole::Assistant => "assistant",
                mylm_core::llm::chat::MessageRole::System => "system",
                mylm_core::llm::chat::MessageRole::Tool => "tool",
            }
            .to_string(),
            content: msg.content.clone(),
            name: None,
            tool_calls: None,
        })
        .collect();

    match exp.process_single_stream(pacore_messages, "tui").await {
        Ok(mut stream) => {
            let mut full_response = String::new();

            while let Some(chunk_result) = stream.next().await {
                if interrupt_flag.load(Ordering::SeqCst) {
                    let _ = event_tx
                        .send(TuiEvent::StatusUpdate("PaCoRe interrupted by user".to_string()));
                    break;
                }

                match chunk_result {
                    Ok(chunk) => {
                        if let Some(choice) = chunk.choices.first() {
                            if let Some(delta) = &choice.delta {
                                full_response.push_str(&delta.content);
                            } else if let Some(message) = &choice.message {
                                full_response.push_str(&message.content);
                            }
                        }
                    }
                    Err(e) => {
                        let _ = event_tx
                            .send(TuiEvent::StatusUpdate(format!("PaCoRe stream error: {}", e)));
                    }
                }
            }

            if !full_response.is_empty() {
                let _ = event_tx.send(TuiEvent::AgentResponseFinal(full_response, TokenUsage::default()));
            }
        }
        Err(e) => {
            let _ = event_tx.send(TuiEvent::AgentResponseFinal(
                format!("PaCoRe error: {}", e),
                TokenUsage::default(),
            ));
        }
    }

    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
}
