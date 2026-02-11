//! Event loop module
//!
//! Handles the main TUI event loop including:
//! - Core event processing (from agent/orchestrator)
//! - TUI event processing (input, PTY, ticks)
//! - Keyboard input handling

use crate::terminal::app::{AppState, Focus, TuiEvent, App};
use crate::terminal::ui::render;
use anyhow::Result;
use crossterm::{
    event::{Event as CrosstermEvent, KeyCode, KeyModifiers, MouseEventKind},
};
use mylm_core::agent::event_bus::CoreEvent;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// Run the main event loop
pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    event_rx: &mut mpsc::UnboundedReceiver<TuiEvent>,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    _executor: Arc<mylm_core::executor::CommandExecutor>,
    store: Arc<mylm_core::memory::VectorStore>,
    state_store: Arc<std::sync::RwLock<mylm_core::state::StateStore>>,
    incognito: bool,
    terminal_delegate: Arc<crate::terminal::delegate_impl::TerminalDelegate>,
    mut core_event_rx: broadcast::Receiver<CoreEvent>,
) -> Result<()> {
    static ANSI_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = ANSI_RE.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap());
    let mut pending_copy_chord = false;

    loop {
        terminal.draw(|f| render(f, app))?;

        // Check for core events (non-blocking)
        if let Ok(event) = core_event_rx.try_recv() {
            let tui_event = convert_core_event(event, app);
            process_tui_event(tui_event, app, &event_tx, terminal, re).await?;
        }

        // Check for TUI events
        if let Some(event) = event_rx.recv().await {
            match event {
                TuiEvent::Input(input_event) => {
                    if !handle_input(
                        input_event, app, terminal, &event_tx, &store,
                        &state_store, incognito, &terminal_delegate, &mut pending_copy_chord,
                    ).await? {
                        return Ok(());
                    }
                }
                other => {
                    process_tui_event(other, app, &event_tx, terminal, re).await?;
                }
            }
        }
    }
}

/// Convert CoreEvent to TuiEvent
fn convert_core_event(event: CoreEvent, app: &mut App) -> TuiEvent {
    match event {
        CoreEvent::StatusUpdate { message } => TuiEvent::StatusUpdate(message),
        CoreEvent::AgentResponse { content, usage } => TuiEvent::AgentResponseFinal(content, usage),
        CoreEvent::AgentThinking { model: _ } => {
            TuiEvent::StatusUpdate(format!("Thinking..."))
        }
        CoreEvent::ToolExecuting { tool, args } => {
            // Add nice AI message describing the action
            let action_msg = match tool.as_str() {
                "web_search" => {
                    let query = extract_search_query(&args);
                    format!("🔍 Searching the web for \"{}\"", query)
                }
                "execute_command" | "shell" | "worker_shell" => {
                    let cmd = extract_command(&args);
                    format!("⚡ Executing: {}", cmd)
                }
                "read_file" | "file_read" => {
                    let path = extract_path(&args);
                    format!("📄 Reading file: {}", path)
                }
                "write_file" | "file_write" => {
                    let path = extract_path(&args);
                    format!("📝 Writing file: {}", path)
                }
                "delegate" => {
                    let task = extract_task_description(&args);
                    format!("👥 Delegating: \"{}\"", task)
                }
                "memory_recall" => "💭 Recalling from memory...".to_string(),
                "memory_record" => "💾 Recording to memory...".to_string(),
                "condense" => "📦 Condensing context...".to_string(),
                _ => format!("🔧 Using {}...", tool),
            };
            app.add_assistant_message_simple(&action_msg);
            TuiEvent::StatusUpdate(format!("Running {}...", tool))
        }
        CoreEvent::WorkerSpawned { .. } => TuiEvent::StatusUpdate(String::new()),
        CoreEvent::WorkerCompleted { .. } => TuiEvent::StatusUpdate(String::new()),
        CoreEvent::WorkerStalled { job_id, reason } => {
            // Update job status in registry so UI shows stalled state
            app.job_registry.stall_job(&job_id, &reason, 0);
            TuiEvent::StatusUpdate(format!("⚠️ Worker {} stalled", &job_id[..8.min(job_id.len())]))
        }
        CoreEvent::WorkerStatusUpdate { job_id, message } => {
            app.job_registry.update_status_message(&job_id, &message);
            TuiEvent::StatusUpdate(String::new())
        }
        CoreEvent::WorkerMetricsUpdate { job_id, prompt_tokens, completion_tokens, total_tokens: _, context_tokens } => {
            // CRITICAL FIX: The metrics have ALREADY been updated by the LLM client.
            // This event is just for UI refresh - DO NOT call update_metrics again
            // or the token counts will be double-counted (causing corruption).
            // The event payload contains the CURRENT accumulated totals, not new deltas.
            let _ = (&job_id, prompt_tokens, completion_tokens, context_tokens); // Use variables for debug if needed
            TuiEvent::StatusUpdate(String::new())
        }
        CoreEvent::PaCoReProgress { round, total } => TuiEvent::PaCoReProgress {
            completed: round,
            total: round,
            current_round: round,
            total_rounds: total,
        },
        CoreEvent::InternalObservation { data } => TuiEvent::PtyWrite(data),
        CoreEvent::SuggestCommand { command } => TuiEvent::SuggestCommand(command),
        CoreEvent::ToolAwaitingApproval { tool, args, approval_id: _ } => {
            let (tx, rx) = tokio::sync::oneshot::channel();
            // Add AI message asking for approval
            let approval_msg = match tool.as_str() {
                "web_search" => {
                    let query = extract_search_query(&args);
                    format!("🔍 May I search the web for \"{}\"? (Y/N)", query)
                }
                "execute_command" | "shell" | "worker_shell" => {
                    let cmd = extract_command(&args);
                    format!("⚡ May I execute: {}? (Y/N)", cmd)
                }
                "read_file" | "file_read" => {
                    let path = extract_path(&args);
                    format!("📄 May I read file: {}? (Y/N)", path)
                }
                "write_file" | "file_write" => {
                    let path = extract_path(&args);
                    format!("📝 May I write file: {}? (Y/N)", path)
                }
                "delegate" => {
                    let task = extract_task_description(&args);
                    format!("👥 May I delegate task: \"{}\" to workers? (Y/N)", task)
                }
                _ => format!("🔧 May I use {}? (Y/N)", tool),
            };
            app.add_assistant_message_simple(&approval_msg);
            // Store the receiver in app state to wait for approval
            app.pending_approval_rx = Some(rx);
            TuiEvent::AwaitingApproval { tool, args, tx }
        }
    }
}

