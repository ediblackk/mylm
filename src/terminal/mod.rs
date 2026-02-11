//! Terminal UI module
//!
//! This module provides the TUI (Terminal User Interface) for mylm.
//! It has been refactored into focused submodules:
//!
//! - `app/` - Application state and logic
//! - `ui.rs` - Rendering/UI code
//! - `pty.rs` - PTY (pseudo-terminal) management
//! - `session.rs` - Session persistence
//! - `session_manager.rs` - Session management UI
//! - `help.rs` - Help content
//! - `delegate_impl.rs` - Terminal delegate for core tools
//! - `setup.rs` - Terminal initialization and cleanup
//! - `agent_setup.rs` - Agent initialization logic
//! - `event_loop.rs` - Main event loop handling

pub mod app;
pub mod pty;
pub mod ui;
pub mod session;
pub mod session_manager;
pub mod help;
pub mod delegate_impl;
pub mod setup;
pub mod agent_setup;
pub mod event_loop;

use crate::terminal::app::{App, TuiEvent};
use crate::terminal::pty::spawn_pty;
use crate::terminal::setup::{init_terminal, calculate_terminal_dimensions};
use crate::terminal::agent_setup::initialize_agent;
use crate::terminal::event_loop::run as run_event_loop;

use mylm_core::config::Config;
use mylm_core::context::TerminalContext;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;
use uuid::Uuid;

