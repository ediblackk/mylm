pub mod app;
pub mod pty;
pub mod ui;
pub mod session;

use crate::terminal::app::{App, Focus, AppState, TuiEvent};
use crate::terminal::pty::spawn_pty;
use crate::terminal::ui::render;
use crate::llm::{LlmClient, LlmConfig, chat::ChatMessage};
use crate::agent::{Agent, Tool};
use crate::agent::tools::{ShellTool, MemoryTool, WebSearchTool, CrawlTool};
use crate::executor::CommandExecutor;
use crate::executor::allowlist::CommandAllowlist;
use crate::executor::safety::SafetyChecker;
use crate::config::Config;
use crate::config::prompt::build_system_prompt;
use crate::context::TerminalContext;
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};
use tokio::sync::mpsc;
use std::sync::atomic::Ordering;

pub async fn run_tui(initial_history: Option<Vec<ChatMessage>>) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Setup LLM Client (using default endpoint)
    let config = Config::load()?;
    let endpoint_config = config.get_endpoint(None)?;
    let llm_config = LlmConfig::new(
        endpoint_config.provider.parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        endpoint_config.base_url.clone(),
        endpoint_config.model.clone(),
        Some(endpoint_config.api_key.clone()),
    )
    .with_pricing(endpoint_config.input_price_per_1k, endpoint_config.output_price_per_1k)
    .with_context_management(endpoint_config.max_context_tokens, endpoint_config.condense_threshold);
    let llm_client = std::sync::Arc::new(LlmClient::new(llm_config)?);

    // Setup Agent dependencies
    let executor = std::sync::Arc::new(CommandExecutor::new(
        CommandAllowlist::new(),
        SafetyChecker::new(),
    ));
    let context = TerminalContext::collect().await;
    
    let data_dir = dirs::data_dir()
        .context("Could not find data directory")?
        .join("mylm")
        .join("memory");
    std::fs::create_dir_all(&data_dir)?;
    let store = std::sync::Arc::new(crate::memory::VectorStore::new(data_dir.to_str().unwrap()).await?);

    // Build hierarchical system prompt
    let prompt_name = config.get_active_profile()
        .map(|p| p.prompt.as_str())
        .unwrap_or("default");
    let system_prompt = build_system_prompt(&context, prompt_name, Some("TUI (Interactive Mode)")).await?;

    // Channel for events
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();

    // Setup Tools
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    let shell_tool = ShellTool::new(executor.clone(), context.clone(), event_tx.clone());
    let memory_tool = MemoryTool::new(store.clone());
    let web_search_tool = WebSearchTool::new(config.web_search.clone());
    let crawl_tool = CrawlTool::new();
    
    tools.push(Box::new(shell_tool) as Box<dyn Tool>);
    tools.push(Box::new(memory_tool) as Box<dyn Tool>);
    tools.push(Box::new(web_search_tool) as Box<dyn Tool>);
    tools.push(Box::new(crawl_tool) as Box<dyn Tool>);

    // Create Agent
    let agent = Agent::new(llm_client, tools, system_prompt);

    // Setup PTY
    let (pty_manager, mut pty_rx) = spawn_pty()?;

    // Create app state
    let mut app = App::new(pty_manager, agent, config);
    if let Some(history) = initial_history {
        app.chat_history = history;
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
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let res = run_loop(
        &mut terminal,
        &mut app,
        &mut event_rx,
        event_tx,
        executor,
        store,
    ).await;

    // Save session on exit
    let _ = app.save_session();

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen
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
    executor: std::sync::Arc<CommandExecutor>,
    store: std::sync::Arc<crate::memory::VectorStore>,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| render(f, app))?;

        if let Some(event) = event_rx.recv().await {
            match event {
                TuiEvent::Pty(data) => {
                    app.terminal_parser.process(&data);
                }
                TuiEvent::PtyWrite(data) => {
                    // Send to both the parser for rendering AND the actual PTY for execution
                    app.terminal_parser.process(&data);
                    let _ = app.pty_manager.write_all(&data);
                }
                TuiEvent::AgentResponse(response, usage) => {
                    app.add_assistant_message(response, usage);
                    app.status_message = None;
                }
                TuiEvent::StatusUpdate(status) => {
                    app.status_message = if status.is_empty() { None } else { Some(status) };
                }
                TuiEvent::CondensedHistory(history) => {
                    app.set_history(history);
                }
                TuiEvent::SuggestCommand(cmd) => {
                    app.chat_input = format!("/exec {}", cmd);
                    app.cursor_position = app.chat_input.chars().count();
                    app.focus = Focus::Chat;
                    app.state = AppState::Idle;
                    app.status_message = None;
                }
                TuiEvent::ConfigUpdate(new_config) => {
                    app.config = new_config.clone();
                    
                    if let Ok(endpoint_config) = new_config.get_endpoint(None) {
                        let llm_config = LlmConfig::new(
                            endpoint_config.provider.parse().unwrap_or(crate::llm::LlmProvider::OpenAiCompatible),
                            endpoint_config.base_url.clone(),
                            endpoint_config.model.clone(),
                            Some(endpoint_config.api_key.clone()),
                        )
                        .with_pricing(endpoint_config.input_price_per_1k, endpoint_config.output_price_per_1k)
                        .with_context_management(endpoint_config.max_context_tokens, endpoint_config.condense_threshold);
                        
                        if let Ok(llm_client) = LlmClient::new(llm_config) {
                            let llm_client = std::sync::Arc::new(llm_client);
                            
                            let prompt_name = new_config.get_active_profile().map(|p| p.prompt.as_str()).unwrap_or("default");
                            let context = TerminalContext::collect().await;
                            if let Ok(system_prompt) = build_system_prompt(&context, prompt_name, Some("TUI (Interactive Mode)")).await {
                                let mut agent = app.agent.lock().await;
                                agent.llm_client = llm_client;
                                agent.system_prompt_prefix = system_prompt;
                                
                                let mut tools: Vec<Box<dyn Tool>> = Vec::new();
                                let shell_tool = ShellTool::new(executor.clone(), context.clone(), event_tx.clone());
                                let memory_tool = MemoryTool::new(store.clone());
                                let web_search_tool = WebSearchTool::new(new_config.web_search.clone());
                                let crawl_tool = CrawlTool::new();
                                
                                tools.push(Box::new(shell_tool) as Box<dyn Tool>);
                                tools.push(Box::new(memory_tool) as Box<dyn Tool>);
                                tools.push(Box::new(web_search_tool) as Box<dyn Tool>);
                                tools.push(Box::new(crawl_tool) as Box<dyn Tool>);

                                let mut tool_map = std::collections::HashMap::new();
                                for tool in tools {
                                    tool_map.insert(tool.name().to_string(), tool);
                                }
                                agent.tools = tool_map;
                            }
                        }
                    }
                    
                    if let Some(path) = crate::config::find_config_file() {
                        let _ = new_config.save(path);
                    }
                }
                TuiEvent::Tick => {}
                TuiEvent::Input(ev) => {
                    match ev {
                        CrosstermEvent::Key(key) => {
                            if key.modifiers.contains(KeyModifiers::CONTROL) {
                                match key.code {
                                    KeyCode::Char('x') => {
                                        app.toggle_focus();
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
                                        app.auto_approve = !app.auto_approve;
                                        continue;
                                    }
                                    KeyCode::Char('c') => {
                                        if app.state == AppState::Processing {
                                            app.abort_current_task();
                                            continue;
                                        } else {
                                            return Ok(());
                                        }
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
                                                match c {
                                                    'l' => vec![12],
                                                    'u' => vec![21],
                                                    'c' => vec![3],
                                                    _ => vec![c as u8],
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
                                    if app.state == AppState::Idle {
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
                                            KeyCode::Up => app.scroll_chat_up(),
                                            KeyCode::Down => app.scroll_chat_down(),
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
                        CrosstermEvent::Resize(_, _) => {
                            terminal.autoresize()?;
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
