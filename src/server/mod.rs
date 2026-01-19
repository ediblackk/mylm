use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex, oneshot};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};
use uuid::Uuid;
use anyhow::{Context, Result};
use base64::Engine;

use mylm_core::config::Config;
use mylm_core::agent::core::{Agent, AgentDecision};
use mylm_core::agent::tool::ToolOutput;
use mylm_core::llm::chat::ChatMessage;
use mylm_core::terminal::app::TuiEvent;
use mylm_core::protocol::{ServerEvent, ClientMessage, MessageEnvelope, ServerInfo, Capabilities};

pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub sessions: Arc<Mutex<HashMap<Uuid, Arc<SessionRuntime>>>>,
}

pub struct SessionRuntime {
    pub agent: Arc<Mutex<Agent>>,
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
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    
    let state = Arc::new(AppState {
        config,
        sessions,
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
                if let Err(_) = ws_sender.send(Message::Text(json.into())).await {
                    break;
                }
            }
        }
    });

    // Handle incoming WebSocket messages
    while let Some(Ok(msg)) = ws_receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(envelope) = serde_json::from_str::<MessageEnvelope<ClientMessage>>(text.as_str()) {
                let _ = handle_client_message(envelope.payload, &state, &tx).await;
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
        ClientMessage::CreateSession { .. } => {
            let session_id = Uuid::new_v4();
            let config = state.config.lock().await.clone();
            let auto_approve = config.commands.allow_execution;
            
            let (event_tx, event_rx) = mpsc::unbounded_channel::<TuiEvent>();
            let agent = mylm_core::factory::create_agent_for_session(&config, event_tx.clone()).await?;
            let agent = Arc::new(Mutex::new(agent));

            let runtime = Arc::new(SessionRuntime {
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

            state.sessions.lock().await.insert(session_id, runtime);
            let _ = tx.send(ServerEvent::SessionCreated { session_id });
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
        _ => {}
    }
    Ok(())
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
                let _ = tx.send(ServerEvent::Activity {
                    session_id,
                    kind: "status".to_string(),
                    detail: if status.is_empty() { None } else { Some(status) },
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
                    buf.push_str("\n");
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

async fn run_agent_for_user_message(
    session_id: Uuid,
    runtime: Arc<SessionRuntime>,
    user_text: String,
    tx: mpsc::UnboundedSender<ServerEvent>,
) {
    let _guard = runtime.run_lock.lock().await;
    let message_id = Uuid::new_v4();

    let _ = tx.send(ServerEvent::MessageStarted {
        session_id,
        message_id,
        role: "assistant".to_string(),
    });

    {
        let mut agent = runtime.agent.lock().await;
        agent.history.push(ChatMessage::user(user_text));
        if let Err(e) = agent.inject_memory_context().await {
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
            let mut agent = runtime.agent.lock().await;
            agent.step(last_observation.take()).await
        };

        match decision_res {
            Ok(decision) => {
                match decision {
                    AgentDecision::Message(msg, usage) => {
                        let has_pending = runtime.agent.lock().await.has_pending_decision();
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
                            let agent = runtime.agent.lock().await;
                            match agent.tools.get(&tool) {
                                Some(t) => {
                                    let processed_args = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                                        if let Some(a) = v.get("args").and_then(|a| a.as_str()) {
                                            a.to_string()
                                        } else if let Some(c) = v.get("command").and_then(|c| c.as_str()) {
                                            c.to_string()
                                        } else if let Some(s) = v.as_str() {
                                            s.to_string()
                                        } else {
                                            args.clone()
                                        }
                                    } else {
                                        args.clone()
                                    };
                                    t.call(&processed_args).await
                                        .unwrap_or_else(|e| ToolOutput::Immediate(serde_json::Value::String(format!("Tool Error: {e}"))))
                                }
                                None => ToolOutput::Immediate(serde_json::Value::String(format!("Error: Tool '{tool}' not found."))),
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
