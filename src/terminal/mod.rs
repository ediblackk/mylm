pub mod app;
pub mod pty;
pub mod ui;
pub mod session;
pub mod session_manager;
pub mod help;

use crate::terminal::app::{App, Focus, AppState, TuiEvent};
use mylm_core::memory::graph::MemoryGraph;
use crate::terminal::pty::spawn_pty;
use crate::terminal::ui::render;
use mylm_core::llm::{LlmClient, LlmConfig};
use mylm_core::agent::{Agent, Tool};
use mylm_core::agent::tools::{
    ShellTool, MemoryTool, WebSearchTool, CrawlTool, FileReadTool, FileWriteTool,
    GitStatusTool, GitLogTool, GitDiffTool, StateTool, SystemMonitorTool,
    DelegateTool, WaitTool, TerminalSightTool, ScratchpadTool,
};
use mylm_core::executor::CommandExecutor;
use mylm_core::executor::allowlist::CommandAllowlist;
use mylm_core::executor::safety::SafetyChecker;
use mylm_core::config::{Config, ConfigUiExt, build_system_prompt};
use mylm_core::context::TerminalContext;
use anyhow::{Context, Result};
use uuid::Uuid;
use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyModifiers, MouseEventKind, EnableBracketedPaste, DisableBracketedPaste, EnableMouseCapture, DisableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, Mutex};
use std::sync::atomic::Ordering;

pub enum TuiResult {
    Exit,
    ReturnToHub,
}

