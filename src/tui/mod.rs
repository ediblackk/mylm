//! TUI Session - Main session UI with integrated terminal, chat, and hub
//!
//! Architecture:
//! - Terminal: vt100 emulator for rendering ANSI output inline
//! - Chat: Scrollable conversation history
//! - Input: Command palette with auto-complete
//! - Hub: Session management, settings, help

use crate::tui::app::App;
use crate::tui::app::TimestampedChatMessage;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

// Main app module - contains all session UI components
pub mod app;

// Setup utilities
pub mod setup;

// Re-export commonly used types from app module for public API
pub use app::{
    AppState,
    spawn_pty,
    agent_setup,
    approval,
    event_loop,
    help,
    pty,
    session,
    session_manager,
    status_tracker,
    terminal_executor,
    types,
    ui,
};

/// Result type for TUI session
#[derive(Debug)]
pub enum TuiResult {
    /// Return to hub (session list)
    ReturnToHub,
    /// Exit the application entirely
    Exit,
}

/// Main entry point for TUI session
pub async fn run_tui_session(
    mut app: App,
    output_rx: mpsc::UnboundedReceiver<mylm_core::agent::OutputEvent>,
    approval_rx: mpsc::Receiver<crate::tui::app::approval::PendingApproval>,
    session_handle: tokio::task::JoinHandle<Result<mylm_core::agent::runtime::SessionResult, mylm_core::agent::runtime::SessionError>>,
) -> io::Result<TuiResult> {
    use crossterm::{
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::{backend::CrosstermBackend, Terminal};

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Store output and approval receivers in app
    app.output_rx = Some(output_rx);
    app.approval_rx = Some(approval_rx);
    
    // Force initial resize to fix terminal display (workaround for tmux/pty initialization issue)
    let size = terminal.size()?;
    let (term_width, term_height) = crate::tui::setup::calculate_terminal_dimensions(
        size.width, size.height, app.chat_width_percent
    );
    app.resize_pty(term_width, term_height);
    
    // Send a clear/redraw to tmux to force it to redraw its status bar correctly
    // This is a workaround for the tmux status bar appearing in the wrong position on startup
    let _ = app.pty_manager.write_all(b"\x0c"); // Send Ctrl+L (form feed/clear) to force redraw

    // Main event loop
    let result = run_event_loop(&mut terminal, &mut app, session_handle).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Save session before returning to hub
    if !app.incognito && !app.return_to_hub {
        let _ = app.save_session(None).await;
    }

    // Determine return result based on app state
    let tui_result = if app.should_quit {
        TuiResult::Exit
    } else {
        TuiResult::ReturnToHub
    };

    result.map(|_| tui_result)
}

/// Handle agent output events
async fn handle_agent_event(
    app: &mut App,
    event: mylm_core::agent::OutputEvent,
) {
    use mylm_core::agent::OutputEvent;
    
    // Update status tracker with the event - this aggregates state from events
    // rather than requiring explicit status declarations from tools
    app.status_tracker.on_event(&event);
    
    // Only log non-chunk events at info level
    if !matches!(event, OutputEvent::ResponseChunk { .. }) {
        mylm_core::info_log!("[AGENT_EVENT] Received event: {:?}", std::mem::discriminant(&event));
    }
    
    match event {
        OutputEvent::Thinking { intent_id } => {
            mylm_core::debug_log!("[AGENT_EVENT] Agent thinking, intent_id={}", intent_id.0);
            app.state = crate::tui::app::AppState::Thinking("Agent is thinking...".to_string());
        }
        
        OutputEvent::ToolExecuting { intent_id, tool, args } => {
            mylm_core::info_log!("[AGENT_EVENT] Tool executing: {} (intent_id={})", tool, intent_id.0);
            app.state = crate::tui::app::AppState::ExecutingTool(format!("{} {}", tool, args));
            app.pending_approval = Some((intent_id.0, tool, args));
        }
        
        OutputEvent::ToolCompleted { result, .. } => {
            mylm_core::info_log!("[AGENT_EVENT] Tool completed, result len={}", result.len());
            app.chat_history.push(TimestampedChatMessage::assistant(format!(
                "🔧 Tool result:\n```\n{}\n```",
                result
            )));
            
            // Add tool result to context manager history
            app.context_manager.add_message("tool", &result);
            
            app.state = crate::tui::app::AppState::Idle;
        }
        
        OutputEvent::ResponseChunk { content } => {
            if content.is_empty() {
                return;
            }
            
            let needs_init = !matches!(app.state, crate::tui::app::AppState::Streaming(_));
            
            if needs_init {
                mylm_core::info_log!("[AGENT_EVENT] Starting streaming response");
                app.state = crate::tui::app::AppState::Streaming("Answering...".to_string());
                // Record start time for generation time tracking
                app.response_start_time = Some(std::time::Instant::now());
                // Initialize with empty assistant message
                app.chat_history.push(TimestampedChatMessage::assistant(String::new()));
            }
            
            // Accumulate response for streaming parse
            app.current_response.push_str(&content);
            
            // Extract partial "t" and "f" values from streaming JSON
            use mylm_core::agent::types::parser::ShortKeyParser;
            let parser = ShortKeyParser::new();
            let (thought, final_answer, _is_complete) = parser.extract_streaming_content(&app.current_response);
            
            // Build display content: show thought if present, then final answer
            let display_content = if !thought.is_empty() && !final_answer.is_empty() {
                format!("💭 {}\n\n{}", thought, final_answer)
            } else if !final_answer.is_empty() {
                final_answer
            } else if !thought.is_empty() {
                format!("💭 {}...", thought)
            } else {
                // Still accumulating, show spinner-like indicator
                "🤔 Thinking ...".to_string()
            };
            
            // Update the last chat message with streaming content
            if let Some(last) = app.chat_history.last_mut() {
                last.message.content = display_content;
            }
        }
        
        OutputEvent::ResponseComplete => {
            mylm_core::info_log!("[AGENT_EVENT] Response complete");
            
            // Calculate and store generation time
            if let Some(start_time) = app.response_start_time.take() {
                let generation_time_ms = start_time.elapsed().as_millis() as u64;
                if let Some(last) = app.chat_history.last_mut() {
                    last.generation_time_ms = Some(generation_time_ms);
                    mylm_core::info_log!("[AGENT_EVENT] Generation time: {}ms", generation_time_ms);
                }
            }
            
            // Get the response content and update context manager
            let response_content = app.chat_history.last()
                .map(|m| m.message.content.clone())
                .unwrap_or_default();
            app.context_manager.on_llm_complete(&response_content);
            let (cached_tokens, max_tokens) = app.context_manager.get_cached_token_usage();
            mylm_core::info_log!("[AGENT_EVENT] Context updated: {}/{} cached tokens", cached_tokens, max_tokens);
            
            // Reset all streaming state
            app.current_response.clear();
            app.stream_thought = None;
            app.stream_in_final = false;
            app.stream_state = None;
            app.stream_lookback.clear();
            app.stream_key_buffer.clear();
            app.stream_escape_next = false;
            app.state = crate::tui::app::AppState::Idle;
        }
        
        OutputEvent::ApprovalRequested { intent_id, tool, args } => {
            mylm_core::info_log!("[AGENT_EVENT] Approval requested for tool: {} (intent_id={})", tool, intent_id.0);
            app.state = crate::tui::app::AppState::AwaitingApproval { tool: tool.clone(), args: args.clone() };
            app.pending_approval = Some((intent_id.0, tool.clone(), args.clone()));
            // Add approval request to chat history (show tool and command)
            // Try to parse args as JSON to extract just the command
            let display_args = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&args) {
                if let Some(cmd) = json.get("command").and_then(|v| v.as_str()) {
                    cmd.to_string()
                } else {
                    args.clone()
                }
            } else {
                args.clone()
            };
            let truncated_args = if display_args.len() > 150 {
                format!("{}...", &display_args[..150])
            } else {
                display_args
            };
            app.chat_history.push(TimestampedChatMessage::assistant(format!(
                "⚠️ Approve running {}?\n```\n{}\n```\n(Y/N)", tool, truncated_args
            )));
        }
        
        OutputEvent::WorkerSpawned { worker_id, objective } => {
            mylm_core::info_log!("[AGENT_EVENT] Worker spawned: {} - {}", worker_id.0, objective);
            app.chat_history.push(TimestampedChatMessage::assistant(format!(
                "🚀 Started worker {}: {}",
                worker_id.0, objective
            )));
        }
        
        OutputEvent::WorkerCompleted { worker_id } => {
            mylm_core::info_log!("[AGENT_EVENT] Worker completed: {}", worker_id.0);
            app.chat_history.push(TimestampedChatMessage::assistant(format!(
                "✅ Worker {} completed",
                worker_id.0
            )));
        }
        
        OutputEvent::WorkerFailed { worker_id, error, is_stall } => {
            mylm_core::error_log!("[AGENT_EVENT] Worker {} failed (stall={}): {}", worker_id.0, is_stall, error);
            let emoji = if is_stall { "⚠️" } else { "❌" };
            let status = if is_stall { "stalled" } else { "failed" };
            app.chat_history.push(TimestampedChatMessage::assistant(format!(
                "{} Worker {} {}: {}",
                emoji, worker_id.0, status, error
            )));
        }
        
        OutputEvent::Error { message } => {
            mylm_core::error_log!("[AGENT_EVENT] Error: {}", message);
            app.state = crate::tui::app::AppState::Error(message.clone());
            app.chat_history.push(TimestampedChatMessage::assistant(format!(
                "❌ Error: {}",
                message
            )));
        }
        
        OutputEvent::Status { message } => {
            mylm_core::debug_log!("[AGENT_EVENT] Status: {}", message);
            if matches!(app.state, crate::tui::app::AppState::Streaming(_)) {
                app.state = crate::tui::app::AppState::Streaming(message);
            }
        }
        
        OutputEvent::Halted { reason } => {
            mylm_core::info_log!("[AGENT_EVENT] Session halted: {}", reason);
            app.state = crate::tui::app::AppState::Idle;
            app.session_active = false;
            app.chat_history.push(TimestampedChatMessage::assistant(format!(
                "Session halted: {}", reason
            )));
            mylm_core::warn_log!("[AGENT_EVENT] Session marked as inactive - restart required for new messages");
        }
        
        OutputEvent::ContextPruned { summary, message_count, tokens_saved, extracted_memories, segment_id } => {
            mylm_core::info_log!(
                "[AGENT_EVENT] Context pruned: {} messages, ~{} tokens saved",
                message_count, tokens_saved
            );
            
            let mem_info = if extracted_memories.is_empty() {
                String::new()
            } else {
                format!("\n💾 {} memories auto-saved", extracted_memories.len())
            };
            
            app.chat_history.push(TimestampedChatMessage::assistant(format!(
                "💾 Context compressed: {} messages summarized (saved ~{} tokens){}\n   \"{}\"\n   Use /pruned to view archive, /restore to recover",
                message_count,
                tokens_saved,
                mem_info,
                summary
            )));
            
            // Store segment ID for potential recovery
            mylm_core::debug_log!("[AGENT_EVENT] Pruned segment ID: {}", segment_id);
        }
    }
}

