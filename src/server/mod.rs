use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex, oneshot};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};
use uuid::Uuid;
use anyhow::{Context, Result};
use base64::Engine;
use async_trait::async_trait;

use mylm_core::config::Config;
use mylm_core::agent::v2::AgentDecision;
use mylm_core::agent::tool::ToolOutput;
use mylm_core::agent::traits::TerminalExecutor;
use mylm_core::llm::chat::ChatMessage;
use mylm_core::terminal::app::TuiEvent;
use mylm_core::protocol::{ServerEvent, ClientMessage, MessageEnvelope, ServerInfo, Capabilities, SessionSummary, SystemInfo};
use std::time::Duration;

/// Stub TerminalExecutor for headless server sessions where no real terminal exists.
pub struct HeadlessTerminalExecutor;

#[async_trait]
impl TerminalExecutor for HeadlessTerminalExecutor {
    async fn execute_command(&self, _cmd: String, _timeout: Option<Duration>) -> Result<String, String> {
        Err("Terminal operations not available in headless mode".to_string())
    }
    
    async fn get_screen(&self) -> Result<String, String> {
        Ok(String::new()) // empty screen
    }
}

pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub sessions: Arc<Mutex<HashMap<Uuid, Arc<SessionRuntime>>>>,
    pub workflows: Arc<Mutex<Vec<mylm_core::protocol::Workflow>>>,
    pub stages: Arc<Mutex<Vec<mylm_core::protocol::Stage>>>,
}

pub struct SessionRuntime {
    pub title: Arc<Mutex<String>>,
    pub status: Arc<Mutex<String>>,
    pub created_at: u64,
    pub agent: Arc<Mutex<mylm_core::BuiltAgent>>,
    pub _event_tx: mpsc::UnboundedSender<TuiEvent>,
    pub terminal_buffer: Arc<Mutex<String>>,
    pub pending_approvals: Arc<Mutex<HashMap<Uuid, oneshot::Sender<bool>>>>,
    pub run_lock: Arc<Mutex<()>>,
    pub auto_approve: bool,
}

pub async fn start_server(port: u16) -> Result<()> {
  let addr = format!("127.0.0.1:{}", port);
  let listener = TcpListener::bind(&addr).await.context("Failed to bind server")?;
  
  println!("mylm Server listening on: ws://{}", addr);

  let config = Arc::new(Mutex::new(Config::load().unwrap_or_default()));
  {
      let cfg = config.lock().await;
      println!("[Server] Config loaded from: {:?}", cfg.save_to_default_location().err());
  }
  let sessions = Arc::new(Mutex::new(HashMap::new()));
    
    let (initial_workflows, initial_stages) = load_workflows().await;
    let workflows = Arc::new(Mutex::new(initial_workflows));
    let stages = Arc::new(Mutex::new(initial_stages));

    let state = Arc::new(AppState {
        config,
        sessions,
        workflows,
        stages,
    });

    while let Ok((stream, _)) = listener.accept().await {
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Ok(ws_stream) = accept_async(stream).await {
                handle_connection(ws_stream, state_clone).await;
            }
        });
    }

    Ok(())
}