/// Process a TUI event and update app state
async fn process_tui_event(
    event: TuiEvent,
    app: &mut App,
    _event_tx: &mpsc::UnboundedSender<TuiEvent>,
    _terminal: &Terminal<CrosstermBackend<io::Stdout>>,
    ansi_re: &regex::Regex,
) -> Result<()> {
    match event {
        TuiEvent::Pty(data) => {
            process_pty_data(&data, app, ansi_re).await;
        }
        TuiEvent::PtyWrite(data) => {
            app.process_terminal_data(&data);
            let _ = app.pty_manager.write_all(&data);
        }
        TuiEvent::InternalObservation(data) => {
            app.process_terminal_data(&data);
        }
        TuiEvent::AgentResponse(response, usage) => {
            app.add_assistant_message(response, usage);
            app.status_message = None;
        }
        TuiEvent::AgentResponseFinal(response, usage) => {
            app.start_streaming_final_answer(response, usage).await;
            app.status_message = None;
        }
        TuiEvent::StatusUpdate(status) => {
            app.status_message = if status.is_empty() { None } else { Some(status) };
        }
        TuiEvent::ActivityUpdate { summary, detail } => {
            app.push_activity(summary, detail);
        }
        TuiEvent::CondensedHistory(history) => {
            app.set_history(history);
        }
        TuiEvent::SuggestCommand(cmd) => {
            let suggestion = format!("\r\n\x1b[33m[Suggestion]:\x1b[0m AI wants to run: \x1b[1;36m{}\x1b[0m\r\n", cmd);
            let prompt = "\x1b[33mExecute? (Press Enter in Chat to confirm)\x1b[0m\r\n";
            app.process_terminal_data(suggestion.as_bytes());
            app.process_terminal_data(prompt.as_bytes());
            app.chat_input = format!("/exec {}", cmd);
            app.cursor_position = app.chat_input.chars().count();
            app.focus = Focus::Chat;
            app.state = AppState::Idle;
            app.status_message = None;
        }
        TuiEvent::AppStateUpdate(state) => {
            app.set_state(state);
        }
        TuiEvent::MemoryGraphUpdate(graph) => {
            app.memory_graph = graph;
        }
        TuiEvent::PaCoReProgress { completed, total, current_round, total_rounds } => {
            app.pacore_progress = Some((completed, total));
            app.pacore_current_round = Some((current_round, total_rounds));
        }
        TuiEvent::ExecuteTerminalCommand(cmd, tx) => {
            execute_terminal_command(cmd, tx, app).await;
        }
        TuiEvent::GetTerminalScreen(tx) => {
            let screen = app.terminal_parser.screen();
            let mut lines: Vec<String> = Vec::new();
            let (rows, cols) = screen.size();
            for row in 0..rows {
                let mut line = String::new();
                for col in 0..cols {
                    if let Some(cell) = screen.cell(row, col) {
                        line.push_str(&cell.contents());
                    }
                }
                // Trim trailing whitespace from each line
                let trimmed = line.trim_end().to_string();
                lines.push(trimmed);
            }
            // Remove trailing empty lines but preserve internal ones
            while lines.len() > 1 && lines.last().map(|l| l.is_empty()).unwrap_or(false) {
                lines.pop();
            }
            let content = lines.join("\n");
            let _ = tx.send(content);
        }
        TuiEvent::Tick => {
            process_tick(app).await;
        }
        TuiEvent::AwaitingApproval { tool, args, tx } => {
            // Store the sender to be used when user approves/denies
            app.pending_approval_tx = Some(tx);
            app.state = AppState::AwaitingApproval { tool: tool.clone(), args: args.clone() };
            app.status_message = Some(format!("⚠️  Approval required: {} {}", tool, args));
        }
        _ => {}
    }
    Ok(())
}

