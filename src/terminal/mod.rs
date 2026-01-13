pub mod app;
pub mod pty;
pub mod ui;
pub mod session;

use crate::terminal::app::{App, Focus, AppState, TuiEvent};
use mylm_core::memory::graph::MemoryGraph;
use crate::terminal::pty::spawn_pty;
use crate::terminal::ui::render;
use mylm_core::llm::{LlmClient, LlmConfig};
use mylm_core::agent::{Agent, Tool};
use mylm_core::agent::tools::{
    ShellTool, MemoryTool, WebSearchTool, CrawlTool, FileReadTool, FileWriteTool,
    GitStatusTool, GitLogTool, GitDiffTool, StateTool, SystemMonitorTool,
};
use mylm_core::executor::CommandExecutor;
use mylm_core::executor::allowlist::CommandAllowlist;
use mylm_core::executor::safety::SafetyChecker;
use mylm_core::config::Config;
use mylm_core::config::prompt::build_system_prompt;
use mylm_core::context::TerminalContext;
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyModifiers, EnableBracketedPaste, DisableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};
use tokio::sync::mpsc;
use std::sync::atomic::Ordering;

pub async fn run_tui(initial_session: Option<crate::terminal::session::Session>, initial_query: Option<String>, initial_context: Option<TerminalContext>, initial_terminal_context: Option<mylm_core::context::terminal::TerminalContext>) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let size = terminal.size()?;

    // Setup LLM Client (using default endpoint)
    let config = Config::load()?;
    let endpoint_config = config.get_endpoint(None)?;
    // Context limit override:
    // - None => use endpoint/model max
    // - Some(n) => use n
    let effective_context_limit = config
        .context_limit
        .unwrap_or(endpoint_config.max_context_tokens);

    // DEBUG LOG
    // println!(
    //     "DEBUG: config.context_limit={:?}, endpoint.max={}, effective={}",
    //     config.context_limit,
    //     endpoint_config.max_context_tokens,
    //     effective_context_limit
    // );

    let llm_config = LlmConfig::new(
        endpoint_config.provider.parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        endpoint_config.base_url.clone(),
        endpoint_config.model.clone(),
        Some(endpoint_config.api_key.clone()),
    )
    .with_pricing(endpoint_config.input_price_per_1k, endpoint_config.output_price_per_1k)
    .with_context_management(effective_context_limit, endpoint_config.condense_threshold)
    .with_memory(config.memory.clone());
    let llm_client = std::sync::Arc::new(LlmClient::new(llm_config)?);

    // Setup Agent dependencies
    let mut allowlist = CommandAllowlist::new();
    allowlist.apply_config(&config.commands);
    let executor = std::sync::Arc::new(CommandExecutor::new(
        allowlist,
        SafetyChecker::new(),
    ));
    let context = if let Some(ctx) = initial_context {
        ctx
    } else {
        TerminalContext::collect().await
    };
    
    let data_dir = dirs::data_dir()
        .context("Could not find data directory")?
        .join("mylm")
        .join("memory");
    std::fs::create_dir_all(&data_dir)?;
    let store = std::sync::Arc::new(mylm_core::memory::VectorStore::new(data_dir.to_str().unwrap()).await?);
    let categorizer = std::sync::Arc::new(mylm_core::memory::categorizer::MemoryCategorizer::new(llm_client.clone(), store.clone()));
    
    // Initialize state store
    let state_store = std::sync::Arc::new(std::sync::RwLock::new(mylm_core::state::StateStore::new()?));

    // Build hierarchical system prompt
    let prompt_name = config.get_active_profile()
        .map(|p| p.prompt.as_str())
        .unwrap_or("default");
    let system_prompt = build_system_prompt(&context, prompt_name, Some("TUI (Interactive Mode)")).await?;

    // Channel for events
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();

    // Setup Tools
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    let shell_tool = ShellTool::new(executor.clone(), context.clone(), event_tx.clone(), Some(store.clone()), Some(categorizer.clone()), None);
    let memory_tool = MemoryTool::new(store.clone());
    let web_search_tool = WebSearchTool::new(config.web_search.clone(), event_tx.clone());
    let crawl_tool = CrawlTool::new(event_tx.clone());
    let state_tool = StateTool::new(state_store.clone());
    let system_tool = SystemMonitorTool::new();
    
    tools.push(Box::new(shell_tool) as Box<dyn Tool>);
    tools.push(Box::new(memory_tool) as Box<dyn Tool>);
    tools.push(Box::new(web_search_tool) as Box<dyn Tool>);
    tools.push(Box::new(crawl_tool) as Box<dyn Tool>);
    tools.push(Box::new(state_tool) as Box<dyn Tool>);
    tools.push(Box::new(system_tool) as Box<dyn Tool>);
    tools.push(Box::new(FileReadTool) as Box<dyn Tool>);
    tools.push(Box::new(FileWriteTool) as Box<dyn Tool>);
    tools.push(Box::new(GitStatusTool) as Box<dyn Tool>);
    tools.push(Box::new(GitLogTool) as Box<dyn Tool>);
    tools.push(Box::new(GitDiffTool) as Box<dyn Tool>);

    // Create Agent
    let agent = Agent::new_with_iterations(llm_client, tools, system_prompt, config.agent.max_iterations, Some(store.clone()), Some(categorizer.clone()));

    // Setup PTY with context CWD
    let (pty_manager, mut pty_rx) = spawn_pty(context.cwd.clone())?;

    // Create app state
    let mut app = App::new(pty_manager, agent, config);
    
    // Resize app to current terminal size before injecting context
    // This ensures vt100 parser wraps lines correctly for our TUI panes.
    // Terminal pane is 70% width, minus borders.
    let term_width = ((size.width as f32 * 0.7) as u16).saturating_sub(2);
    let term_height = size.height.saturating_sub(4);
    app.resize_pty(term_width, term_height);

    // Inject initial terminal context directly into the parser now.
    // This ensures it is the first thing visible on the alternate screen,
    // appearing above the shell prompt that will arrive shortly from the PTY.
    if let Some(ctx) = initial_terminal_context {
        // Restore raw scrollback if available
        if let Some(scrollback) = ctx.raw_scrollback {
            // Set actual terminal size BEFORE processing scrollback
            // This allows the parser to wrap logical lines naturally
            // Note: resize_pty is already called above, so parser has correct size
            
            // Clear parser state and process scrollback
            app.process_terminal_data(b"\x1c\x1b[2J\x1b[H");
            app.process_terminal_data(scrollback.as_bytes());
            
            // Add a visual divider to separate history from the new session
            let divider = "\r\n\x1b[2m--- mylm session started ---\x1b[0m\r\n\r\n";
            app.process_terminal_data(divider.as_bytes());
        } else {
            // Fallback: If no tmux scrollback, at least show some context info so it's not empty
            let header = format!("\x1b[1;33m[mylm context fallback - tmux session not detected]\x1b[0m\n");
            let cwd_info = format!("\x1b[2mCurrent Directory: {}\x1b[0m\n", ctx.current_dir_str);
            let history_header = format!("\x1b[2mRecent History:\x1b[0m\n");
            
            app.process_terminal_data(header.as_bytes());
            app.process_terminal_data(cwd_info.as_bytes());
            app.process_terminal_data(history_header.as_bytes());
            
            for cmd in ctx.shell_history.iter().take(5) {
                let line = format!("  - {}\n", cmd);
                app.process_terminal_data(line.as_bytes());
            }
            
            let footer = "\n\x1b[2m--- mylm session started ---\x1b[0m\n\n";
            app.process_terminal_data(footer.as_bytes());
        }
    }

    if let Some(session) = initial_session {
        app.chat_history = session.history;
        app.session_id = session.id;
        app.session_monitor.resume_stats(&session.metadata);

        if !session.terminal_history.is_empty() {
            // Restore saved terminal history
            app.process_terminal_data(&session.terminal_history);
            
            // Add a visual divider to separate history from the resumed session
            let divider = "\r\n\x1b[2m--- mylm session resumed ---\x1b[0m\r\n\r\n";
            app.process_terminal_data(divider.as_bytes());
        }
    }

    if let Some(query) = initial_query {
        app.chat_input = query;
        app.cursor_position = app.chat_input.chars().count();
        app.focus = Focus::Chat;
        app.submit_message(event_tx.clone());
    }

    // Spawn PTY listener
    let pty_tx = event_tx.clone();
    tokio::spawn(async move {
        while let Some(data) = pty_rx.recv().await {
            let _ = pty_tx.send(TuiEvent::Pty(data));
        }
    });

    // Spawn Input listener
    let input_tx = event_tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(Duration::from_millis(10)).unwrap_or(false) {
                if let Ok(ev) = event::read() {
                    let _ = input_tx.send(TuiEvent::Input(ev));
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    // Spawn Tick listener
    let tick_tx = event_tx.clone();
    tokio::spawn(async move {
        loop {
            let _ = tick_tx.send(TuiEvent::Tick);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    let res = run_loop(
        &mut terminal,
        &mut app,
        &mut event_rx,
        event_tx,
        executor,
        store,
        state_store,
    ).await;

    // Save session on exit
    let _ = app.save_session();

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    if let Err(_err) = res {
        // Silently exit or log to file in the future
    }

    Ok(())
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    event_rx: &mut mpsc::UnboundedReceiver<TuiEvent>,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    _executor: std::sync::Arc<CommandExecutor>,
    store: std::sync::Arc<mylm_core::memory::VectorStore>,
    state_store: std::sync::Arc<std::sync::RwLock<mylm_core::state::StateStore>>,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| render(f, app))?;

        if let Some(event) = event_rx.recv().await {
            match event {
                TuiEvent::Pty(data) => {
                    let mut data_to_process = data.clone();
                    
                    // 1. Handle Echo Suppression (at start of command)
                    // We look for the "stty -echo;" signature which indicates our wrapper is being echoed.
                    if !app.pending_echo_suppression.is_empty() {
                        let data_str = String::from_utf8_lossy(&data_to_process);
                        if let Some(pos) = data_str.find("stty -echo;") {
                            // Found the wrapper start! Discard everything in this packet up to the next newline.
                            app.pending_echo_suppression.clear();
                            
                            if let Some(clean_cmd) = app.pending_clean_command.take() {
                                // Inject a clean version of the command for the user to see
                                let display = format!("\x1b[32m> {}\x1b[0m\r\n", clean_cmd.trim());
                                app.process_terminal_data(display.as_bytes());
                            }
                            
                            // Search for newline AFTER the start of the wrapper
                            if let Some(nl_pos) = data_str[pos..].find('\r').or_else(|| data_str[pos..].find('\n')) {
                                // Skip the rest of the line that contained the wrapper
                                let skip_len = pos + nl_pos + 1;
                                if skip_len < data_to_process.len() {
                                    data_to_process = data_to_process[skip_len..].to_vec();
                                } else {
                                    data_to_process.clear();
                                }
                            } else {
                                // Wrapper line is not finished in this packet, discard everything after the start for now
                                data_to_process = data_to_process[..pos].to_vec();
                            }
                        } else {
                            // Fallback: check if the packet is part of the expected echo (for slow PTYs)
                            let normalized_data = data_str.replace("\r\n", "\n").replace('\r', "\n");
                            let normalized_expected = app.pending_echo_suppression.replace("\r\n", "\n").replace('\r', "\n");
                            
                            if normalized_expected.starts_with(&normalized_data) {
                                let remaining = normalized_expected[normalized_data.len()..].to_string();
                                app.pending_echo_suppression = remaining;
                                data_to_process.clear();
                            } else {
                                // Mismatch, stop suppression to avoid losing real output
                                app.pending_echo_suppression.clear();
                            }
                        }
                    }

                    // 2. Handle Marker Suppression (at end of command)
                    // We also want to hide the "_MYLM_EOF_..." line from the visible terminal.
                    if !data_to_process.is_empty() {
                        let data_str = String::from_utf8_lossy(&data_to_process);
                        if let Some(pos) = data_str.find("_MYLM_EOF_") {
                            // Found the end marker!
                            // We want to suppress the line containing the marker, but keep anything after it (like a prompt).
                            let mut filtered = data_to_process[..pos].to_vec();
                            
                            // If there was a newline right before it, try to clean that up too to avoid an empty line
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

                            // Search for newline AFTER the marker
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
                        continue;
                    }

                    let screen = app.terminal_parser.screen();
                    let (rows, _cols) = screen.size();
                    
                    // Count potential scrolls (newlines in data while cursor is at bottom)
                    // This is a heuristic but much faster than byte-by-byte
                    let (cursor_row, _) = screen.cursor_position();
                    let is_at_bottom = cursor_row >= rows.saturating_sub(1);
                    
                    if is_at_bottom {
                        let newlines = data.iter().filter(|&&b| b == b'\n').count();
                        if newlines > 0 {
                            // Collect current visible lines as they will be pushed up
                            // We only take the top 'newlines' lines
                            let screen_contents = screen.contents();
                            let lines: Vec<&str> = screen_contents.split('\n').collect();
                            for i in 0..newlines.min(lines.len()) {
                                let line = lines[i].to_string();
                                if !line.trim().is_empty() {
                                    app.terminal_history.push(line);
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

                    if app.capturing_command_output {
                        let text = String::from_utf8_lossy(&data);
                        app.command_output_buffer.push_str(&text);
                        
                        // Use rfind to match the LAST occurrence, just in case (though obfuscation should handle it)
                        if let Some(pos) = app.command_output_buffer.rfind("_MYLM_EOF_") {
                            let marker_line = &app.command_output_buffer[pos..];
                            if let Some(end_pos) = marker_line.find('\r').or_else(|| marker_line.find('\n')) {
                                let full_marker = &marker_line[..end_pos].trim();
                                let exit_code = full_marker.strip_prefix("_MYLM_EOF_").unwrap_or("0");
                                
                                let raw_output = app.command_output_buffer[..pos].to_string();
                                // Strip ANSI escape codes for cleaner LLM input
                                let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
                                let final_output = re.replace_all(&raw_output, "").to_string();

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
                TuiEvent::PtyWrite(data) => {
                    // Send to both the parser for rendering AND the actual PTY for execution
                    app.process_terminal_data(&data);
                    let _ = app.pty_manager.write_all(&data);
                }
                TuiEvent::InternalObservation(data) => {
                    // Send ONLY to the parser for rendering, NOT to the PTY
                    app.process_terminal_data(&data);
                }
                TuiEvent::AgentResponse(response, usage) => {
                    app.add_assistant_message(response, usage);
                    app.status_message = None;
                }
                TuiEvent::AgentResponseFinal(response, usage) => {
                    app.start_streaming_final_answer(response, usage);
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
                    // Show suggestion in Terminal
                    let suggestion = format!("\r\n\x1b[33m[Suggestion]:\x1b[0m AI wants to run: \x1b[1;36m{}\x1b[0m\r\n", cmd);
                    let prompt = "\x1b[33mExecute? (Press Enter in Chat to confirm)\x1b[0m\r\n";
                    app.process_terminal_data(suggestion.as_bytes());
                    app.process_terminal_data(prompt.as_bytes());
                    
                    // Populate Chat Input
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
                TuiEvent::ConfigUpdate(new_config) => {
                    app.config = new_config.clone();
                    
                    if let Ok(endpoint_config) = new_config.get_endpoint(None) {
                        let effective_context_limit = new_config
                            .context_limit
                            .unwrap_or(endpoint_config.max_context_tokens);

                        let llm_config = LlmConfig::new(
                            endpoint_config.provider.parse().unwrap_or(mylm_core::llm::LlmProvider::OpenAiCompatible),
                            endpoint_config.base_url.clone(),
                            endpoint_config.model.clone(),
                            Some(endpoint_config.api_key.clone()),
                        )
                        .with_pricing(endpoint_config.input_price_per_1k, endpoint_config.output_price_per_1k)
                        .with_context_management(effective_context_limit, endpoint_config.condense_threshold)
                        .with_memory(new_config.memory.clone());
                        
                        if let Ok(llm_client) = LlmClient::new(llm_config) {
                            app.input_price = endpoint_config.input_price_per_1k;
                            app.output_price = endpoint_config.output_price_per_1k;
                            app.session_monitor.set_max_context(effective_context_limit as u32);
                            
                            let llm_client = std::sync::Arc::new(llm_client);
                            
                            let prompt_name = new_config.get_active_profile().map(|p| p.prompt.as_str()).unwrap_or("default");
                            let context = TerminalContext::collect().await;
                            if let Ok(system_prompt) = build_system_prompt(&context, prompt_name, Some("TUI (Interactive Mode)")).await {
                                let mut agent = app.agent.lock().await;
                                agent.llm_client = llm_client;
                                agent.system_prompt_prefix = system_prompt;
                                
                                let mut tools: Vec<Box<dyn Tool>> = Vec::new();
                                let agent_session_id = agent.session_id.clone();
                                let categorizer = agent.categorizer.as_ref().cloned();

                                // Re-create executor with updated allowlist from config
                                let mut allowlist = CommandAllowlist::new();
                                allowlist.apply_config(&new_config.commands);
                                let updated_executor = std::sync::Arc::new(CommandExecutor::new(
                                    allowlist,
                                    mylm_core::executor::safety::SafetyChecker::new(),
                                ));

                                let shell_tool = ShellTool::new(updated_executor, context.clone(), event_tx.clone(), Some(store.clone()), categorizer, Some(agent_session_id));
                                let memory_tool = MemoryTool::new(store.clone());
                                let web_search_tool = WebSearchTool::new(new_config.web_search.clone(), event_tx.clone());
                                let crawl_tool = CrawlTool::new(event_tx.clone());
                                let state_tool = StateTool::new(state_store.clone());
                                let system_tool = SystemMonitorTool::new();
                                
                                tools.push(Box::new(shell_tool) as Box<dyn Tool>);
                                tools.push(Box::new(memory_tool) as Box<dyn Tool>);
                                tools.push(Box::new(web_search_tool) as Box<dyn Tool>);
                                tools.push(Box::new(crawl_tool) as Box<dyn Tool>);
                                tools.push(Box::new(state_tool) as Box<dyn Tool>);
                                tools.push(Box::new(system_tool) as Box<dyn Tool>);
                                tools.push(Box::new(FileReadTool) as Box<dyn Tool>);
                                tools.push(Box::new(FileWriteTool) as Box<dyn Tool>);
                                tools.push(Box::new(GitStatusTool) as Box<dyn Tool>);
                                tools.push(Box::new(GitLogTool) as Box<dyn Tool>);
                                tools.push(Box::new(GitDiffTool) as Box<dyn Tool>);

                                let mut tool_map = std::collections::HashMap::new();
                                for tool in tools {
                                    tool_map.insert(tool.name().to_string(), tool);
                                }
                                agent.tools = tool_map;
                            }
                        }
                    }
                    
                    if let Some(path) = mylm_core::config::find_config_file() {
                        let _ = new_config.save(path);
                    }
                }
                TuiEvent::ExecuteTerminalCommand(cmd, tx) => {
                    app.capturing_command_output = true;
                    app.command_output_buffer.clear();
                    app.pending_command_tx = Some(tx);
                    
                    // Wrap command to get exit code and marker
                    // We obfuscate the marker ("_MYLM_" "EOF") to prevent the shell echo from triggering the detector
                    // We also use stty -echo to prevent the command itself from being echoed (if suppression fails),
                    // which keeps the output clean and prevents false positives if the command itself contains the marker.
                    let wrapped_cmd = format!("stty -echo; {{ {}; }} ; echo _MYLM_\"\"EOF_$?; stty echo\r", cmd.trim());
                    
                    // Set up echo suppression
                    // We expect the shell to echo exactly what we type
                    app.pending_echo_suppression = wrapped_cmd.clone();
                    app.pending_clean_command = Some(cmd.clone());
                    
                    let _ = app.pty_manager.write_all(wrapped_cmd.as_bytes());
                }
                TuiEvent::GetTerminalScreen(tx) => {
                    let screen = app.terminal_parser.screen();
                    let mut content = String::new();
                    let (rows, cols) = screen.size();
                    for row in 0..rows {
                        for col in 0..cols {
                            if let Some(cell) = screen.cell(row, col) {
                                content.push_str(&cell.contents());
                            }
                        }
                        content.push('\n');
                    }
                    let _ = tx.send(content);
                }
                TuiEvent::Tick => {
                    app.tick_count += 1;

                    // Incremental rendering of streamed final answer.
                    // We render a small batch per tick to keep UI responsive.
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
                            // Apply usage accounting once at end of stream.
                            let usage = pending.usage.clone();
                            app.pending_stream = None;
                            app.session_monitor.add_usage(&usage, app.input_price, app.output_price);
                            app.set_state(AppState::Idle);
                        }
                    }
                }
                TuiEvent::Input(ev) => {
                    match ev {
                        CrosstermEvent::Key(key) => {
                            // Global Focus Toggle (F2) - Works everywhere
                            if key.code == KeyCode::F(2) {
                                app.toggle_focus();
                                continue;
                            }

                            // Global Memory View Toggle (F3)
                            if key.code == KeyCode::F(3) {
                                app.show_memory_view = !app.show_memory_view;
                                if app.show_memory_view {
                                    let store_clone = store.clone();
                                    let event_tx_clone = event_tx.clone();
                                    // Use last user message or "project" as query
                                    let query = app.chat_history.iter()
                                        .rev()
                                        .find(|m| m.role == mylm_core::llm::chat::MessageRole::User)
                                        .map(|m| m.content.clone())
                                        .unwrap_or_else(|| "project".to_string());
                                    
                                    tokio::spawn(async move {
                                        if let Ok(graph) = MemoryGraph::generate_related_graph(&store_clone, &query, 10).await {
                                            let _ = event_tx_clone.send(TuiEvent::MemoryGraphUpdate(graph));
                                        }
                                    });
                                }
                                continue;
                            }

                            match app.focus {
                                Focus::Terminal => {
                                    match key.code {
                                        KeyCode::PageUp => {
                                            for _ in 0..10 { app.scroll_terminal_up(); }
                                            continue;
                                        }
                                        KeyCode::PageDown => {
                                            for _ in 0..10 { app.scroll_terminal_down(); }
                                            continue;
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
                                }
                                Focus::Chat => {
                                    // Control shortcuts - only active when Chat is focused
                                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                                        match key.code {
                                            KeyCode::Char('y') => {
                                                app.copy_last_ai_response_to_clipboard();
                                                continue;
                                            }
                                            KeyCode::Char('b') => {
                                                app.copy_terminal_buffer_to_clipboard();
                                                continue;
                                            }
                                            KeyCode::Char('v') => {
                                                app.verbose_mode = !app.verbose_mode;
                                                continue;
                                            }
                                            KeyCode::Char('t') => {
                                                app.show_thoughts = !app.show_thoughts;
                                                continue;
                                            }
                                            KeyCode::Char('a') => {
                                                let current = app.auto_approve.load(Ordering::SeqCst);
                                                app.auto_approve.store(!current, Ordering::SeqCst);
                                                continue;
                                            }
                                            KeyCode::Char('c') => {
                                                if app.state != AppState::Idle {
                                                    app.abort_current_task();
                                                    continue;
                                                } else {
                                                    return Ok(());
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    if app.state == AppState::Idle || app.state == AppState::WaitingForUser {
                                        match key.code {
                                            KeyCode::Enter => {
                                                app.submit_message(event_tx.clone());
                                            }
                                            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                                app.trigger_manual_condensation(event_tx.clone());
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
                                                    app.scroll_chat_up();
                                                }
                                            }
                                            KeyCode::Down => {
                                                if app.show_memory_view {
                                                    app.memory_graph_scroll = app.memory_graph_scroll.saturating_add(1);
                                                } else {
                                                    app.scroll_chat_down();
                                                }
                                            }
                                            KeyCode::PageUp => {
                                                for _ in 0..10 { app.scroll_chat_up(); }
                                            }
                                            KeyCode::PageDown => {
                                                for _ in 0..10 { app.scroll_chat_down(); }
                                            }
                                            KeyCode::Esc => app.toggle_focus(),
                                            _ => {}
                                        }
                                    } else {
                                        match key.code {
                                            KeyCode::Esc => {
                                                app.interrupt_flag.store(true, Ordering::SeqCst);
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
                                }
                            }
                        }
                        CrosstermEvent::Resize(width, height) => {
                            terminal.autoresize()?;
                            
                            // Recalculate terminal pane size (70% width, minus borders)
                            let term_width = ((width as f32 * 0.7) as u16).saturating_sub(2);
                            let term_height = height.saturating_sub(4);
                            
                            // Resize PTY and reflow content
                            app.resize_pty(term_width, term_height);
                        }
                        CrosstermEvent::Paste(text) => {
                            match app.focus {
                                Focus::Chat => {
                                    for c in text.chars() {
                                        app.enter_char(c);
                                    }
                                }
                                Focus::Terminal => {
                                    app.handle_terminal_input(text.as_bytes());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
