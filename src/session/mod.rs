//! Session module - TUI session composition and execution
//!
//! This module handles the wiring of PTY, approval, agent session factory,
//! and the broadcast-to-mpsc bridge needed for the TUI.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

use mylm_core::agent::runtime::Session;
use mylm_core::agent::factory::AgentSessionFactory;
use mylm_core::agent::runtime::core::ApprovalCapability;
use mylm_core::agent::runtime::core::terminal::TerminalExecutor;
use mylm_core::config::Config;

use crate::tui::app;
use crate::tui::TuiResult;

/// Create AgentSessionFactory from config
///
/// Optionally provide custom terminal executor and approval capability.
/// If approval is None, auto-approve is used (suitable for non-interactive use).
fn create_session_factory(
    config: &Config,
    terminal: Option<Arc<dyn TerminalExecutor>>,
    approval: Option<Arc<dyn ApprovalCapability>>,
) -> AgentSessionFactory {
    let mut factory = AgentSessionFactory::new(config.clone());

    if let Some(terminal) = terminal {
        factory = factory.with_terminal(terminal);
    }

    if let Some(approval) = approval {
        factory = factory.with_approval(approval);
    }

    factory
}

/// Run TUI session with all components wired together
pub async fn run_tui_with_session(config: &Config) -> Result<TuiResult> {
    mylm_core::info_log!("[SESSION] Starting TUI session (profile: {})", config.active_profile);

    // Create PTY manager FIRST (needed for terminal executor)
    let cwd = std::env::current_dir().ok();
    let (pty_manager, pty_rx) = match cwd {
        Some(path) => match app::spawn_pty(Some(path)) {
            Ok((pm, rx)) => (pm, rx),
            Err(e) => {
                mylm_core::error_log!("[SESSION] Failed to spawn PTY: {}", e);
                eprintln!("❌ Failed to spawn PTY: {}", e);
                return Ok(TuiResult::ReturnToHub);
            }
        },
        None => {
            mylm_core::error_log!("[SESSION] Could not determine current directory");
            eprintln!("❌ Could not determine current directory");
            return Ok(TuiResult::ReturnToHub);
        }
    };

    // Create job registry
    let job_registry = app::JobRegistry::new();

    // Create approval capability for interactive tool approval
    let approval_capability = app::TuiApprovalCapability::new();
    let auto_approve = approval_capability.auto_approve();
    let approval_arc = Arc::new(approval_capability);
    let approval_handle = app::ApprovalHandle::new(Arc::clone(&approval_arc));

    // Create App with PTY (but no session yet)
    let mut app = crate::tui::app::App::new(
        pty_manager,
        config.clone(),
        job_registry,
        false, // not incognito
        Some(auto_approve), // Share auto_approve with approval capability
    );
    app.pty_rx = Some(pty_rx);
    app.approval_handle = Some(approval_handle);

    // Create terminal executor that uses the App's screen
    let _app_weak = Arc::downgrade(&std::sync::Arc::new(std::sync::Mutex::new(())));
    // Placeholder - will use actual app reference
    // For now, use default terminal executor (commands run via std::process::Command)
    // The terminal context is collected separately via app.get_screen()

    // Create agent session factory with approval
    let factory = create_session_factory(
        config,
        None, // Use default terminal executor for now
        Some(approval_arc),
    );

    // Create session for default profile
    let mut session = match factory.create_default_session().await {
        Ok(s) => s,
        Err(e) => {
            mylm_core::error_log!("[SESSION] Failed to create agent session: {}", e);
            eprintln!("❌ Failed to create agent session: {}", e);
            return Ok(TuiResult::ReturnToHub);
        }
    };

    // Get input sender and subscribe to output events
    let input_tx = session.input_sender();
    let mut broadcast_rx = session.subscribe_output();

    // Create mpsc channel to bridge broadcast to unbounded for TUI
    let (output_tx, output_rx) = mpsc::unbounded_channel::<
        mylm_core::agent::OutputEvent,
    >();

    // Spawn bridge task to forward events from broadcast to mpsc
    tokio::spawn(async move {
        let mut event_count = 0u64;
        let mut chunk_count = 0u64;
        loop {
            match broadcast_rx.recv().await {
                Ok(event) => {
                    event_count += 1;
                    use mylm_core::agent::OutputEvent;
                    if matches!(event, OutputEvent::ResponseChunk { .. }) {
                        chunk_count += 1;
                    } else {
                        mylm_core::debug_log!(
                            "[BRIDGE] Forwarding event #{} (chunks={}): {:?}",
                            event_count,
                            chunk_count,
                            std::mem::discriminant(&event)
                        );
                        chunk_count = 0; // Reset after reporting
                    }
                    if output_tx.send(event).is_err() {
                        mylm_core::warn_log!("[BRIDGE] Output channel closed, stopping bridge");
                        break;
                    }
                }
                Err(e) => {
                    mylm_core::warn_log!("[BRIDGE] Broadcast recv error: {:?}", e);
                    break;
                }
            }
        }
        mylm_core::debug_log!(
            "[BRIDGE] Bridge task ended, forwarded {} events",
            event_count
        );
    });

    // Set the input channel
    app.input_tx = Some(input_tx);

    let session_handle = tokio::spawn(async move {
        mylm_core::debug_log!("[SESSION_TASK] Session started");
        session.run().await
    });

    // Run TUI
    match app::run_tui_session(app, output_rx, session_handle).await {
        Ok(result) => {
            mylm_core::debug_log!("[SESSION] TUI session ended: {:?}", result);
            Ok(result)
        }
        Err(e) => {
            mylm_core::error_log!("[SESSION] TUI error: {}", e);
            Err(e.into())
        }
    }
}