async fn handle_connection(
    ws_stream: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    state: Arc<AppState>,
) {
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerEvent>();

    // Task to forward ServerEvents to WebSocket
    let send_task = tokio::spawn(async move {
        let mut event_id = 0;
        while let Some(event) = rx.recv().await {
            event_id += 1;
            let envelope = MessageEnvelope {
                v: 1,
                msg_type: "event".to_string(),
                request_id: None,
                event_id: Some(event_id),
                payload: event,
            };
            if let Ok(json) = serde_json::to_string(&envelope) {
                if ws_sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming WebSocket messages
    while let Some(Ok(msg)) = ws_receiver.next().await {
        if let Message::Text(text) = msg {
            match serde_json::from_str::<MessageEnvelope<ClientMessage>>(text.as_str()) {
                Ok(envelope) => {
                    let _ = handle_client_message(envelope.payload, &state, &tx).await;
                }
                Err(e) => {
                    let preview = if text.len() > 200 {
                        format!("{}...", &text[..200])
                    } else {
                        text.clone()
                    };
                    println!("[Server] Failed to parse client message envelope: {} - Preview: {}", e, preview);
                }
            }
        }
    }
    
    send_task.abort();
}

async fn handle_client_message(
    msg: ClientMessage,
    state: &Arc<AppState>,
    tx: &mpsc::UnboundedSender<ServerEvent>,
) -> Result<()> {
    match msg {
        ClientMessage::Hello { .. } => {
            let _ = tx.send(ServerEvent::HelloAck {
                server: ServerInfo {
                    name: "mylm-server".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                capabilities: Capabilities {
                    terminal: true,
                    approvals: true,
                    tools: vec![],
                },
            });
        }
        ClientMessage::CreateSession { config: custom_config, .. } => {
            let session_id = Uuid::new_v4();
            let mut config = state.config.lock().await.clone();
            
            if let Some(cfg_val) = custom_config {
                if let Ok(overridden) = serde_json::from_value::<mylm_core::config::Config>(cfg_val) {
                    config = overridden;
                    println!("[Server] Using custom configuration for session {}", session_id);
                }
            }

            // Agent version is determined by profile settings in V2
            // For server, we default to V2 behavior
            
            let auto_approve = false; // V2 doesn't have allow_execution setting, default to false for safety
            
            let (event_tx, event_rx) = mpsc::unbounded_channel::<TuiEvent>();
            let event_bus = Arc::new(mylm_core::agent::event_bus::EventBus::new());
            let agent = mylm_core::factory::create_agent_for_session(
                &config, 
                event_bus,
                Arc::new(HeadlessTerminalExecutor),
                None, // No escalation channel for server mode
            ).await?;
            let agent = Arc::new(Mutex::new(agent));

            let created_at = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let runtime = Arc::new(SessionRuntime {
                title: Arc::new(Mutex::new("New Task".to_string())),
                status: Arc::new(Mutex::new("idle".to_string())),
                created_at,
                agent,
                _event_tx: event_tx.clone(),
                terminal_buffer: Arc::new(Mutex::new(String::new())),
                pending_approvals: Arc::new(Mutex::new(HashMap::new())),
                run_lock: Arc::new(Mutex::new(())),
                auto_approve,
            });

            // Start TUI event loop
            tokio::spawn(spawn_tui_event_loop(
                session_id,
                event_rx,
                runtime.clone(),
                tx.clone(),
            ));

            // Start Job Heartbeat for V2
            tokio::spawn(spawn_job_heartbeat(
                session_id,
                runtime.clone(),
                tx.clone(),
            ));

            state.sessions.lock().await.insert(session_id, runtime);
            let _ = tx.send(ServerEvent::SessionCreated { session_id });
            let _ = tx.send(ServerEvent::CreateSessionAck { session_id });
        }
        ClientMessage::ListSessions => {
            let sessions_map = state.sessions.lock().await;
            let mut sessions = Vec::new();
            for (id, runtime) in sessions_map.iter() {
                sessions.push(SessionSummary {
                    session_id: *id,
                    title: runtime.title.lock().await.clone(),
                    status: runtime.status.lock().await.clone(),
                    created_at: runtime.created_at,
                });
            }
            let _ = tx.send(ServerEvent::Sessions { sessions });
        }
        ClientMessage::GetProjectInfo => {
            let root_path = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .to_string_lossy()
                .to_string();
            
            let files = crawl_directory(std::path::Path::new(".")).await.unwrap_or_default();
            let stats = calculate_project_stats(std::path::Path::new(".")).await.ok();
            let _ = tx.send(ServerEvent::ProjectInfo { root_path, files, stats });
        }
        ClientMessage::SendUserMessage { session_id, message } => {
            if let Some(runtime) = state.sessions.lock().await.get(&session_id).cloned() {
                tokio::spawn(run_agent_for_user_message(
                    session_id,
                    runtime,
                    message.text,
                    tx.clone(),
                ));
            }
        }
        ClientMessage::ApproveAction { session_id, approval_id, decision } => {
            if let Some(runtime) = state.sessions.lock().await.get(&session_id) {
                let approve = matches!(decision.as_str(), "approve" | "approved" | "yes" | "true");
                let mut pending = runtime.pending_approvals.lock().await;
                if let Some(sender) = pending.remove(&approval_id) {
                    let _ = sender.send(approve);
                }
            }
        }
        ClientMessage::GetServerConfig => {
            let config = state.config.lock().await.clone();
            let _ = tx.send(ServerEvent::Config {
                config: serde_json::to_value(config).unwrap_or(serde_json::Value::Null),
            });
        }
        ClientMessage::UpdateServerConfig { config: new_config_val } => {
            if let Ok(new_config) = serde_json::from_value::<Config>(new_config_val) {
                {
                    let mut config = state.config.lock().await;
                    *config = new_config.clone();
                    if let Err(e) = config.save_to_default_location() {
                        eprintln!("[Server] Failed to save config: {}", e);
                    }
                }
                // Broadcast the updated config back
                let _ = tx.send(ServerEvent::Config {
                    config: serde_json::to_value(new_config).unwrap_or(serde_json::Value::Null),
                });
            }
        }
        ClientMessage::GetWorkflows => {
            let workflows = state.workflows.lock().await.clone();
            let stages = state.stages.lock().await.clone();
            let _ = tx.send(ServerEvent::Workflows { workflows, stages });
        }
        ClientMessage::SyncWorkflows { workflows: new_workflows, stages: new_stages } => {
            {
                let mut w_lock = state.workflows.lock().await;
                *w_lock = new_workflows.clone();
                let mut s_lock = state.stages.lock().await;
                *s_lock = new_stages.clone();
            }
            if let Err(e) = save_workflows(&new_workflows, &new_stages).await {
                eprintln!("[Server] Failed to save workflows: {}", e);
            }
            // Broadcast back to acknowledge
            let _ = tx.send(ServerEvent::Workflows {
                workflows: new_workflows,
                stages: new_stages,
            });
        }
        ClientMessage::Ping => {
            let _ = tx.send(ServerEvent::Pong);
        }
        ClientMessage::GetSystemInfo => {
            let config_dir = mylm_core::config::get_config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
            let config_path = mylm_core::config::find_config_file()
                .unwrap_or_else(|| config_dir.join("mylm.yaml"));
            
            let info = SystemInfo {
                config_path: config_path.to_string_lossy().to_string(),
                data_path: config_dir.to_string_lossy().to_string(),
                memory_db_path: config_dir.join("memory").to_string_lossy().to_string(),
                sessions_path: config_dir.join("sessions.json").to_string_lossy().to_string(),
                workflows_path: config_dir.join("workflows.json").to_string_lossy().to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            };
            println!("[Server] Sending SystemInfo: {:?}", info);
            let _ = tx.send(ServerEvent::SystemInfo { info });
        }
        ClientMessage::TestConnection { provider, base_url, api_key } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let result = validate_api_key(&provider, base_url.as_deref(), &api_key).await;
                match result {
                    Ok(_) => {
                        let _ = tx.send(ServerEvent::ConnectionTestResult {
                            ok: true,
                            message: "Connection successful".to_string(),
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(ServerEvent::ConnectionTestResult {
                            ok: false,
                            message: e.to_string(),
                        });
                    }
                }
            });
        }
        _ => {}
    }
    Ok(())
}

async fn validate_api_key(provider: &str, base_url: Option<&str>, api_key: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let url = if let Some(base) = base_url {
        if base.ends_with('/') {
            format!("{}models", base)
        } else {
            format!("{}/models", base)
        }
    } else {
        match provider {
            "openai" => "https://api.openai.com/v1/models".to_string(),
            "anthropic" => "https://api.anthropic.com/v1/models".to_string(), // Note: Anthropic models endpoint is actually different but usually /v1 works for base
            "gemini" | "google" => "https://generativelanguage.googleapis.com/v1beta/models".to_string(),
            "ollama" => "http://localhost:11434/api/tags".to_string(),
            _ => return Err(anyhow::anyhow!("Unsupported provider for automatic validation. Please provide a Base URL.")),
        }
    };

    let mut request = client.get(&url);

    // Apply provider-specific headers
    request = match provider {
        "anthropic" => request.header("x-api-key", api_key).header("anthropic-version", "2023-06-01"),
        "gemini" | "google" => request.query(&[("key", api_key)]),
        "ollama" => request, // Ollama usually doesn't need key by default
        _ => request.header("Authorization", format!("Bearer {}", api_key)),
    };

    let response = request.send().await.context("Failed to connect to provider")?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        
        // Try to parse JSON error if possible
        let error_message = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&error_body) {
            if let Some(msg) = json["error"]["message"].as_str() {
                msg.to_string()
            } else if let Some(msg) = json["message"].as_str() {
                msg.to_string()
            } else {
                error_body
            }
        } else {
            error_body
        };

        Err(anyhow::anyhow!("{}: {}", status, error_message))
    }
}

async fn spawn_tui_event_loop(
    session_id: Uuid,
    mut event_rx: mpsc::UnboundedReceiver<TuiEvent>,
    runtime: Arc<SessionRuntime>,
    tx: mpsc::UnboundedSender<ServerEvent>,
) {
    while let Some(ev) = event_rx.recv().await {
        match ev {
            TuiEvent::StatusUpdate(status) => {
                {
                    let mut s = runtime.status.lock().await;
                    *s = status.clone();
                }
                let _ = tx.send(ServerEvent::StatusUpdate {
                    session_id,
                    status: if status.is_empty() { "idle".to_string() } else { status },
                });
            }
            TuiEvent::InternalObservation(bytes) => {
                let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let _ = tx.send(ServerEvent::TerminalOutput { session_id, data });
            }
            TuiEvent::GetTerminalScreen(reply) => {
                let buf = runtime.terminal_buffer.lock().await.clone();
                let _ = reply.send(buf);
            }
            TuiEvent::ExecuteTerminalCommand(cmd, reply) => {
                let _ = tx.send(ServerEvent::Activity {
                    session_id,
                    kind: "executing_tool".to_string(),
                    detail: Some(format!("execute_command: {cmd}")),
                });

                let output = exec_shell_command(&cmd).await;
                {
                    let mut buf = runtime.terminal_buffer.lock().await;
                    buf.push('\n');
                    buf.push_str(&output);
                    if buf.len() > 200_000 {
                        *buf = buf.chars().rev().take(200_000).collect::<String>().chars().rev().collect();
                    }
                }

                let _ = tx.send(ServerEvent::TerminalOutput {
                    session_id,
                    data: base64::engine::general_purpose::STANDARD.encode(output.as_bytes()),
                });

                let _ = reply.send(output);
            }
            _ => {}
        }
    }
}

async fn spawn_job_heartbeat(
    session_id: Uuid,
    runtime: Arc<SessionRuntime>,
    tx: mpsc::UnboundedSender<ServerEvent>,
) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        let mut jobs_to_send = Vec::new();
        
        {
            let agent_enum = runtime.agent.lock().await;
            if let mylm_core::BuiltAgent::V2(agent) = &*agent_enum {
                let jobs = agent.job_registry.list_active_jobs();
                for job in jobs {
                    jobs_to_send.push(serde_json::to_value(job).unwrap_or(serde_json::Value::Null));
                }
            }
        }

        if !jobs_to_send.is_empty()
            && tx.send(ServerEvent::JobsUpdate {
                session_id,
                jobs: jobs_to_send,
            }).is_err()
        {
            break;
        }
    }
}