/// Process PTY data with echo suppression and command capture
async fn process_pty_data(data: &[u8], app: &mut App, ansi_re: &regex::Regex) {
    let mut data_to_process = data.to_vec();

    // Handle echo suppression
    if !app.pending_echo_suppression.is_empty() {
        let data_str = String::from_utf8_lossy(&data_to_process);
        if let Some(pos) = data_str.find("stty -echo") {
            app.pending_echo_suppression.clear();
            
            if let Some(clean_cmd) = app.pending_clean_command.take() {
                let display = format!("\x1b[32m> {}\x1b[0m\r\n", clean_cmd.trim());
                app.process_terminal_data(display.as_bytes());
            }
            
            if let Some(nl_pos) = data_str[pos..].find('\r').or_else(|| data_str[pos..].find('\n')) {
                let skip_len = pos + nl_pos + 1;
                if skip_len < data_to_process.len() {
                    data_to_process = data_to_process[skip_len..].to_vec();
                } else {
                    data_to_process.clear();
                }
            } else {
                data_to_process = data_to_process[..pos].to_vec();
            }
        } else {
            let normalized_data = data_str.replace("\r\n", "\n").replace('\r', "\n");
            let normalized_expected = app.pending_echo_suppression.replace("\r\n", "\n").replace('\r', "\n");
            
            if normalized_expected.starts_with(&normalized_data) {
                let remaining = normalized_expected[normalized_data.len()..].to_string();
                app.pending_echo_suppression = remaining;
                data_to_process.clear();
            } else {
                app.pending_echo_suppression.clear();
            }
        }
    }

    // Handle marker suppression
    if !data_to_process.is_empty() {
        let data_str = String::from_utf8_lossy(&data_to_process);
        if let Some(pos) = data_str.find("_MYLM_EOF_") {
            let mut filtered = data_to_process[..pos].to_vec();
            
            if let Some(last) = filtered.last() {
                if *last == b'\n' || *last == b'\r' {
                    filtered.pop();
                }
            }
            if let Some(last) = filtered.last() {
                if *last == b'\n' || *last == b'\r' {
                    filtered.pop();
                }
            }

            if let Some(nl_pos) = data_str[pos..].find('\r').or_else(|| data_str[pos..].find('\n')) {
                let after_pos = pos + nl_pos + 1;
                if after_pos < data_to_process.len() {
                    filtered.extend_from_slice(&data_to_process[after_pos..]);
                }
            }
            
            data_to_process = filtered;
        }
    }

    if data_to_process.is_empty() && !app.capturing_command_output {
        return;
    }

    // Track terminal history
    let screen = app.terminal_parser.screen();
    let (rows, _cols) = screen.size();
    let (cursor_row, _) = screen.cursor_position();
    let is_at_bottom = cursor_row >= rows.saturating_sub(1);
    
    if is_at_bottom {
        let newlines = data.iter().filter(|&&b| b == b'\n').count();
        if newlines > 0 {
            let screen_contents = screen.contents();
            let lines: Vec<&str> = screen_contents.split('\n').collect();
            for line in lines.iter().take(newlines.min(lines.len())) {
                if !line.trim().is_empty() {
                    app.terminal_history.push(line.to_string());
                }
            }
            
            if app.terminal_history.len() > 1000 {
                let to_remove = app.terminal_history.len() - 1000;
                app.terminal_history.drain(0..to_remove);
            }
        }
    }

    if !data_to_process.is_empty() {
        app.process_terminal_data(&data_to_process);
    }

    // Handle command capture
    if app.capturing_command_output {
        let text = String::from_utf8_lossy(data);
        app.command_output_buffer.push_str(&text);
        
        if let Some(pos) = app.command_output_buffer.rfind("_MYLM_EOF_") {
            let marker_line = &app.command_output_buffer[pos..];
            if let Some(end_pos) = marker_line.find('\r').or_else(|| marker_line.find('\n')) {
                let full_marker = &marker_line[..end_pos].trim();
                let exit_code = full_marker.strip_prefix("_MYLM_EOF_").unwrap_or("0");
                
                let raw_output = app.command_output_buffer[..pos].to_string();
                let final_output = ansi_re.replace_all(&raw_output, "").to_string();

                if let Some(tx) = app.pending_command_tx.take() {
                    let result = if exit_code == "0" {
                        final_output
                    } else {
                        format!("Command failed (exit {}):\n{}", exit_code, final_output)
                    };
                    let _ = tx.send(result);
                }
                
                app.capturing_command_output = false;
                app.command_output_buffer.clear();
            }
        }
    }
}

/// Execute a terminal command with wrapper for exit code capture
async fn execute_terminal_command(cmd: String, tx: tokio::sync::oneshot::Sender<String>, app: &mut App) {
    mylm_core::info_log!("TUI: Starting terminal command execution: {}", cmd);

    if let Some(old_tx) = app.pending_command_tx.take() {
        mylm_core::debug_log!("TUI: Cancelling previous pending command tx");
        let _ = old_tx.send("Error: Command cancelled by new execution".to_string());
    }

    app.capturing_command_output = true;
    app.command_output_buffer.clear();
    app.pending_command_tx = Some(tx);
    
    let wrapped_cmd = format!(
        "([ -t 0 ] && stty -echo) 2>/dev/null; {{ {}; }} ; echo '_MYLM_EOF_'$?; ([ -t 0 ] && stty echo) 2>/dev/null\r",
        cmd.trim()
    );
    
    app.pending_echo_suppression = wrapped_cmd.clone();
    app.pending_clean_command = Some(cmd.clone());
    
    if let Err(e) = app.pty_manager.write_all(wrapped_cmd.as_bytes()) {
        mylm_core::error_log!("TUI: Failed to write to PTY: {}", e);
        if let Some(tx) = app.pending_command_tx.take() {
            let _ = tx.send(format!("Error: Failed to write to PTY: {}", e));
        }
        app.capturing_command_output = false;
    }
}

