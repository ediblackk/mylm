//! mylm - Terminal AI Assistant (V3 Architecture)
//!
//! Main entry point with EXACT original hub menu structure.
//! All handlers are stubbed for individual implementation.

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;

use mylm_core::config::Config;
use mylm_core::agent::runtime::orchestrator::commonbox::Commonbox;

mod hub;
mod settings;
mod tui;

use hub::HubChoice;
use hub::show_hub;

/// ============================================================================
/// MAIN ENTRY
/// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize debug.log in current directory
    let _ = mylm_core::init_debug_log(Some(std::path::PathBuf::from("debug.log")));
    
    // Ensure data directory exists
    let data_dir = dirs::data_dir()
        .context("Could not find data directory")?
        .join("mylm");
    std::fs::create_dir_all(&data_dir)?;
    
    // Load configuration
    let mut config = Config::load_or_default();
    
    // Check for first-run onboarding
    if !config.is_initialized() && config.providers.is_empty() {
        println!("\n👋 Welcome to mylm! Let's set up your first LLM provider.\n");
        setup_wizard(&mut config).await?;
        return Ok(());
    }
    
    // Show hub menu
    run_hub_menu(&mut config).await?;
    
    Ok(())
}

/// ============================================================================
/// HUB MENU LOOP - EXACT original structure with stubs
/// ============================================================================

async fn run_hub_menu(config: &mut Config) -> Result<()> {
    loop {
        match show_hub(config).await? {
            HubChoice::PopTerminal => {
                // Pop Terminal = TUI Session with tmux context injected
                println!("\n[STUB] Pop Terminal - Starting TUI with tmux context injection...\n");
                // TODO: Inject tmux context into TUI session
                // This would capture the current tmux pane's content and inject it into the terminal parser
                if let Err(e) = run_tui_with_session(config, false).await {
                    eprintln!("TUI error: {}", e);
                }
            }
            HubChoice::PopTerminalMissing => {
                println!("\n⚠️  tmux is not installed. Please install tmux to use Pop Terminal.\n");
            }
            HubChoice::ResumeSession => {
                use crate::tui::app::session_manager::SessionManager;
                
                // Load the latest TUI session (not just agent session)
                if let Some(session) = SessionManager::load_latest().await {
                    println!("\n🔄 Resuming session from {} with {} messages...\n", 
                        session.timestamp.format("%Y-%m-%d %H:%M"),
                        session.history.len());
                    match run_tui_with_saved_session(config, session).await {
                        Ok(tui::TuiResult::ReturnToHub) => {}
                        Ok(tui::TuiResult::Exit) => {
                            println!("\n👋 Goodbye!\n");
                            return Ok(());
                        }
                        Err(e) => eprintln!("TUI error: {}", e),
                    }
                } else {
                    println!("\n⚠️  No previous session found.\n");
                }
            }
            HubChoice::StartTui => {
                match run_tui_with_session(config, false).await {
                    Ok(tui::TuiResult::ReturnToHub) => {
                        // Continue to next hub iteration
                    }
                    Ok(tui::TuiResult::Exit) => {
                        println!("\n👋 Goodbye!\n");
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("TUI error: {}", e);
                    }
                }
            }
            HubChoice::StartIncognito => {
                // Incognito = TUI Session without memory enabled
                println!("\n🕵️  Incognito Mode - Starting TUI without memory persistence...\n");
                match run_tui_with_session(config, false).await {
                    Ok(tui::TuiResult::ReturnToHub) => {}
                    Ok(tui::TuiResult::Exit) => {
                        println!("\n👋 Goodbye!\n");
                        return Ok(());
                    }
                    Err(e) => eprintln!("TUI error: {}", e),
                }
            }
            HubChoice::QuickQuery => {
                let query: String = dialoguer::Input::new()
                    .with_prompt("Enter your query")
                    .interact()?;
                quick_query(config, &query).await?;
            }
            HubChoice::ManageSessions => {
                // Manage Sessions = Load/view/delete saved TUI Sessions
                println!("\n[STUB] Manage Sessions - Session management interface...\n");
                // TODO: List all saved sessions with metadata
                // Allow user to:
                // - Select and resume a session
                // - Delete old sessions
                // - Rename sessions
                // - View session statistics (cost, duration, etc.)
                run_session_manager(config).await?;
            }
            HubChoice::BackgroundJobs => {
                // Background Jobs = Create/edit/view daemon-spawned workers with scheduled jobs
                println!("\n[STUB] Background Jobs - Background job management...\n");
                // TODO: Interface for managing background workers
                // - List running/completed jobs
                // - Create new scheduled jobs (one-time or recurring)
                // - View job output and logs
                // - Cancel running jobs
                // - Edit job schedules
                run_background_jobs_manager(config).await?;
            }
            HubChoice::Configuration => {
                settings::run_settings_dashboard(config).await?;
            }
            HubChoice::Exit => {
                println!("\n👋 Goodbye!\n");
                break;
            }
        }
    }
    
    Ok(())
}