async fn run_agent_for_user_message(
    session_id: Uuid,
    runtime: Arc<SessionRuntime>,
    user_text: String,
    tx: mpsc::UnboundedSender<ServerEvent>,
) {
    let _guard = runtime.run_lock.lock().await;
    let message_id = Uuid::new_v4();

    let _ = tx.send(ServerEvent::TypingIndicator {
        session_id,
        is_typing: true,
    });

    let _ = tx.send(ServerEvent::MessageStarted {
        session_id,
        message_id,
        role: "assistant".to_string(),
    });

    {
        let mut agent_enum = runtime.agent.lock().await;
        let res = match &mut *agent_enum {
            mylm_core::BuiltAgent::V1(agent) => {
                agent.history.push(ChatMessage::user(user_text.clone()));
                agent.inject_memory_context().await
            }
            mylm_core::BuiltAgent::V2(agent) => {
                agent.history.push(ChatMessage::user(user_text.clone()));
                agent.inject_memory_context().await
            }
        };
        if let Err(e) = res {
            let _ = tx.send(ServerEvent::Error { 
                code: "memory_error".to_string(), 
                message: e.to_string() 
            });
        }
    }

    let mut last_observation: Option<String> = None;
    let mut seq: u64 = 0;

    loop {
        let _ = tx.send(ServerEvent::Activity {
            session_id,
            kind: "thinking".to_string(),
            detail: None,
        });

        let decision_res = {
            let mut agent_enum = runtime.agent.lock().await;
            match &mut *agent_enum {
                mylm_core::BuiltAgent::V1(agent) => {
                    agent.step(last_observation.take()).await
                        .map(|d| match d {
                            mylm_core::agent::AgentDecision::Message(m, u) => AgentDecision::Message(m, u),
                            mylm_core::agent::AgentDecision::Action { tool, args, kind } => AgentDecision::Action { tool, args, kind },
                            mylm_core::agent::AgentDecision::MalformedAction(e) => AgentDecision::MalformedAction(e),
                            mylm_core::agent::AgentDecision::Error(e) => AgentDecision::Error(e),
                            mylm_core::agent::AgentDecision::Stall { reason, tool_failures } => AgentDecision::Stall { reason, tool_failures },
                        })
                }
                mylm_core::BuiltAgent::V2(agent) => {
                    agent.step(last_observation.take()).await
                        .map(|d| match d {
                            mylm_core::agent::v2::AgentDecision::Message(m, u) => AgentDecision::Message(m, u),
                            mylm_core::agent::v2::AgentDecision::Action { tool, args, kind } => AgentDecision::Action { tool, args, kind },
                            mylm_core::agent::v2::AgentDecision::MalformedAction(e) => AgentDecision::MalformedAction(e),
                            mylm_core::agent::v2::AgentDecision::Error(e) => AgentDecision::Error(e),
                            mylm_core::agent::v2::AgentDecision::Stall { reason, tool_failures } => AgentDecision::Stall { reason, tool_failures },
                        })
                }
            }
        };

        match decision_res {
            Ok(decision) => {
                match decision {
                    AgentDecision::Message(msg, usage) => {
                        let has_pending = {
                            let agent_enum = runtime.agent.lock().await;
                            match &*agent_enum {
                                mylm_core::BuiltAgent::V1(agent) => agent.has_pending_decision(),
                                mylm_core::BuiltAgent::V2(agent) => agent.has_pending_decision(),
                            }
                        };
                        if has_pending {
                            let _ = tx.send(ServerEvent::Activity {
                                session_id,
                                kind: "thought".to_string(),
                                detail: Some(msg),
                            });
                            continue;
                        }

                        // Chunked output
                        for chunk in msg.as_bytes().chunks(64 * 4) {
                            seq += 1;
                            let _ = tx.send(ServerEvent::TokenDelta {
                                session_id,
                                message_id,
                                seq,
                                text: String::from_utf8_lossy(chunk).to_string(),
                            });
                        }

                        let _ = tx.send(ServerEvent::MessageFinal {
                            session_id,
                            message_id,
                            text: msg,
                            usage,
                        });

                        let _ = tx.send(ServerEvent::TypingIndicator {
                            session_id,
                            is_typing: false,
                        });
                        break;
                    }
                    AgentDecision::Action { tool, args, kind } => {
                        let call_id = Uuid::new_v4();
                        let input = serde_json::from_str::<serde_json::Value>(&args)
                            .unwrap_or_else(|_| serde_json::Value::String(args.clone()));
                        
                        let _ = tx.send(ServerEvent::ToolCall {
                            session_id,
                            tool: tool.clone(),
                            call_id,
                            input: input.clone(),
                        });

                        if should_require_approval(&runtime, &tool, kind).await {
                             let approval_id = Uuid::new_v4();
                             let (atx, arx) = oneshot::channel::<bool>();
                             {
                                 let mut pending = runtime.pending_approvals.lock().await;
                                 pending.insert(approval_id, atx);
                             }

                             let _ = tx.send(ServerEvent::ApprovalRequested {
                                 session_id,
                                 approval_id,
                                 kind: "tool".to_string(),
                                 summary: format!("{tool} {args}"),
                                 details: serde_json::json!({ "tool": tool, "args": input }),
                             });

                             match arx.await {
                                 Ok(true) => {
                                     let _ = tx.send(ServerEvent::Activity {
                                         session_id,
                                         kind: "approved".to_string(),
                                         detail: None,
                                     });
                                 }
                                 Ok(false) | Err(_) => {
                                     let _ = tx.send(ServerEvent::ToolResult {
                                         session_id,
                                         tool: tool.clone(),
                                         call_id,
                                         ok: false,
                                         output: serde_json::json!({ "error": "Denied" }),
                                     });
                                     last_observation = Some(format!("Error: User denied the execution of tool '{tool}'."));
                                     continue;
                                 }
                             }
                        }

                        // Execute tool
                        let output = {
                            let agent_enum = runtime.agent.lock().await;
                            match &*agent_enum {
                                mylm_core::BuiltAgent::V1(agent) => {
                                    match agent.tool_registry.execute_tool(&tool, &args).await {
                                        Ok(output) => output,
                                        Err(e) => ToolOutput::Immediate(serde_json::Value::String(format!("Tool Error: {e}"))),
                                    }
                                }
                                mylm_core::BuiltAgent::V2(agent) => {
                                    // V2 tools are in a HashMap
                                    if let Some(t) = agent.tools.get(&tool) {
                                        match t.call(&args).await {
                                            Ok(out) => out,
                                            Err(e) => ToolOutput::Immediate(serde_json::Value::String(format!("Tool Error: {e}"))),
                                        }
                                    } else {
                                        ToolOutput::Immediate(serde_json::Value::String(format!("Error: Tool '{tool}' not found.")))
                                    }
                                }
                            }
                        };

                        let output_str = output.as_string();
                        let _ = tx.send(ServerEvent::ToolResult {
                            session_id,
                            tool: tool.clone(),
                            call_id,
                            ok: !output_str.starts_with("Tool Error:"),
                            output: serde_json::Value::String(output_str.clone()),
                        });

                        last_observation = Some(output_str);
                    }
                    AgentDecision::MalformedAction(err) => {
                        let _ = tx.send(ServerEvent::Error {
                            code: "malformed_action".to_string(),
                            message: err,
                        });
                        break;
                    }
                    AgentDecision::Error(err) => {
                        let _ = tx.send(ServerEvent::Error {
                            code: "agent_error".to_string(),
                            message: err,
                        });
                        break;
                    }
                    AgentDecision::Stall { reason, tool_failures } => {
                        let _ = tx.send(ServerEvent::Error {
                            code: "worker_stalled".to_string(),
                            message: format!(
                                "Worker stalled after {} consecutive tool failures: {}",
                                tool_failures, reason
                            ),
                        });
                        break;
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(ServerEvent::Error {
                    code: "agent_error".to_string(),
                    message: e.to_string(),
                });
                break;
            }
        }
    }
}

async fn exec_shell_command(cmd: &str) -> String {
    #[cfg(windows)]
    let output_res = tokio::process::Command::new("cmd")
        .args(["/C", cmd])
        .output()
        .await;

    #[cfg(not(windows))]
    let output_res = tokio::process::Command::new("bash")
        .args(["-lc", cmd])
        .output()
        .await;

    match output_res {
        Ok(out) => {
            let mut s = String::new();
            if !out.stdout.is_empty() {
                s.push_str(&String::from_utf8_lossy(&out.stdout));
            }
            if !out.stderr.is_empty() {
                if !s.is_empty() {
                    s.push_str("\n--- stderr ---\n");
                }
                s.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            s
        }
        Err(e) => format!("Failed to execute command: {e}"),
    }
}

async fn should_require_approval(runtime: &SessionRuntime, tool: &str, kind: mylm_core::agent::tool::ToolKind) -> bool {
    if runtime.auto_approve {
        return false;
    }
    match kind {
        mylm_core::agent::tool::ToolKind::Terminal => true,
        mylm_core::agent::tool::ToolKind::Web => true,
        mylm_core::agent::tool::ToolKind::Internal => tool == "write_file",
    }
}

async fn calculate_project_stats(path: &std::path::Path) -> Result<mylm_core::protocol::ProjectStats> {
    let mut file_count = 0;
    let mut total_size = 0;
    let mut total_loc = 0;

    let mut stack = vec![path.to_path_buf()];

    while let Some(current_path) = stack.pop() {
        let mut dir = tokio::fs::read_dir(&current_path).await?;
        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist" || name == "build" {
                continue;
            }

            let metadata = entry.metadata().await?;
            if metadata.is_dir() {
                stack.push(path);
            } else {
                file_count += 1;
                total_size += metadata.len();

                // Basic LOC count for common text files
                if let Some(ext) = path.extension() {
                    let ext = ext.to_string_lossy().to_lowercase();
                    if matches!(ext.as_str(), "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "c" | "cpp" | "h" | "hpp" | "go" | "java" | "md" | "json" | "toml" | "yaml" | "yml" | "css" | "html") {
                        if let Ok(content) = tokio::fs::read_to_string(&path).await {
                            total_loc += content.lines().count() as u32;
                        }
                    }
                }
            }
        }
    }

    Ok(mylm_core::protocol::ProjectStats {
        file_count,
        total_size,
        loc: total_loc,
    })
}

async fn crawl_directory(path: &std::path::Path) -> Result<Vec<mylm_core::protocol::FileInfo>> {
    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(path).await?;

    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        
        // Skip hidden files and common ignore patterns
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }

        let is_directory = entry.file_type().await?.is_dir();
        let children = if is_directory {
            // Only crawl 3 levels deep to avoid massive trees
            if path.components().count() < 5 {
                Some(Box::pin(crawl_directory(&path)).await?)
            } else {
                None
            }
        } else {
            None
        };

        entries.push(mylm_core::protocol::FileInfo {
            path: path.to_string_lossy().to_string(),
            name,
            is_directory,
            children,
        });
    }

    Ok(entries)
}

async fn load_workflows() -> (Vec<mylm_core::protocol::Workflow>, Vec<mylm_core::protocol::Stage>) {
    if let Some(config_dir) = mylm_core::config::get_config_dir() {
        let path = config_dir.join("workflows.json");
        if path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(path).await {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                    let workflows = serde_json::from_value(data["workflows"].clone()).unwrap_or_default();
                    let stages = serde_json::from_value(data["stages"].clone()).unwrap_or_default();
                    return (workflows, stages);
                }
            }
        }
    }
    (vec![], vec![])
}

async fn save_workflows(
    workflows: &[mylm_core::protocol::Workflow],
    stages: &[mylm_core::protocol::Stage],
) -> Result<()> {
    if let Some(config_dir) = mylm_core::config::get_config_dir() {
        if !config_dir.exists() {
            tokio::fs::create_dir_all(&config_dir).await?;
        }
        let path = config_dir.join("workflows.json");
        let data = serde_json::json!({
            "workflows": workflows,
            "stages": stages,
        });
        let content = serde_json::to_string_pretty(&data)?;
        tokio::fs::write(path, content).await?;
    }
    Ok(())
}