pub async fn run_tui(initial_session: Option<crate::terminal::session::Session>, initial_query: Option<String>, initial_context: Option<TerminalContext>, initial_terminal_context: Option<mylm_core::context::terminal::TerminalContext>, update_available: bool, incognito: bool) -> Result<TuiResult> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let size = terminal.size()?;

    // Setup LLM Client (using resolved config with profile)
    let config = Config::load()?;
    let resolved = config.resolve_profile();
    
    let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
    let api_key = resolved.api_key.unwrap_or_default();
    
    // V2 uses fixed context limits - no per-endpoint override
    let llm_config = LlmConfig::new(
        format!("{:?}", resolved.provider).to_lowercase().parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        base_url.clone(),
        resolved.model.clone(),
        Some(api_key.clone()),
    )
    .with_memory(config.features.memory.clone());
    let llm_client = std::sync::Arc::new(LlmClient::new(llm_config)?);

    // Setup Agent dependencies
    let allowlist = CommandAllowlist::new();
    let executor = std::sync::Arc::new(CommandExecutor::new(
        allowlist,
        SafetyChecker::new(),
    ));
    let context = if let Some(ctx) = initial_context {
        ctx
    } else {
        TerminalContext::collect().await
    };
    
    let base_data_dir = dirs::data_dir()
        .context("Could not find data directory")?
        .join("mylm");
    
    // Initialize Debug Logger
    mylm_core::agent::logger::init(base_data_dir.clone());
    mylm_core::info_log!("mylm starting up...");

    // Incognito mode: use temporary directory that will be deleted on exit
    let incognito_dir_opt = if incognito {
        let temp_dir = std::env::temp_dir().join(format!("mylm-incognito-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir)?;
        Some(temp_dir)
    } else {
        None
    };

    // Determine store and categorizer directories
    let (store, categorizer, journal) = if incognito {
        // Use temporary directories
        let memory_dir = incognito_dir_opt.as_ref().unwrap().join("memory");
        let journal_path = incognito_dir_opt.as_ref().unwrap().join("journal.md");
        std::fs::create_dir_all(&memory_dir)?;
        let store = std::sync::Arc::new(mylm_core::memory::VectorStore::new(memory_dir.to_str().unwrap()).await?);
        let journal = mylm_core::memory::journal::Journal::with_path(journal_path)?;
        (store, None, journal)
    } else {
        // Use persistent data directory
        let data_dir = base_data_dir.join("memory");
        std::fs::create_dir_all(&data_dir)?;
        let store = std::sync::Arc::new(mylm_core::memory::VectorStore::new(data_dir.to_str().unwrap()).await?);
        let categorizer = std::sync::Arc::new(mylm_core::memory::categorizer::MemoryCategorizer::new(llm_client.clone(), store.clone()));
        let journal = mylm_core::memory::journal::Journal::new()?;
        (store, Some(categorizer), journal)
    };
    
    // Initialize state store
    let state_store = std::sync::Arc::new(std::sync::RwLock::new(mylm_core::state::StateStore::new()?));

    // Initialize Scratchpad (Shared State)
    let scratchpad = Arc::new(RwLock::new(String::new()));

    // Build hierarchical system prompt
    // V2 doesn't have per-profile prompts, use profile name or "default"
    let prompt_name = "default";
    let system_prompt = build_system_prompt(&context, prompt_name, Some("TUI (Interactive Mode)"), None).await?;

    // Channel for events
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();

    // Setup Tools - using Arc for shared ownership
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    
    // We need a job registry for the agent and tools
    let job_registry = mylm_core::agent::v2::jobs::JobRegistry::new();

    // Create a temporary Scribe for tools that need it during initialization
    // The Agent will create its own internal Scribe, but we need one for DelegateTool now.
    let journal = Arc::new(Mutex::new(journal));
    let scribe = Arc::new(mylm_core::memory::scribe::Scribe::new(journal, store.clone(), llm_client.clone()));

    let shell_tool = ShellTool::new(executor.clone(), context.clone(), event_tx.clone(), Some(store.clone()), categorizer.clone(), None, Some(job_registry.clone()));
    let memory_tool = MemoryTool::new(store.clone());
    let web_search_tool = WebSearchTool::new(config.features.web_search.clone(), event_tx.clone());
    let crawl_tool = CrawlTool::new(event_tx.clone());
    let state_tool = StateTool::new(state_store.clone());
    let system_tool = SystemMonitorTool::new();
    let delegate_tool = DelegateTool::new(llm_client.clone(), scribe.clone(), job_registry.clone(), Some(store.clone()), categorizer.clone(), None);
    let wait_tool = WaitTool;
    let terminal_sight_tool = TerminalSightTool::new(event_tx.clone());
    let scratchpad_tool = ScratchpadTool::new(scratchpad.clone());
    
    tools.push(Arc::new(shell_tool));
    tools.push(Arc::new(memory_tool));
    tools.push(Arc::new(web_search_tool));
    tools.push(Arc::new(crawl_tool));
    tools.push(Arc::new(state_tool));
    tools.push(Arc::new(system_tool));
    tools.push(Arc::new(delegate_tool));
    tools.push(Arc::new(wait_tool));
    tools.push(Arc::new(terminal_sight_tool));
    tools.push(Arc::new(scratchpad_tool));
    tools.push(Arc::new(FileReadTool));
    tools.push(Arc::new(FileWriteTool));
    tools.push(Arc::new(GitStatusTool));
    tools.push(Arc::new(GitLogTool));
    tools.push(Arc::new(GitDiffTool));

    // Create Agent
    // Get max_iterations from profile override or use default
    let max_iterations = config.get_active_profile_info()
        .and_then(|p| p.max_iterations)
        .unwrap_or(10);
    
    let mut agent = Agent::new_with_iterations(
        llm_client,
        tools,
        system_prompt,
        max_iterations,
        mylm_core::config::AgentVersion::V2,
        Some(store.clone()),
        categorizer.clone(),
        Some(job_registry.clone()),
        Some(scratchpad.clone()),
        incognito,
    ).await;
    
    // Override the agent's scribe to use our incognito journal (if incognito)
    // This ensures the agent uses the same journal as the tools, which may be incognito
    agent.scribe = Some(scribe.clone());

    // Setup PTY with context CWD
    let (pty_manager, mut pty_rx) = spawn_pty(context.cwd.clone())?;

    // Create app state
    let mut app = App::new(pty_manager, agent, config, scratchpad, job_registry, incognito);
    app.update_available = update_available;
    
    
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
            let header = "\x1b[1;33m[mylm context fallback - tmux session not detected]\x1b[0m\n".to_string();
            let cwd_info = format!("\x1b[2mCurrent Directory: {}\x1b[0m\n", ctx.current_dir_str);
            let history_header = "\x1b[2mRecent History:\x1b[0m\n".to_string();
            
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

        // Restore agent state from session
        {
            let mut agent = app.agent.lock().await;
            agent.session_id = session.agent_session_id.clone();
            agent.history = session.agent_history.clone();
            app.context_manager.set_history(&agent.history);
        }

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
        app.submit_message(event_tx.clone()).await;
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

    let result = run_loop(
        &mut terminal,
        &mut app,
        &mut event_rx,
        event_tx,
        executor,
        store,
        state_store,
        incognito,
    ).await;

    // Save session on exit as fallback (only if not already handled and not incognito)
    if !app.should_quit && !app.return_to_hub && !incognito {
        let _ = app.save_session(None).await;
    }

    // Cleanup incognito temporary directory
    if incognito {
        if let Some(incognito_dir) = incognito_dir_opt {
            let _ = std::fs::remove_dir_all(incognito_dir);
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    event_rx: &mut mpsc::UnboundedReceiver<TuiEvent>,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    _executor: std::sync::Arc<CommandExecutor>,
    store: std::sync::Arc<mylm_core::memory::VectorStore>,
    state_store: std::sync::Arc<std::sync::RwLock<mylm_core::state::StateStore>>,
    incognito: bool,
) -> Result<TuiResult> {
    static ANSI_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = ANSI_RE.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap());
    let mut pending_copy_chord = false;

    loop {
        terminal.draw(|f| render(f, app))?;

        if let Some(event) = event_rx.recv().await {
            match event {
                TuiEvent::Pty(data) => {
                    let mut data_to_process = data.clone();
                    
                    // 1. Handle Echo Suppression (at start of command)
                    // We look for the "stty -echo" signature which indicates our wrapper is being echoed.
                    if !app.pending_echo_suppression.is_empty() {
                        let data_str = String::from_utf8_lossy(&data_to_process);
                        if let Some(pos) = data_str.find("stty -echo") {
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
                TuiEvent::PaCoReProgress { completed, total, current_round, total_rounds } => {
                    app.pacore_progress = Some((completed, total));
                    app.pacore_current_round = Some((current_round, total_rounds));
                }
                TuiEvent::ConfigUpdate(new_config) => {
                    app.config = new_config.clone();
                    
                    // Resolve configuration with active profile
                    let resolved = new_config.resolve_profile();
                    let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
                    let api_key = resolved.api_key.unwrap_or_default();
                    let effective_context_limit = 128000_usize;

                    let llm_config = LlmConfig::new(
                        format!("{:?}", resolved.provider).to_lowercase().parse().unwrap_or(mylm_core::llm::LlmProvider::OpenAiCompatible),
                        base_url.clone(),
                        resolved.model.clone(),
                        Some(api_key.clone()),
                    )
                    .with_memory(new_config.features.memory.clone());
                    
                    if let Ok(llm_client) = LlmClient::new(llm_config) {
                        app.input_price = 0.0; // V2 doesn't track pricing
                        app.output_price = 0.0;
                        app.session_monitor.set_max_context(effective_context_limit as u32);
                        
                        let llm_client = std::sync::Arc::new(llm_client);
                        
                        let prompt_name = "default"; // V2 doesn't have per-profile prompts
                        let context = TerminalContext::collect().await;
                        if let Ok(system_prompt) = build_system_prompt(&context, prompt_name, Some("TUI (Interactive Mode)"), None).await {
                            let mut agent = app.agent.lock().await;
                            agent.llm_client = llm_client.clone();
                            agent.system_prompt_prefix = system_prompt;
                            
                            let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
                            let agent_session_id = agent.session_id.clone();
                            let categorizer = agent.categorizer.as_ref().cloned();
                            let job_registry = agent.job_registry.clone();
                            let scribe = agent.scribe.as_ref().cloned().unwrap(); // Should exist for V2

                            // Re-create executor (V2 doesn't have command config)
                            let allowlist = CommandAllowlist::new();
                            let updated_executor = std::sync::Arc::new(CommandExecutor::new(
                                allowlist,
                                mylm_core::executor::safety::SafetyChecker::new(),
                            ));

                            let shell_tool = ShellTool::new(updated_executor, context.clone(), event_tx.clone(), Some(store.clone()), categorizer.clone(), Some(agent_session_id), Some(job_registry.clone()));
                            let memory_tool = MemoryTool::new(store.clone());
                            let web_search_tool = WebSearchTool::new(new_config.features.web_search.clone(), event_tx.clone());
                            let crawl_tool = CrawlTool::new(event_tx.clone());
                            let state_tool = StateTool::new(state_store.clone());
                            let system_tool = SystemMonitorTool::new();
                            let delegate_tool = DelegateTool::new(llm_client.clone(), scribe.clone(), job_registry.clone(), Some(store.clone()), categorizer.clone(), None);
                            let wait_tool = WaitTool;
                            let terminal_sight_tool = TerminalSightTool::new(event_tx.clone());
                            
                            tools.push(Arc::new(shell_tool));
                            tools.push(Arc::new(memory_tool));
                            tools.push(Arc::new(web_search_tool));
                            tools.push(Arc::new(crawl_tool));
                            tools.push(Arc::new(state_tool));
                            tools.push(Arc::new(system_tool));
                            tools.push(Arc::new(delegate_tool));
                            tools.push(Arc::new(wait_tool));
                            tools.push(Arc::new(terminal_sight_tool));
                            tools.push(Arc::new(FileReadTool));
                            tools.push(Arc::new(FileWriteTool));
                            tools.push(Arc::new(GitStatusTool));
                            tools.push(Arc::new(GitLogTool));
                            tools.push(Arc::new(GitDiffTool));

                            // Register tools in the new tool registry
                            for tool in tools {
                                let _ = agent.tool_registry.register_tool_arc(tool).await;
                            }
                        }
                    }
                    
                    let _ = new_config.save_to_default_location();
                }
                TuiEvent::ExecuteTerminalCommand(cmd, tx) => {
                    mylm_core::info_log!("TUI: Starting terminal command execution: {}", cmd);
                    
                    // If there was a pending command, drop it (sends error to rx)
                    if let Some(old_tx) = app.pending_command_tx.take() {
                        mylm_core::debug_log!("TUI: Cancelling previous pending command tx");
                        let _ = old_tx.send("Error: Command cancelled by new execution".to_string());
                    }

                    app.capturing_command_output = true;
                    app.command_output_buffer.clear();
                    app.pending_command_tx = Some(tx);
                    
                    // Wrap command to get exit code and marker
                    // We obfuscate the marker ("_MYLM_" "EOF") to prevent the shell echo from triggering the detector
                    // We also use stty -echo to prevent the command itself from being echoed (if suppression fails),
                    // which keeps the output clean and prevents false positives if the command itself contains the marker.
                    // We check if stdin is a TTY before calling stty to avoid errors in non-interactive environments.
                    let wrapped_cmd = format!("([ -t 0 ] && stty -echo) 2>/dev/null; {{ {}; }} ; echo '_MYLM_EOF_'$?; ([ -t 0 ] && stty echo) 2>/dev/null\r", cmd.trim());
                    
                    // Set up echo suppression
                    // We expect the shell to echo exactly what we type
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
                            // Global Help View Toggle (F1)
                            if key.code == KeyCode::F(1) {
                                app.show_help_view = !app.show_help_view;
                                // Reset other view states to prevent selection leaking
                                if app.show_help_view {
                                    app.show_memory_view = false;
                                    app.memory_graph_scroll = 0;
                                    app.show_job_detail = false;
                                    app.job_scroll = 0;
                                    app.help_scroll = 0;
                                }
                                continue;
                            }

                            // Help View Scroll Handling (when help is shown)
                            if app.show_help_view {
                                // Max scroll - help content is ~35 lines, limit to 30
                                const MAX_HELP_SCROLL: usize = 30;
                                
                                match key.code {
                                    KeyCode::Up => {
                                        app.help_scroll = app.help_scroll.saturating_sub(1);
                                        continue;
                                    }
                                    KeyCode::Down => {
                                        app.help_scroll = (app.help_scroll + 1).min(MAX_HELP_SCROLL);
                                        continue;
                                    }
                                    KeyCode::PageUp => {
                                        app.help_scroll = app.help_scroll.saturating_sub(10);
                                        continue;
                                    }
                                    KeyCode::PageDown => {
                                        app.help_scroll = (app.help_scroll + 10).min(MAX_HELP_SCROLL);
                                        continue;
                                    }
                                    _ => {}
                                }
                            }

                            // Global Focus Toggle (F2) - Works everywhere
                            if key.code == KeyCode::F(2) {
                                app.toggle_focus();
                                continue;
                            }

                            // Global Memory View Toggle (F3)
                            if key.code == KeyCode::F(3) {
                                app.show_memory_view = !app.show_memory_view;
                                // Reset view states when toggling to prevent selection leaking
                                if app.show_memory_view {
                                    app.show_help_view = false;
                                    app.show_job_detail = false;
                                    app.job_scroll = 0;
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

                            // Global Jobs Panel Toggle (F4 only)
                            if key.code == KeyCode::F(4) {
                                app.show_jobs_panel = !app.show_jobs_panel;
                                // Initialize selection if panel is opened and we have jobs
                                if app.show_jobs_panel {
                                    let jobs = app.job_registry.list_active_jobs();
                                    if !jobs.is_empty() && app.selected_job_index.is_none() {
                                        app.selected_job_index = Some(0);
                                    }
                                } else {
                                    // Clear selection when closing panel to prevent leaking
                                    app.selected_job_index = None;
                                    app.job_scroll = 0;
                                }
                                continue;
                            }

                            // Global Chat Width Adjustment (Ctrl+Shift+Left/Right)
                            if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) {
                                match key.code {
                                    KeyCode::Left => {
                                        app.adjust_chat_width(-5);
                                        continue;
                                    }
                                    KeyCode::Right => {
                                        app.adjust_chat_width(5);
                                        continue;
                                    }
                                    _ => {}
                                }
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
                                    // Job Panel navigation (when visible)
                                    if app.show_jobs_panel {
                                        match key.code {
                                            KeyCode::Char('q') if app.show_job_detail => {
                                                app.show_job_detail = false;
                                                app.job_scroll = 0;
                                                continue;
                                            }
                                            KeyCode::Esc if app.show_job_detail => {
                                                app.show_job_detail = false;
                                                app.job_scroll = 0;
                                                continue;
                                            }
                                            KeyCode::Esc if app.show_jobs_panel => {
                                                app.show_jobs_panel = false;
                                                app.selected_job_index = None;
                                                app.job_scroll = 0;
                                                continue;
                                            }
                                            KeyCode::Up => {
                                                if app.show_job_detail {
                                                    app.job_scroll = app.job_scroll.saturating_sub(1);
                                                } else {
                                                    let jobs = app.job_registry.list_active_jobs();
                                                    if let Some(idx) = app.selected_job_index {
                                                        app.selected_job_index = Some(idx.saturating_sub(1));
                                                    } else if !jobs.is_empty() {
                                                        app.selected_job_index = Some(0);
                                                    }
                                                }
                                                continue;
                                            }
                                            KeyCode::Down => {
                                                if app.show_job_detail {
                                                    app.job_scroll = app.job_scroll.saturating_add(1);
                                                } else {
                                                    let jobs = app.job_registry.list_active_jobs();
                                                    if let Some(idx) = app.selected_job_index {
                                                        if idx + 1 < jobs.len() {
                                                            app.selected_job_index = Some(idx + 1);
                                                        }
                                                    } else if !jobs.is_empty() {
                                                        app.selected_job_index = Some(0);
                                                    }
                                                }
                                                continue;
                                            }
                                            KeyCode::Enter => {
                                                if !app.show_job_detail && app.selected_job_index.is_some() {
                                                    app.show_job_detail = true;
                                                    app.job_scroll = 0;
                                                    continue; // Only consume Enter if we opened job details
                                                }
                                                // Otherwise fall through to chat input handling
                                            }
                                            _ => {}
                                        }
                                    }

                                    // Control shortcuts - only active when Chat is focused
                                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                                        match key.code {
                                            KeyCode::Char('y') => {
                                                app.copy_last_ai_response_to_clipboard();
                                                pending_copy_chord = true;
                                                let _ = terminal.clear();
                                                continue;
                                            }
                                            KeyCode::Char('b') => {
                                                app.copy_terminal_buffer_to_clipboard();
                                                let _ = terminal.clear();
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
                                                // Always toggle auto-approve on Ctrl+A
                                                let current = app.auto_approve.load(Ordering::SeqCst);
                                                app.auto_approve.store(!current, Ordering::SeqCst);
                                                
                                                // If we're in a state with an input field, also move cursor to home
                                                if app.state == AppState::Idle || app.state == AppState::WaitingForUser || app.state == AppState::NamingSession {
                                                    app.move_cursor_home();
                                                }
                                                continue;
                                            }
                                            KeyCode::Char('e') => {
                                                app.move_cursor_end();
                                                continue;
                                            }
                                            KeyCode::Char('k') => {
                                                // Kill line from cursor
                                                let chars: Vec<char> = app.chat_input.chars().collect();
                                                if app.cursor_position < chars.len() {
                                                    app.chat_input = chars.into_iter().take(app.cursor_position).collect();
                                                }
                                                continue;
                                            }
                                            KeyCode::Char('u') => {
                                                // Kill line to cursor
                                                let chars: Vec<char> = app.chat_input.chars().collect();
                                                if app.cursor_position > 0 {
                                                    app.chat_input = chars.into_iter().skip(app.cursor_position).collect();
                                                    app.cursor_position = 0;
                                                }
                                                continue;
                                            }
                                            KeyCode::Char('c') => {
                                                if app.state != AppState::Idle && app.state != AppState::WaitingForUser {
                                                    app.abort_current_task();
                                                    continue;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    // Handle copy chord (Ctrl+Y then U)
                                    if pending_copy_chord {
                                        pending_copy_chord = false;
                                        if key.code == KeyCode::Char('u') || key.code == KeyCode::Char('U') {
                                            app.copy_visible_conversation_to_clipboard();
                                            let _ = terminal.clear();
                                            continue;
                                        }
                                        // If not 'u', fall through to normal handling
                                    }

                                    if app.state == AppState::Idle || app.state == AppState::WaitingForUser {
                                        match key.code {
                                            KeyCode::Enter => {
                                                app.submit_message(event_tx.clone()).await;
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
                                                    // Move cursor up in input if multi-line
                                                    let width = terminal.size().ok().map(|s| s.width).unwrap_or(80);
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
                                                    // Move cursor down in input if multi-line
                                                    let width = terminal.size().ok().map(|s| s.width).unwrap_or(80);
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
                                                app.set_state(AppState::ConfirmExit);
                                            },
                                            _ => {}
                                        }
                                    } else if app.state == AppState::ConfirmExit {
                                        match key.code {
                                            KeyCode::Char('s') | KeyCode::Char('S') => {
                                                app.set_state(AppState::NamingSession);
                                            }
                                            KeyCode::Char('e') | KeyCode::Char('E') => {
                                                app.should_quit = true; // We set this to avoid the fallback save
                                                return Ok(TuiResult::Exit);
                                            }
                                            KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
                                                app.set_state(AppState::Idle);
                                            }
                                            _ => {}
                                        }
                                    } else if app.state == AppState::NamingSession {
                                        match key.code {
                                    KeyCode::Enter => {
                                        let name = if app.exit_name_input.trim().is_empty() {
                                            None
                                        } else {
                                            Some(app.exit_name_input.trim().to_string())
                                        };
                                        let _ = app.save_session(name).await;
                                        app.should_quit = true;
                                        return Ok(TuiResult::Exit);
                                    }
                                            KeyCode::Esc => {
                                                app.set_state(AppState::ConfirmExit);
                                            }
                                            KeyCode::Char(c) => {
                                                app.exit_name_input.push(c);
                                            }
                                            KeyCode::Backspace => {
                                                app.exit_name_input.pop();
                                            }
                                            _ => {}
                                        }
                                    } else {
                                        match key.code {
                                            KeyCode::Esc => {
                                                // Interrupt any active task
                                                if app.state != AppState::Idle && app.state != AppState::WaitingForUser {
                                                     app.interrupt_flag.store(true, Ordering::SeqCst);
                                                     app.abort_current_task();
                                                }
                                                // Save session and return to hub
                                                if !incognito {
                                                    let _ = app.save_session(None).await;
                                                }
                                                app.return_to_hub = true;
                                                return Ok(TuiResult::ReturnToHub);
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
                                    app.enter_string(&text);
                                }
                                Focus::Terminal => {
                                    app.handle_terminal_input(text.as_bytes());
                                }
                            }
                        }
                        CrosstermEvent::Mouse(mouse_event) => {
                            // Mouse support - click to focus, wheel to scroll, drag to select
                            match mouse_event.kind {
                                MouseEventKind::Down(_) => {
                                    // Clear any previous selection and start new one
                                    app.clear_selection();
                                    
                                    // Determine which pane was clicked and set focus
                                    // Terminal is on the left (when visible), Chat on the right
                                    let terminal_width = (terminal.size().map(|s| s.width).unwrap_or(80) as f32 *
                                        if app.show_terminal { 0.7 } else { 0.0 }) as u16;
                                    
                                    if app.show_terminal && mouse_event.column < terminal_width {
                                        app.focus = Focus::Terminal;
                                        app.start_selection(mouse_event.column, mouse_event.row, Focus::Terminal);
                                    } else {
                                        app.focus = Focus::Chat;
                                        app.start_selection(mouse_event.column, mouse_event.row, Focus::Chat);
                                    }
                                }
                                MouseEventKind::Drag(_) => {
                                    // Update selection on drag
                                    app.update_selection(mouse_event.column, mouse_event.row);
                                }
                                MouseEventKind::Up(_) => {
                                    // End selection and copy to clipboard on release
                                    if let Some(selected_text) = app.end_selection() {
                                        if !selected_text.is_empty() {
                                            // Copy to clipboard using internal method
                                            app.copy_text_to_clipboard(selected_text);
                                        }
                                    }
                                }
                                MouseEventKind::ScrollUp => {
                                    match app.focus {
                                        Focus::Terminal => app.scroll_terminal_up(),
                                        Focus::Chat => app.scroll_chat_up(),
                                    }
                                }
                                MouseEventKind::ScrollDown => {
                                    match app.focus {
                                        Focus::Terminal => app.scroll_terminal_down(),
                                        Focus::Chat => app.scroll_chat_down(),
                                    }
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