/// Process tick event for streaming and animations
async fn process_tick(app: &mut App) {
    app.tick_count += 1;

    // Incremental rendering of streamed final answer
    if let Some(pending) = &mut app.pending_stream {
        let batch = 48usize;
        let end = (pending.rendered + batch).min(pending.chars.len());
        if end > pending.rendered {
            let slice: String = pending.chars[pending.rendered..end].iter().collect();
            if let Some(msg) = app.chat_history.get_mut(pending.msg_index) {
                msg.content.push_str(&slice);
            }
            pending.rendered = end;
        }

        if pending.rendered >= pending.chars.len() {
            let usage = pending.usage.clone();
            app.pending_stream = None;
            app.session_monitor.add_usage(&usage, app.input_price, app.output_price);
            app.set_state(AppState::Idle);
            
            // Save session after streaming completes so the full AI response is persisted
            if !app.incognito {
                let _ = app.save_session(None).await;
            }
        }
    }
}

/// Handle keyboard/mouse input. Returns Ok(true) to continue, Ok(false) to exit.
async fn handle_input(
    ev: CrosstermEvent,
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    event_tx: &mpsc::UnboundedSender<TuiEvent>,
    store: &Arc<mylm_core::memory::VectorStore>,
    state_store: &Arc<std::sync::RwLock<mylm_core::state::StateStore>>,
    incognito: bool,
    _terminal_delegate: &Arc<crate::terminal::delegate_impl::TerminalDelegate>,
    pending_copy_chord: &mut bool,
) -> Result<bool> {
    match ev {
        CrosstermEvent::Key(key) => {
            handle_key_event(
                key, app, terminal, event_tx, store, state_store,
                incognito, pending_copy_chord,
            ).await
        }
        CrosstermEvent::Resize(width, height) => {
            terminal.autoresize()?;
            let term_width = ((width as f32 * 0.7) as u16).saturating_sub(2);
            let term_height = height.saturating_sub(4);
            app.resize_pty(term_width, term_height);
            Ok(true)
        }
        CrosstermEvent::Paste(text) => {
            handle_paste(text, app);
            Ok(true)
        }
        CrosstermEvent::Mouse(mouse_event) => {
            handle_mouse(mouse_event, app, terminal).await;
            Ok(true)
        }
        _ => Ok(true),
    }
}

/// Handle key events. Returns Ok(true) to continue, Ok(false) to exit.
async fn handle_key_event(
    key: crossterm::event::KeyEvent,
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    event_tx: &mpsc::UnboundedSender<TuiEvent>,
    store: &Arc<mylm_core::memory::VectorStore>,
    _state_store: &Arc<std::sync::RwLock<mylm_core::state::StateStore>>,
    incognito: bool,
    pending_copy_chord: &mut bool,
) -> Result<bool> {
    // Global Help View Toggle (F1)
    if key.code == KeyCode::F(1) {
        app.show_help_view = !app.show_help_view;
        if app.show_help_view {
            app.show_memory_view = false;
            app.memory_graph_scroll = 0;
            app.show_job_detail = false;
            app.job_scroll = 0;
            app.help_scroll = 0;
        }
        return Ok(true);
    }

    // Help View Scroll Handling
    if app.show_help_view {
        const MAX_HELP_SCROLL: usize = 30;
        match key.code {
            KeyCode::Up => {
                app.help_scroll = app.help_scroll.saturating_sub(1);
                return Ok(true);
            }
            KeyCode::Down => {
                app.help_scroll = (app.help_scroll + 1).min(MAX_HELP_SCROLL);
                return Ok(true);
            }
            KeyCode::PageUp => {
                app.help_scroll = app.help_scroll.saturating_sub(10);
                return Ok(true);
            }
            KeyCode::PageDown => {
                app.help_scroll = (app.help_scroll + 10).min(MAX_HELP_SCROLL);
                return Ok(true);
            }
            _ => {}
        }
    }

    // Global Focus Toggle (F2)
    if key.code == KeyCode::F(2) {
        app.toggle_focus();
        let focus_name = match app.focus {
            Focus::Terminal => "Terminal",
            Focus::Chat => "Chat",
            Focus::Jobs => "Jobs",
        };
        let _ = event_tx.send(TuiEvent::StatusUpdate(
            format!("Focus: {} (F2 to cycle)", focus_name)
        ));
        return Ok(true);
    }

    // Global Memory View Toggle (F3)
    if key.code == KeyCode::F(3) {
        app.show_memory_view = !app.show_memory_view;
        if app.show_memory_view {
            app.show_help_view = false;
            app.show_job_detail = false;
            app.job_scroll = 0;
            let store_clone = store.clone();
            let event_tx_clone = event_tx.clone();
            let query = app.chat_history.iter()
                .rev()
                .find(|m| m.role == mylm_core::llm::chat::MessageRole::User)
                .map(|m| m.content.clone())
                .unwrap_or_else(|| "project".to_string());
            
            tokio::spawn(async move {
                if let Ok(graph) = mylm_core::memory::graph::MemoryGraph::generate_related_graph(&store_clone, &query, 10).await {
                    let _ = event_tx_clone.send(TuiEvent::MemoryGraphUpdate(graph));
                }
            });
        }
        return Ok(true);
    }

    // Global Jobs Panel Toggle (F4)
    if key.code == KeyCode::F(4) {
        app.show_jobs_panel = !app.show_jobs_panel;
        if app.show_jobs_panel {
            let jobs = app.job_registry.list_all_jobs();
            if !jobs.is_empty() && app.selected_job_index.is_none() {
                app.selected_job_index = Some(0);
            }
            app.focus = Focus::Jobs;
            let _ = event_tx.send(TuiEvent::StatusUpdate(
                "Jobs panel opened - use ↑↓ to select, c to cancel, F2 to change focus".to_string()
            ));
        } else {
            app.selected_job_index = None;
            app.job_scroll = 0;
            if app.focus == Focus::Jobs {
                app.focus = Focus::Chat;
            }
            let _ = event_tx.send(TuiEvent::StatusUpdate("Jobs panel closed".to_string()));
        }
        return Ok(true);
    }

    // Global Chat Width Adjustment
    if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) {
        match key.code {
            KeyCode::Left => {
                app.adjust_chat_width(-5);
                return Ok(true);
            }
            KeyCode::Right => {
                app.adjust_chat_width(5);
                return Ok(true);
            }
            _ => {}
        }
    }

    match app.focus {
        Focus::Terminal => handle_terminal_keys(key, app).await,
        Focus::Chat => {
            handle_chat_keys(
                key, app, terminal, event_tx, incognito, pending_copy_chord,
            ).await
        }
        Focus::Jobs => {
            handle_jobs_keys(key, app, event_tx).await;
            Ok(true)
        }
    }
}