/// Run the TUI with optional initial session and query
pub async fn run_tui(
    initial_session: Option<crate::terminal::session::Session>,
    initial_query: Option<String>,
    initial_context: Option<TerminalContext>,
    initial_terminal_context: Option<mylm_core::context::terminal::TerminalContext>,
    update_available: bool,
    incognito: bool,
) -> Result<()> {
    // Initialize terminal
    let (mut terminal, _guard) = init_terminal()?;
    let size = terminal.size()?;

    // Load configuration
    let config = Config::load()?;

    // Collect or use provided context
    let context = if let Some(ctx) = initial_context {
        ctx
    } else {
        TerminalContext::collect().await
    };

    // Setup data directory
    let base_data_dir = dirs::data_dir()
        .context("Could not find data directory")?
        .join("mylm");

    // Initialize logging
    mylm_core::agent::logger::init(base_data_dir.clone());
    mylm_core::info_log!("mylm starting up...");

    // Setup incognito directory if needed
    let incognito_dir_opt: Option<std::path::PathBuf> = if incognito {
        let temp_dir = std::env::temp_dir().join(format!("mylm-incognito-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir)?;
        Some(temp_dir)
    } else {
        None
    };

    // Create event channel
    let (event_tx, event_rx) = mpsc::unbounded_channel::<TuiEvent>();

    // Setup PTY
    let (pty_manager, pty_rx) = spawn_pty(context.cwd.clone())?;
    let pty_manager = Arc::new(pty_manager);

    // Initialize agent components
    let agent_components = initialize_agent(
        &config,
        context.clone(),
        &base_data_dir,
        incognito,
        incognito_dir_opt.as_deref(),
        pty_manager.clone(),
        event_tx.clone(),
    ).await?;

    // Create app state
    let mut app = App::new_with_orchestrator(
        (*pty_manager).clone(),
        agent_components.agent_wrapper,
        config.clone(),
        agent_components.scratchpad,
        crate::get_job_registry().clone(),
        incognito,
        agent_components.orchestrator,
        agent_components.terminal_delegate.clone(),
        agent_components.event_bus.clone(),
    ).await;
    app.update_available = update_available;

    // Resize PTY to initial dimensions
    let (term_width, term_height) = calculate_terminal_dimensions(size.width, size.height, app.chat_width_percent);
    app.resize_pty(term_width, term_height);

    // Inject initial terminal context
    inject_terminal_context(&mut app, initial_terminal_context);

    // Restore session if provided
    if let Some(session) = initial_session {
        restore_session(&mut app, session).await;
    }

    // Submit initial query if provided
    if let Some(query) = initial_query {
        app.chat_input = query;
        app.cursor_position = app.chat_input.chars().count();
        app.focus = crate::terminal::app::Focus::Chat;
        app.submit_message(event_tx.clone()).await;
    }

    // Spawn background tasks
    let pty_handle = spawn_pty_listener(pty_rx, event_tx.clone());
    let input_handle = spawn_input_listener(event_tx.clone());
    let tick_handle = spawn_tick_generator(event_tx.clone());

    // Run main event loop
    let core_event_rx = agent_components.event_bus.subscribe();
    let mut event_rx = event_rx;
    let result = run_event_loop(
        &mut terminal,
        &mut app,
        &mut event_rx,
        event_tx.clone(),
        agent_components.executor,
        agent_components.store,
        agent_components.state_store,
        incognito,
        agent_components.terminal_delegate,
        core_event_rx,
    ).await;

    // Cleanup on exit
    cleanup_on_exit(
        &mut app,
        incognito,
        incognito_dir_opt,
        pty_handle,
        input_handle,
        tick_handle,
    ).await;

    result
}

/// Inject initial terminal context into the parser
fn inject_terminal_context(app: &mut App, ctx: Option<mylm_core::context::terminal::TerminalContext>) {
    if let Some(ctx) = ctx {
        if let Some(scrollback) = ctx.raw_scrollback {
            app.process_terminal_data(b"\x1c\x1b[2J\x1b[H");
            app.process_terminal_data(scrollback.as_bytes());
            let divider = "\r\n\x1b[2m--- mylm session started ---\x1b[0m\r\n\r\n";
            app.process_terminal_data(divider.as_bytes());
        } else {
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
}

/// Restore session data into the app
async fn restore_session(app: &mut App, session: crate::terminal::session::Session) {
    app.chat_history = session.history;
    app.session_id = session.id;
    let max_ctx = app.config.endpoint.max_context_tokens.unwrap_or(128000) as u32;
    app.session_monitor.resume_stats(&session.metadata, max_ctx);

    app.agent.set_session_id(session.agent_session_id.clone()).await;
    app.agent.set_history(session.agent_history.clone()).await;
    let agent_history = app.agent.history().await;
    app.context_manager.set_history(&agent_history);

    if !session.terminal_history.is_empty() {
        app.process_terminal_data(&session.terminal_history);
        let divider = "\r\n\x1b[2m--- mylm session resumed ---\x1b[0m\r\n\r\n";
        app.process_terminal_data(divider.as_bytes());
    }
}

/// Spawn PTY listener task
fn spawn_pty_listener(
    mut pty_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(data) = pty_rx.recv().await {
            let _ = event_tx.send(TuiEvent::Pty(data));
        }
    })
}

/// Spawn input listener task
fn spawn_input_listener(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if crossterm::event::poll(Duration::from_millis(10)).unwrap_or(false) {
                if let Ok(ev) = crossterm::event::read() {
                    let _ = event_tx.send(TuiEvent::Input(ev));
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
}

/// Spawn tick generator task
fn spawn_tick_generator(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let _ = event_tx.send(TuiEvent::Tick);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
}

/// Cleanup resources on exit
async fn cleanup_on_exit(
    app: &mut App,
    incognito: bool,
    incognito_dir_opt: Option<std::path::PathBuf>,
    pty_handle: tokio::task::JoinHandle<()>,
    input_handle: tokio::task::JoinHandle<()>,
    tick_handle: tokio::task::JoinHandle<()>,
) {
    // Save session if needed
    if !app.should_quit && !app.return_to_hub && !incognito {
        let _ = app.save_session(None).await;
    }

    // Cleanup incognito directory
    if incognito {
        if let Some(incognito_dir) = incognito_dir_opt {
            let _ = std::fs::remove_dir_all(incognito_dir);
        }
    }

    // Abort background tasks
    pty_handle.abort();
    input_handle.abort();
    tick_handle.abort();

    // Small delay for terminal state restoration
    tokio::time::sleep(Duration::from_millis(50)).await;
}