/// ============================================================================
/// SESSION MANAGER - Manage saved TUI Sessions
/// ============================================================================

async fn run_session_manager(config: &Config) -> Result<()> {
    use crate::tui::app::session_manager::SessionManager;
    use dialoguer::{Select, Confirm};
    
    loop {
        // Load all sessions
        let sessions = SessionManager::load_sessions();
        
        if sessions.is_empty() {
            println!("\n┌─────────────────────────────────────────────────────────────┐");
            println!("│                    SESSION MANAGER                          │");
            println!("├─────────────────────────────────────────────────────────────┤");
            println!("│                                                             │");
            println!("│  No saved sessions found.                                   │");
            println!("│  Start a new chat session to create one.                    │");
            println!("│                                                             │");
            println!("└─────────────────────────────────────────────────────────────┘\n");
            
            dialoguer::Input::<String>::new()
                .with_prompt("Press Enter to return to hub")
                .allow_empty(true)
                .interact()?;
            return Ok(());
        }
        
        // Build menu items
        let mut items: Vec<String> = sessions.iter().enumerate().map(|(i, s)| {
            let date = s.timestamp.format("%Y-%m-%d %H:%M");
            let preview = if s.metadata.last_message_preview.len() > 30 {
                format!("{}...", &s.metadata.last_message_preview[..30])
            } else {
                s.metadata.last_message_preview.clone()
            };
            format!("{:2}. [{}] {} msgs, ${:.4} - {}", 
                i + 1, date, s.metadata.message_count, s.metadata.cost, preview)
        }).collect();
        
        items.push("─".repeat(60));
        items.push("🗑️  Delete a session".to_string());
        items.push("✏️  Rename a session".to_string());
        items.push("🔄 Refresh".to_string());
        items.push("🔙 Back to hub".to_string());
        
        println!("\n");
        let selection = Select::new()
            .with_prompt("Select a session to resume, or choose an action")
            .items(&items)
            .default(0)
            .interact()?;
        
        let session_count = sessions.len();
        
        if selection < session_count {
            // Resume selected session
            let session = &sessions[selection];
            println!("\n📂 Resuming session from {}...", session.timestamp.format("%Y-%m-%d %H:%M"));
            
            // Start TUI with resumed session
            run_tui_with_saved_session(config, session.clone()).await?;
            return Ok(());
        } else {
            match selection - session_count {
                1 => {
                    // Delete session
                    let delete_idx = Select::new()
                        .with_prompt("Select a session to delete")
                        .items(&sessions.iter().enumerate().map(|(i, s)| {
                            format!("{:2}. [{}] {}", i + 1, 
                                s.timestamp.format("%Y-%m-%d %H:%M"),
                                s.metadata.message_count)
                        }).collect::<Vec<_>>())
                        .interact()?;
                    
                    let session_to_delete = &sessions[delete_idx];
                    if Confirm::new()
                        .with_prompt(format!("Delete session from {}?", 
                            session_to_delete.timestamp.format("%Y-%m-%d %H:%M")))
                        .default(false)
                        .interact()?
                    {
                        if let Err(e) = SessionManager::delete_session(&session_to_delete.id).await {
                            println!("❌ Failed to delete session: {}", e);
                        } else {
                            println!("✅ Session deleted");
                        }
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
                2 => {
                    // Rename session - currently just a placeholder
                    println!("\n✏️  Rename session - feature coming soon!");
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
                3 => continue, // Refresh
                _ => return Ok(()), // Back to hub
            }
        }
    }
}

/// ============================================================================
/// BACKGROUND JOBS MANAGER - Manage daemon workers (STUB)
/// ============================================================================

async fn run_background_jobs_manager(_config: &Config) -> Result<()> {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│                  BACKGROUND JOBS MANAGER                    │");
    println!("├─────────────────────────────────────────────────────────────┤");
    println!("│                                                             │");
    println!("│  [STUB] Background jobs not yet implemented                 │");
    println!("│                                                             │");
    println!("│  Planned features:                                          │");
    println!("│  • List running/completed jobs                              │");
    println!("│  • Create new scheduled jobs (one-time or recurring)        │");
    println!("│  • View job output and logs                                 │");
    println!("│  • Cancel running jobs                                      │");
    println!("│  • Edit job schedules                                       │");
    println!("│                                                             │");
    println!("│  Job Types:                                                 │");
    println!("│  • File watcher - Monitor files for changes                 │");
    println!("│  • Scheduled task - Run at specific times (cron-like)       │");
    println!("│  • Webhook listener - HTTP endpoint for triggers            │");
    println!("│  • Background worker - Long-running tasks                   │");
    println!("│                                                             │");
    println!("└─────────────────────────────────────────────────────────────┘\n");
    
    // TODO: Implement background jobs manager
    // This would:
    // 1. Connect to a daemon process that manages background workers
    // 2. List all jobs with status (running/paused/completed/failed)
    // 3. Allow creating new jobs with schedules
    // 4. Show job output/logs
    // 5. Allow cancelling/editing jobs
    
    dialoguer::Input::<String>::new()
        .with_prompt("Press Enter to return to hub")
        .allow_empty(true)
        .interact()?;
    
    Ok(())
}

/// ============================================================================
/// SETUP WIZARD - Stub
/// ============================================================================

async fn setup_wizard(config: &mut Config) -> Result<()> {
    println!("\n⚙️  Setup Wizard\n");
    
    if dialoguer::Confirm::new()
        .with_prompt("Would you like to add an LLM provider?")
        .default(true)
        .interact()?
    {
        hub::handle_add_provider(config).await?;
    }
    
    Ok(())
}

/// ============================================================================
/// TUI SESSION WRAPPER - Composition root for TUI
/// ============================================================================

async fn run_tui_with_session(config: &Config, resume: bool) -> Result<tui::TuiResult> {
    
    use mylm_core::agent::runtime::Session;
    use tokio::sync::mpsc;
    
    mylm_core::info_log!("[MAIN] Starting TUI session (profile: {}, resume: {})", config.active_profile, resume);
    
    // Create PTY manager FIRST (needed for terminal executor)
    let cwd = std::env::current_dir().ok();
    let (pty_manager, pty_rx) = match cwd {
        Some(path) => match tui::spawn_pty(Some(path)) {
            Ok((pm, rx)) => (pm, rx),
            Err(e) => {
                mylm_core::error_log!("[MAIN] Failed to spawn PTY: {}", e);
                eprintln!("❌ Failed to spawn PTY: {}", e);
                return Ok(tui::TuiResult::ReturnToHub);
            }
        },
        None => {
            mylm_core::error_log!("[MAIN] Could not determine current directory");
            eprintln!("❌ Could not determine current directory");
            return Ok(tui::TuiResult::ReturnToHub);
        }
    };
    
    // Create job registry
    let job_registry = tui::app::types::JobRegistry::new();
    
    // Create App with PTY (but no session yet)
    // Note: AppStateContainer::new is now async
    let mut app = tui::app::App::new(
        pty_manager,
        config.clone(),
        job_registry,
        false, // not incognito
    ).await;
    app.pty_rx = Some(pty_rx);
    
    // Create approval capability for interactive tool approval
    let (approval_capability, approval_rx) = tui::app::approval::TuiApprovalCapability::new();
    
    // Create terminal executor that uses the App's screen
    let app_weak = Arc::downgrade(&std::sync::Arc::new(std::sync::Mutex::new(())));
    let _ = app_weak; // Placeholder - will use actual app reference
    
    // For now, use default terminal executor (commands run via std::process::Command)
    // The terminal context is collected separately via app.get_screen()
    
    // Create commonbox for worker spawning (enables delegate tool)
    let commonbox = Arc::new(Commonbox::new());
    
    // Create agent session factory with approval and commonbox
    let factory = tui::agent_setup::create_session_factory(
        config,
        None, // Use default terminal executor for now
        Some(Arc::new(approval_capability)),
        Some(commonbox), // Enable worker spawning
    );
    
    // Create session - resumable if requested
    let (mut session, session_data) = if resume {
        match factory.create_resumable_session().await {
            Ok((s, data)) => (s, data),
            Err(e) => {
                mylm_core::error_log!("[MAIN] Failed to create resumable agent session: {}", e);
                eprintln!("❌ Failed to create agent session: {}", e);
                return Ok(tui::TuiResult::ReturnToHub);
            }
        }
    } else {
        match factory.create_default_session().await {
            Ok(s) => (s, None),
            Err(e) => {
                mylm_core::error_log!("[MAIN] Failed to create agent session: {}", e);
                eprintln!("❌ Failed to create agent session: {}", e);
                return Ok(tui::TuiResult::ReturnToHub);
            }
        }
    };
    
    // Restore session data if available (UI stays dumb, just displays what core provides)
    if let Some(ref data) = session_data {
        app.restore_from_session(data);
        println!("✅ Previous session restored with {} messages", data.history.len());
    }
    
    // Get input sender and subscribe to output events
    let input_tx = session.input_sender();
    let mut broadcast_rx = session.subscribe_output();
    
    // Create mpsc channel to bridge broadcast to unbounded for TUI
    let (output_tx, output_rx) = mpsc::unbounded_channel::<mylm_core::agent::OutputEvent>();
    
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
                        mylm_core::debug_log!("[BRIDGE] Forwarding event #{} (chunks={}): {:?}", 
                            event_count, chunk_count, std::mem::discriminant(&event));
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
        mylm_core::debug_log!("[BRIDGE] Bridge task ended, forwarded {} events", event_count);
    });
    
    // Set the input channel
    app.input_tx = Some(input_tx);
    
    let session_handle = tokio::spawn(async move {
        mylm_core::debug_log!("[SESSION_TASK] Session started");
        session.run().await
    });
    
    // Run TUI
    match tui::run_tui_session(app, output_rx, approval_rx, session_handle).await {
        Ok(result) => {
            mylm_core::debug_log!("[MAIN] TUI session ended: {:?}", result);
            Ok(result)
        }
        Err(e) => {
            mylm_core::error_log!("[MAIN] TUI error: {}", e);
            Err(e.into())
        }
    }
}

/// Run TUI with a saved session (restores chat history)
async fn run_tui_with_saved_session(config: &Config, saved_session: crate::tui::app::session::Session) -> Result<tui::TuiResult> {
    use mylm_core::agent::runtime::Session;
    use tokio::sync::mpsc;
    
    mylm_core::info_log!("[MAIN] Starting TUI with saved session (id: {}, messages: {})", 
        saved_session.id, saved_session.history.len());
    
    // Create PTY manager
    let cwd = std::env::current_dir().ok();
    let (pty_manager, pty_rx) = match cwd {
        Some(path) => match tui::spawn_pty(Some(path)) {
            Ok((pm, rx)) => (pm, rx),
            Err(e) => {
                mylm_core::error_log!("[MAIN] Failed to spawn PTY: {}", e);
                eprintln!("❌ Failed to spawn PTY: {}", e);
                return Ok(tui::TuiResult::ReturnToHub);
            }
        },
        None => {
            mylm_core::error_log!("[MAIN] Could not determine current directory");
            eprintln!("❌ Could not determine current directory");
            return Ok(tui::TuiResult::ReturnToHub);
        }
    };
    
    // Create job registry
    let job_registry = tui::app::types::JobRegistry::new();
    
    // Create App with PTY
    let mut app = tui::app::App::new(
        pty_manager,
        config.clone(),
        job_registry,
        false, // not incognito
    ).await;
    app.pty_rx = Some(pty_rx);
    
    // Restore chat history from saved session
    use crate::tui::app::TimestampedChatMessage;
    app.chat_history = saved_session.history.into_iter()
        .map(TimestampedChatMessage::from)
        .collect();
    app.session_id = saved_session.id;
    
    println!("✅ Loaded {} messages from saved session", app.chat_history.len());
    
    // Create approval capability
    let (approval_capability, approval_rx) = tui::app::approval::TuiApprovalCapability::new();
    
    // Create commonbox for worker spawning
    let commonbox = Arc::new(Commonbox::new());
    
    // Create agent session factory
    let factory = tui::agent_setup::create_session_factory(
        config,
        None,
        Some(Arc::new(approval_capability)),
        Some(commonbox),
    );
    
    // Create new agent session (we don't restore agent state, just UI state)
    let mut session = match factory.create_default_session().await {
        Ok(s) => s,
        Err(e) => {
            mylm_core::error_log!("[MAIN] Failed to create agent session: {}", e);
            eprintln!("❌ Failed to create agent session: {}", e);
            return Ok(tui::TuiResult::ReturnToHub);
        }
    };
    
    // Get input sender and subscribe to output events
    let input_tx = session.input_sender();
    let mut broadcast_rx = session.subscribe_output();
    
    // Create mpsc channel to bridge broadcast to unbounded for TUI
    let (output_tx, output_rx) = mpsc::unbounded_channel::<mylm_core::agent::OutputEvent>();
    
    // Spawn bridge task
    tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(event) => {
                    if output_tx.send(event).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    
    app.input_tx = Some(input_tx);
    
    let session_handle = tokio::spawn(async move {
        session.run().await
    });
    
    // Run TUI
    tui::run_tui_session(app, output_rx, approval_rx, session_handle).await
        .map_err(|e| e.into())
}

/// ============================================================================
/// QUICK QUERY - Using new Agent Session Factory
/// ============================================================================

async fn quick_query(config: &Config, query: &str) -> Result<()> {
    println!("\n⚡ Quick Query: {}", query);
    
    // Create agent session factory with commonbox (enables delegate tool)
    use mylm_core::agent::AgentSessionFactory;
    use mylm_core::agent::runtime::Session as ContractSession;
    
    let commonbox = Arc::new(Commonbox::new());
    let factory = AgentSessionFactory::new(config.clone())
        .with_commonbox(commonbox);
    
    // Create session for default profile
    let mut session = match factory.create_default_session().await {
        Ok(s) => s,
        Err(e) => {
            println!("❌ Failed to create agent session: {}", e);
            return Ok(());
        }
    };
    
    // Subscribe to output events
    let mut output_rx = session.subscribe_output();
    
    // Submit user input
    use mylm_core::agent::UserInput;
    if let Err(e) = session.submit_input(UserInput::Message(query.to_string())).await {
        println!("❌ Failed to submit input: {}", e);
        return Ok(());
    }
    
    // Run session and collect output
    let session_handle = tokio::spawn(async move {
        session.run().await
    });
    
    // Print output events as they arrive
    println!("\n🤔 Thinking...\n");
    loop {
        match output_rx.try_recv() {
            Ok(event) => {
                use mylm_core::agent::OutputEvent;
                match event {
                    OutputEvent::ResponseChunk { content } => {
                        print!("{}", content);
                    }
                    OutputEvent::ResponseComplete { .. } => {
                        println!("\n");
                        break;
                    }
                    OutputEvent::ToolExecuting { tool, .. } => {
                        println!("🔧 Using tool: {}", tool);
                    }
                    OutputEvent::Error { message } => {
                        println!("❌ Error: {}", message);
                        break;
                    }
                    OutputEvent::Halted { reason } => {
                        println!("\n✅ Session halted: {}", reason);
                        break;
                    }
                    _ => {}
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
            Err(_) => break,
        }
    }
    
    // Wait for session to complete
    if let Err(e) = session_handle.await {
        println!("❌ Session error: {}", e);
    }
    
    Ok(())
}