/// Handle keys when Terminal is focused
async fn handle_terminal_keys(
    key: crossterm::event::KeyEvent,
    app: &mut App,
) -> Result<bool> {
    match key.code {
        KeyCode::PageUp => {
            for _ in 0..10 { app.scroll_terminal_up(); }
            return Ok(true);
        }
        KeyCode::PageDown => {
            for _ in 0..10 { app.scroll_terminal_down(); }
            return Ok(true);
        }
        _ => {}
    }

    let input = match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                if c.is_ascii_alphabetic() {
                    vec![c.to_ascii_uppercase() as u8 - 64]
                } else {
                    match c {
                        '@' => vec![0],
                        '[' => vec![27],
                        '\\' => vec![28],
                        ']' => vec![29],
                        '^' => vec![30],
                        '_' => vec![31],
                        '?' => vec![127],
                        _ => vec![c as u8],
                    }
                }
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![8],
        KeyCode::Tab => vec![9],
        KeyCode::Esc => vec![27],
        KeyCode::Up => vec![27, b'[', b'A'],
        KeyCode::Down => vec![27, b'[', b'B'],
        KeyCode::Right => vec![27, b'[', b'C'],
        KeyCode::Left => vec![27, b'[', b'D'],
        _ => vec![],
    };
    
    if !input.is_empty() {
        app.handle_terminal_input(&input);
    }
    Ok(true)
}