// Use LoopAction from app::event_loop module
use crate::tui::app::event_loop::LoopAction;

/// Main event loop
async fn run_event_loop<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut App,
    session_handle: tokio::task::JoinHandle<Result<mylm_core::agent::runtime::SessionResult, mylm_core::agent::runtime::SessionError>>,
) -> io::Result<TuiResult> {
    use crate::tui::app::event_loop::handle_key_event;
    use crossterm::event::Event;

    let tick_rate = Duration::from_millis(16); // ~60 FPS
    let mut session_completed = false;

    // Track session completion using the passed handle
    let mut session_handle = session_handle;

    loop {
        // Check if we need to save session
        if app.save_session_request {
            if let Err(e) = app.save_session(None).await {
                eprintln!("Failed to save session: {}", e);
            }
            app.save_session_request = false;
        }

        // Draw UI
        terminal.draw(|f| crate::tui::app::ui::render(f, app))?;
        std::io::Write::flush(&mut std::io::stdout())?;

        // Check if returning to hub
        if app.return_to_hub {
            return Ok(TuiResult::ReturnToHub);
        }

        // Handle events with timeout using tokio::select!
        // Build the select! branches dynamically based on session state
        
        // Helper async block for PTY receiver
        let pty_recv = async {
            if let Some(ref mut rx) = app.pty_rx {
                rx.recv().await
            } else {
                None
            }
        };
        
        // Update animation frame (slower than tick rate for visibility)
        app.status_animation_frame = app.status_animation_frame.wrapping_add(1);
        
        if session_completed {
            // Session done - just handle UI events and PTY
            tokio::select! {
                // Handle crossterm events
                _ = tokio::time::sleep(tick_rate) => {
                    if crossterm::event::poll(Duration::from_secs(0))? {
                        match event::read()? {
                            Event::Key(key) => {
                                match handle_key_event(app, key).await {
                                    LoopAction::Continue => {}
                                    LoopAction::Break => break,
                                }
                            }
                            Event::Mouse(mouse) => {
                                crate::tui::app::event_loop::handle_mouse_event(app, mouse);
                            }
                            Event::Resize(width, height) => {
                                // Calculate new terminal dimensions
                                let (term_width, term_height) = crate::tui::setup::calculate_terminal_dimensions(
                                    width, height, app.chat_width_percent
                                );
                                app.resize_pty(term_width, term_height);
                            }
                            _ => {}
                        }
                    }
                }
                
                // Handle PTY data
                data = pty_recv => {
                    if let Some(data) = data {
                        app.process_pty_data(&data);
                    }
                }
                
                // Handle any remaining agent output events
                event = async {
                    if let Some(ref mut rx) = app.output_rx {
                        rx.recv().await
                    } else {
                        None
                    }
                } => {
                    if let Some(evt) = event {
                        handle_agent_event(app, evt).await;
                    }
                }
            }
        } else {
            // Session still running - handle all events including session completion
            tokio::select! {
                // Handle crossterm events
                _ = tokio::time::sleep(tick_rate) => {
                    if crossterm::event::poll(Duration::from_secs(0))? {
                        match event::read()? {
                            Event::Key(key) => {
                                match handle_key_event(app, key).await {
                                    LoopAction::Continue => {}
                                    LoopAction::Break => break,
                                }
                            }
                            Event::Mouse(mouse) => {
                                crate::tui::app::event_loop::handle_mouse_event(app, mouse);
                            }
                            Event::Resize(_, _) => {}
                            _ => {}
                        }
                    }
                }
                
                // Handle PTY data
                data = pty_recv => {
                    if let Some(data) = data {
                        app.process_pty_data(&data);
                    }
                }
                
                // Handle agent output events
                event = async {
                    if let Some(ref mut rx) = app.output_rx {
                        rx.recv().await
                    } else {
                        None
                    }
                } => {
                    if let Some(evt) = event {
                        handle_agent_event(app, evt).await;
                    }
                }
                
                // Handle approval requests from the capability
                pending_approval = async {
                    if let Some(ref mut rx) = app.approval_rx {
                        rx.recv().await
                    } else {
                        None
                    }
                } => {
                    if let Some(pending) = pending_approval {
                        mylm_core::info_log!(
                            "[TUI] Received approval request for tool: {} (stored pending_approval_with_response)",
                            pending.request.tool
                        );
                        // Store the pending approval with its response channel
                        app.pending_approval_with_response = Some(pending);
                        // The actual display update happens via OutputEvent::ApprovalRequested
                        // which comes through output_rx and sets the UI state
                    } else {
                        mylm_core::warn_log!("[TUI] approval_rx returned None (channel closed)");
                    }
                }
                
                // Handle session completion
                result = &mut session_handle => {
                    session_completed = true;
                    match result {
                        Ok(_) => {
                            app.chat_history.push(TimestampedChatMessage::assistant(
                                "Session completed.".to_string()
                            ));
                        }
                        Err(e) => {
                            app.chat_history.push(TimestampedChatMessage::assistant(format!(
                                "Session panicked: {}", e
                            )));
                        }
                    }
                }
            }
        }
    }

    Ok(if app.should_quit {
        TuiResult::Exit
    } else {
        TuiResult::ReturnToHub
    })
}