/// Handle keys when Chat is focused. Returns Ok(true) to continue, Ok(false) to exit.
async fn handle_chat_keys(
    key: crossterm::event::KeyEvent,
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    event_tx: &mpsc::UnboundedSender<TuiEvent>,
    incognito: bool,
    pending_copy_chord: &mut bool,
) -> Result<bool> {
    // Job Panel navigation (when visible)
    if app.show_jobs_panel {
        match key.code {
            KeyCode::Char('q') if app.show_job_detail => {
                app.show_job_detail = false;
                app.job_scroll = 0;
                return Ok(true);
            }
            KeyCode::Esc if app.show_job_detail => {
                app.show_job_detail = false;
                app.job_scroll = 0;
                return Ok(true);
            }
            KeyCode::Esc if app.show_jobs_panel => {
                app.show_jobs_panel = false;
                app.selected_job_index = None;
                app.job_scroll = 0;
                return Ok(true);
            }
            KeyCode::Up => {
                if app.show_job_detail {
                    app.job_scroll = app.job_scroll.saturating_sub(1);
                } else {
                    let jobs = app.job_registry.list_all_jobs();
                    if let Some(idx) = app.selected_job_index {
                        app.selected_job_index = Some(idx.saturating_sub(1));
                    } else if !jobs.is_empty() {
                        app.selected_job_index = Some(0);
                    }
                }
                return Ok(true);
            }
            KeyCode::Down => {
                if app.show_job_detail {
                    app.job_scroll = app.job_scroll.saturating_add(1);
                } else {
                    let jobs = app.job_registry.list_all_jobs();
                    if let Some(idx) = app.selected_job_index {
                        if idx + 1 < jobs.len() {
                            app.selected_job_index = Some(idx + 1);
                        }
                    } else if !jobs.is_empty() {
                        app.selected_job_index = Some(0);
                    }
                }
                return Ok(true);
            }
            KeyCode::Enter => {
                if !app.show_job_detail && app.selected_job_index.is_some() {
                    app.show_job_detail = true;
                    app.job_scroll = 0;
                    return Ok(true);
                }
            }
            _ => {}
        }
    }

    // Control shortcuts
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('y') => {
                app.copy_last_ai_response_to_clipboard();
                *pending_copy_chord = true;
                let _ = terminal.clear();
                return Ok(true);
            }
            KeyCode::Char('b') => {
                app.copy_terminal_buffer_to_clipboard();
                let _ = terminal.clear();
                return Ok(true);
            }
            KeyCode::Char('v') => {
                app.verbose_mode = !app.verbose_mode;
                return Ok(true);
            }
            KeyCode::Char('t') => {
                app.show_thoughts = !app.show_thoughts;
                return Ok(true);
            }
            KeyCode::Char('a') => {
                let current = app.auto_approve.load(Ordering::SeqCst);
                let new_value = !current;
                app.auto_approve.store(new_value, Ordering::SeqCst);
                // Also update the orchestrator's auto_approve setting
                if let Some(ref mut orchestrator) = app.orchestrator {
                    orchestrator.set_auto_approve(new_value);
                }
                if app.state == AppState::Idle || app.state == AppState::WaitingForUser || app.state == AppState::NamingSession {
                    app.move_cursor_home();
                }
                return Ok(true);
            }
            KeyCode::Char('e') => {
                app.move_cursor_end();
                return Ok(true);
            }
            KeyCode::Char('k') => {
                let chars: Vec<char> = app.chat_input.chars().collect();
                if app.cursor_position < chars.len() {
                    app.chat_input = chars.into_iter().take(app.cursor_position).collect();
                }
                return Ok(true);
            }
            KeyCode::Char('u') => {
                let chars: Vec<char> = app.chat_input.chars().collect();
                if app.cursor_position > 0 {
                    app.chat_input = chars.into_iter().skip(app.cursor_position).collect();
                    app.cursor_position = 0;
                }
                return Ok(true);
            }
            KeyCode::Char('c') => {
                if app.state != AppState::Idle && app.state != AppState::WaitingForUser {
                    app.abort_current_task();
                    return Ok(true);
                }
            }
            _ => {}
        }
    }

    // Handle copy chord (Ctrl+Y then U)
    if *pending_copy_chord {
        *pending_copy_chord = false;
        if key.code == KeyCode::Char('u') || key.code == KeyCode::Char('U') {
            app.copy_visible_conversation_to_clipboard();
            let _ = terminal.clear();
            return Ok(true);
        }
    }

    // State-based handling
    if matches!(app.state, AppState::AwaitingApproval { .. }) {
        // Handle approval/denial
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                // Approve the tool execution
                if let Some(tx) = app.pending_approval_tx.take() {
                    let _ = tx.send(true);
                }
                // Also publish to EventBus for chat session loop
                if let Some(ref orchestrator) = app.orchestrator {
                    if let AppState::AwaitingApproval { ref tool, ref args } = app.state {
                        orchestrator.event_bus().publish(CoreEvent::ToolExecuting {
                            tool: tool.clone(),
                            args: args.clone(),
                        });
                    }
                }
                app.state = AppState::Idle;
                app.status_message = Some("✅ Approved".to_string());
                return Ok(true);
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                // Deny the tool execution
                if let Some(tx) = app.pending_approval_tx.take() {
                    let _ = tx.send(false);
                }
                // Also publish rejection to EventBus for chat session loop
                if let Some(ref orchestrator) = app.orchestrator {
                    orchestrator.event_bus().publish(CoreEvent::StatusUpdate {
                        message: "Tool rejected by user".to_string(),
                    });
                }
                app.state = AppState::Idle;
                app.status_message = Some("❌ Denied".to_string());
                return Ok(true);
            }
            _ => {}
        }
        return Ok(true); // Ignore other keys while awaiting approval
    }

    if app.state == AppState::Idle || app.state == AppState::WaitingForUser {
        match key.code {
            KeyCode::Enter => {
                app.submit_message(event_tx.clone()).await;
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let _ = app.trigger_manual_condensation(event_tx.clone());
            }
            KeyCode::Char(c) => {
                app.enter_char(c);
            }
            KeyCode::Backspace => app.delete_char(),
            KeyCode::Delete => app.delete_at_cursor(),
            KeyCode::Left => app.move_cursor_left(),
            KeyCode::Right => app.move_cursor_right(),
            KeyCode::Home => app.move_cursor_home(),
            KeyCode::End => app.move_cursor_end(),
            KeyCode::Up => {
                if app.show_memory_view {
                    app.memory_graph_scroll = app.memory_graph_scroll.saturating_sub(1);
                } else {
                    let width = app.terminal_size.1;
                    let input_width = ((width as f32 * 0.3) as usize).saturating_sub(2);
                    let (x, y) = crate::terminal::ui::calculate_input_cursor_pos(&app.chat_input, app.cursor_position, input_width);
                    if y > 0 {
                        app.cursor_position = crate::terminal::ui::find_idx_from_coords(&app.chat_input, x, y - 1, input_width);
                    } else {
                        app.scroll_chat_up();
                    }
                }
            }
            KeyCode::Down => {
                if app.show_memory_view {
                    app.memory_graph_scroll = app.memory_graph_scroll.saturating_add(1);
                } else {
                    let width = app.terminal_size.1;
                    let input_width = ((width as f32 * 0.3) as usize).saturating_sub(2);
                    let (x, y) = crate::terminal::ui::calculate_input_cursor_pos(&app.chat_input, app.cursor_position, input_width);
                    let wrapped = crate::terminal::ui::wrap_text(&app.chat_input, input_width);
                    if (y as usize) < wrapped.len().saturating_sub(1) {
                        app.cursor_position = crate::terminal::ui::find_idx_from_coords(&app.chat_input, x, y + 1, input_width);
                    } else {
                        app.scroll_chat_down();
                    }
                }
            }
            KeyCode::PageUp => {
                for _ in 0..10 { app.scroll_chat_up(); }
            }
            KeyCode::PageDown => {
                for _ in 0..10 { app.scroll_chat_down(); }
            }
            KeyCode::Esc => {
                // Auto-save and return to hub
                if !incognito {
                    if let Some(pending) = app.pending_stream.take() {
                        let remaining: String = pending.chars[pending.rendered..].iter().collect();
                        if let Some(msg) = app.chat_history.get_mut(pending.msg_index) {
                            msg.content.push_str(&remaining);
                        }
                        app.session_monitor.add_usage(&pending.usage, app.input_price, app.output_price);
                    }
                    let _ = app.save_session(None).await;
                }
                app.return_to_hub = true;
                return Ok(false);
            }
            _ => {}
        }
    } else if app.state == AppState::ConfirmExit || app.state == AppState::NamingSession {
        if key.code == KeyCode::Esc {
            app.set_state(AppState::Idle);
        }
    } else {
        match key.code {
            KeyCode::Esc => {
                if app.state != AppState::Idle && app.state != AppState::WaitingForUser {
                    app.interrupt_flag.store(true, Ordering::SeqCst);
                    app.abort_current_task();
                }
                if !incognito {
                    let _ = app.save_session(None).await;
                }
                app.return_to_hub = true;
                return Ok(false);
            }
            KeyCode::Up => app.scroll_chat_up(),
            KeyCode::Down => app.scroll_chat_down(),
            KeyCode::PageUp => {
                for _ in 0..10 { app.scroll_chat_up(); }
            }
            KeyCode::PageDown => {
                for _ in 0..10 { app.scroll_chat_down(); }
            }
            _ => {}
        }
    }

    Ok(true)
}

/// Handle keys when Jobs panel is focused
async fn handle_jobs_keys(
    key: crossterm::event::KeyEvent,
    app: &mut App,
    event_tx: &mpsc::UnboundedSender<TuiEvent>,
) {
    if !app.show_jobs_panel {
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            if app.show_job_detail {
                app.show_job_detail = false;
                app.job_scroll = 0;
            } else {
                app.show_jobs_panel = false;
                app.selected_job_index = None;
                app.job_scroll = 0;
                app.focus = Focus::Chat;
            }
        }
        KeyCode::Up => {
            if app.show_job_detail {
                app.job_scroll = app.job_scroll.saturating_sub(1);
            } else {
                let jobs = app.job_registry.list_all_jobs();
                if let Some(idx) = app.selected_job_index {
                    app.selected_job_index = Some(idx.saturating_sub(1));
                } else if !jobs.is_empty() {
                    app.selected_job_index = Some(0);
                }
            }
        }
        KeyCode::Down => {
            if app.show_job_detail {
                app.job_scroll = app.job_scroll.saturating_add(1);
            } else {
                let jobs = app.job_registry.list_all_jobs();
                if let Some(idx) = app.selected_job_index {
                    if idx + 1 < jobs.len() {
                        app.selected_job_index = Some(idx + 1);
                    }
                } else if !jobs.is_empty() {
                    app.selected_job_index = Some(0);
                }
            }
        }
        KeyCode::Enter => {
            if !app.show_job_detail && app.selected_job_index.is_some() {
                app.show_job_detail = true;
                app.job_scroll = 0;
            }
        }
        KeyCode::Char('c') => {
            if let Some(idx) = app.selected_job_index {
                let jobs = app.job_registry.list_all_jobs();
                if let Some(job) = jobs.get(idx) {
                    let _ = event_tx.send(TuiEvent::StatusUpdate(
                        format!("Cancelling job {}...", &job.id[..8.min(job.id.len())])
                    ));
                    app.job_registry.cancel_job(&job.id);
                }
            }
        }
        KeyCode::Char('a') => {
            let cancelled = app.job_registry.cancel_all_jobs();
            let _ = event_tx.send(TuiEvent::StatusUpdate(format!("Cancelled {} job(s)", cancelled)));
        }
        _ => {}
    }
}

/// Handle paste events
fn handle_paste(text: String, app: &mut App) {
    match app.focus {
        Focus::Chat => {
            app.enter_string(&text);
        }
        Focus::Terminal => {
            app.handle_terminal_input(text.as_bytes());
        }
        Focus::Jobs => {}
    }
}

/// Handle mouse events
async fn handle_mouse(
    mouse_event: crossterm::event::MouseEvent,
    app: &mut App,
    terminal: &Terminal<CrosstermBackend<io::Stdout>>,
) {
    // Shift+Mouse bypass for native selection
    if mouse_event.modifiers.contains(KeyModifiers::SHIFT) {
        return;
    }

    let term_size = terminal.size().unwrap_or(ratatui::prelude::Size { width: 80, height: 24 });
    let job_panel_height = if app.show_jobs_panel { 6u16 } else { 0u16 };
    let bottom_bar_height = 1u16;
    let jobs_panel_top = term_size.height.saturating_sub(job_panel_height + bottom_bar_height);

    match mouse_event.kind {
        MouseEventKind::Down(btn) => {
            if btn == crossterm::event::MouseButton::Left {
                app.clear_selection();
                
                if app.show_jobs_panel && mouse_event.row >= jobs_panel_top {
                    app.focus = Focus::Jobs;
                    let job_row = mouse_event.row.saturating_sub(jobs_panel_top).saturating_sub(1);
                    if job_row > 0 {
                        let jobs = app.job_registry.list_all_jobs();
                        let job_index = (job_row as usize).saturating_sub(1);
                        if job_index < jobs.len() {
                            app.selected_job_index = Some(job_index);
                        }
                    }
                } else {
                    let terminal_width = (term_size.width as f32 *
                        if app.show_terminal { 1.0 - (app.chat_width_percent as f32 / 100.0) } else { 0.0 }) as u16;
                    
                    if app.show_terminal && mouse_event.column < terminal_width {
                        app.focus = Focus::Terminal;
                        app.start_selection(mouse_event.column, mouse_event.row, Focus::Terminal);
                    } else {
                        app.focus = Focus::Chat;
                        app.start_selection(mouse_event.column, mouse_event.row, Focus::Chat);
                    }
                }
            }
        }
        MouseEventKind::Drag(btn) => {
            if btn == crossterm::event::MouseButton::Left {
                app.update_selection(mouse_event.column, mouse_event.row);
            }
        }
        MouseEventKind::Up(btn) => {
            if btn == crossterm::event::MouseButton::Left {
                if let Some(selected_text) = app.end_selection() {
                    if !selected_text.is_empty() {
                        app.copy_text_to_clipboard(selected_text);
                    }
                }
            }
            if btn == crossterm::event::MouseButton::Right || btn == crossterm::event::MouseButton::Middle {
                let pane = if app.show_terminal {
                    let term_width = (term_size.width as f32 * (1.0 - (app.chat_width_percent as f32 / 100.0))) as u16;
                    if mouse_event.column < term_width {
                        Focus::Terminal
                    } else {
                        Focus::Chat
                    }
                } else {
                    Focus::Chat
                };
                
                if app.is_in_selection(mouse_event.column, mouse_event.row, pane) {
                    if let Some(selected_text) = app.get_selected_text() {
                        if !selected_text.is_empty() {
                            app.copy_text_to_clipboard(selected_text);
                            app.status_message = Some("Copied to clipboard".to_string());
                        }
                    }
                    return;
                }
                
                match pane {
                    Focus::Chat => {
                        app.focus = Focus::Chat;
                        if matches!(app.state, AppState::Idle | AppState::WaitingForUser) {
                            if let Some(clipboard) = &mut app.clipboard {
                                if let Ok(text) = clipboard.get_text() {
                                    app.enter_string(&text);
                                    app.status_message = Some("Pasted from clipboard".to_string());
                                }
                            }
                        }
                    }
                    Focus::Terminal => {
                        app.focus = Focus::Terminal;
                        if let Some(clipboard) = &mut app.clipboard {
                            if let Ok(text) = clipboard.get_text() {
                                app.handle_terminal_input(text.as_bytes());
                                app.status_message = Some("Pasted to terminal".to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        MouseEventKind::ScrollUp => {
            match app.focus {
                Focus::Terminal => app.scroll_terminal_up(),
                Focus::Chat => app.scroll_chat_up(),
                Focus::Jobs => {
                    if app.show_job_detail {
                        app.job_scroll = app.job_scroll.saturating_sub(1);
                    } else {
                        let jobs = app.job_registry.list_all_jobs();
                        if let Some(idx) = app.selected_job_index {
                            app.selected_job_index = Some(idx.saturating_sub(1));
                        } else if !jobs.is_empty() {
                            app.selected_job_index = Some(0);
                        }
                    }
                }
            }
        }
        MouseEventKind::ScrollDown => {
            match app.focus {
                Focus::Terminal => app.scroll_terminal_down(),
                Focus::Chat => app.scroll_chat_down(),
                Focus::Jobs => {
                    if app.show_job_detail {
                        app.job_scroll = app.job_scroll.saturating_add(1);
                    } else {
                        let jobs = app.job_registry.list_all_jobs();
                        if let Some(idx) = app.selected_job_index {
                            if idx + 1 < jobs.len() {
                                app.selected_job_index = Some(idx + 1);
                            }
                        } else if !jobs.is_empty() {
                            app.selected_job_index = Some(0);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

// Helper functions for extracting info from tool args

fn extract_search_query(args: &str) -> String {
    // Try to extract query from JSON {"query": "..."} or just use the args directly
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(args) {
        if let Some(q) = val.get("query").and_then(|v| v.as_str()) {
            return q.to_string();
        }
        if let Some(q) = val.get("args").and_then(|v| v.as_str()) {
            return q.to_string();
        }
    }
    // Fallback: use args directly, truncated if too long
    if args.len() > 60 {
        format!("{}...", &args[..57])
    } else {
        args.to_string()
    }
}

fn extract_command(args: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(args) {
        if let Some(cmd) = val.get("command").and_then(|v| v.as_str()) {
            return if cmd.len() > 60 { format!("{}...", &cmd[..57]) } else { cmd.to_string() };
        }
    }
    if args.len() > 60 {
        format!("{}...", &args[..57])
    } else {
        args.to_string()
    }
}

fn extract_path(args: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(args) {
        if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
            return if path.len() > 60 { format!("{}...", &path[..57]) } else { path.to_string() };
        }
    }
    if args.len() > 60 {
        format!("{}...", &args[..57])
    } else {
        args.to_string()
    }
}

fn extract_task_description(args: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(args) {
        // Try to extract from workers array
        if let Some(workers) = val.get("workers").and_then(|v| v.as_array()) {
            if let Some(first) = workers.first() {
                // Try task field first
                if let Some(task) = first.get("task").and_then(|v| v.as_str()) {
                    return if task.len() > 60 { format!("{}...", &task[..57]) } else { task.to_string() };
                }
                // Fallback to context field
                if let Some(context) = first.get("context").and_then(|v| v.as_str()) {
                    return if context.len() > 60 { format!("{}...", &context[..57]) } else { context.to_string() };
                }
            }
        }
        // Try direct task field
        if let Some(task) = val.get("task").and_then(|v| v.as_str()) {
            return if task.len() > 60 { format!("{}...", &task[..57]) } else { task.to_string() };
        }
        // Try description field
        if let Some(desc) = val.get("description").and_then(|v| v.as_str()) {
            return if desc.len() > 60 { format!("{}...", &desc[..57]) } else { desc.to_string() };
        }
    }
    if args.len() > 60 {
        format!("{}...", &args[..57])
    } else {
        args.to_string()
    }
}
