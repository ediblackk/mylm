//! mylm - Terminal AI Assistant (V3 Architecture)
//!
//! Main entry point with EXACT original hub menu structure.
//! All handlers are stubbed for individual implementation.

use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;

use mylm_core::config::Config;

mod cli;
mod hub;
mod tui;

use cli::{Cli, Commands};
use hub::{HubChoice, SettingsMenuChoice, show_hub, show_settings_dashboard};

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
    
    // Parse CLI
    let cli = Cli::parse();
    
    // Load configuration
    let mut config = Config::load_or_default();
    
    // Handle subcommands
    match cli.command {
        Some(Commands::Config) => {
            run_settings_dashboard(&mut config).await?;
            return Ok(());
        }
        Some(Commands::Setup) => {
            setup_wizard(&mut config).await?;
            return Ok(());
        }
        _ => {}
    }
    
    // Handle direct query
    if !cli.query.is_empty() {
        let query = cli.query.join(" ");
        quick_query(&config, &query).await?;
        return Ok(());
    }
    
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
                if let Err(e) = run_tui_with_session(config).await {
                    eprintln!("TUI error: {}", e);
                }
            }
            HubChoice::PopTerminalMissing => {
                println!("\n⚠️  tmux is not installed. Please install tmux to use Pop Terminal.\n");
            }
            HubChoice::ResumeSession => {
                if hub::session_exists() {
                    println!("\n[STUB] Resume Session - Loading last saved TUI session...\n");
                    // TODO: Load session history and terminal state from disk
                    // This would restore chat_history, terminal_parser state, and session metadata
                    match run_tui_with_session(config).await {
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
                match run_tui_with_session(config).await {
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
                println!("\n[STUB] Incognito Mode - Starting TUI without memory persistence...\n");
                // TODO: Disable memory features in TUI
                // - memory_graph should be empty or disabled
                // - scratchpad should not persist
                // - session should not auto-save on exit
                match run_tui_with_session(config).await {
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
                run_settings_dashboard(config).await?;
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
/// SETTINGS DASHBOARD - REVISED with Main/Worker LLM comprehensive settings
/// ============================================================================

async fn run_settings_dashboard(config: &mut Config) -> Result<()> {
    loop {
        match show_settings_dashboard(config)? {
            SettingsMenuChoice::ManageProviders => {
                run_provider_menu(config).await?;
            }
            SettingsMenuChoice::MainLLMSettings => {
                run_main_llm_settings(config).await?;
            }
            SettingsMenuChoice::WorkerLLMSettings => {
                run_worker_llm_settings(config).await?;
            }
            SettingsMenuChoice::TestMainConnection => {
                hub::test_profile_connection(config, &config.active_profile.clone()).await?;
            }
            SettingsMenuChoice::TestWorkerConnection => {
                hub::test_profile_connection(config, "worker").await?;
            }
            SettingsMenuChoice::WebSearchSettings => {
                run_web_search_menu(config).await?;
            }
            SettingsMenuChoice::ApplicationSettings => {
                run_application_settings(config).await?;
            }
            SettingsMenuChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// MAIN LLM SETTINGS - Comprehensive settings menu
/// ============================================================================

async fn run_main_llm_settings(config: &mut Config) -> Result<()> {
    use hub::{MainLLMSettingsChoice, ContextSettingsChoice, AgenticSettingsChoice, PaCoReSubSettingsChoice};
    
    loop {
        match hub::show_main_llm_settings_menu(config)? {
            MainLLMSettingsChoice::SelectModel => {
                hub::handle_select_main_model(config).await?;
            }
            MainLLMSettingsChoice::ContextSettings => {
                loop {
                    match hub::show_context_settings_menu(true)? {
                        ContextSettingsChoice::SetMaxTokens => {
                            hub::set_max_tokens(config, true)?;
                        }
                        ContextSettingsChoice::SetCondenseThreshold => {
                            hub::set_condense_threshold(config, true)?;
                        }
                        ContextSettingsChoice::SetInputPrice => {
                            hub::set_input_price(config, true)?;
                        }
                        ContextSettingsChoice::SetOutputPrice => {
                            hub::set_output_price(config, true)?;
                        }
                        ContextSettingsChoice::SetRateLimit => {
                            hub::set_rate_limit_rpm(config, true)?;
                        }
                        ContextSettingsChoice::Back => break,
                    }
                }
            }
            MainLLMSettingsChoice::AgenticSettings => {
                loop {
                    match hub::show_agentic_settings_menu(true)? {
                        AgenticSettingsChoice::SetAllowedCommands => {
                            hub::set_allowed_commands(config, true)?;
                        }
                        AgenticSettingsChoice::SetRestrictedCommands => {
                            hub::set_restricted_commands(config, true)?;
                        }
                        AgenticSettingsChoice::SetMaxActionsBeforeStall => {
                            hub::set_max_actions_before_stall(config, true)?;
                        }
                        AgenticSettingsChoice::PaCoReSettings => {
                            loop {
                                match hub::show_pacore_sub_settings_menu()? {
                                    PaCoReSubSettingsChoice::ToggleEnabled => {
                                        hub::toggle_pacore_enabled(config, true)?;
                                    }
                                    PaCoReSubSettingsChoice::SetRounds => {
                                        hub::set_pacore_rounds(config, true)?;
                                    }
                                    PaCoReSubSettingsChoice::Back => break,
                                }
                            }
                        }
                        AgenticSettingsChoice::Back => break,
                    }
                }
            }
            MainLLMSettingsChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// WORKER LLM SETTINGS - Comprehensive settings menu
/// ============================================================================

async fn run_worker_llm_settings(config: &mut Config) -> Result<()> {
    use hub::{WorkerLLMSettingsChoice, ContextSettingsChoice, AgenticSettingsChoice, PaCoReSubSettingsChoice};
    
    loop {
        match hub::show_worker_llm_settings_menu(config)? {
            WorkerLLMSettingsChoice::SelectModel => {
                hub::handle_select_worker_model(config).await?;
            }
            WorkerLLMSettingsChoice::ContextSettings => {
                loop {
                    match hub::show_context_settings_menu(false)? {
                        ContextSettingsChoice::SetMaxTokens => {
                            hub::set_max_tokens(config, false)?;
                        }
                        ContextSettingsChoice::SetCondenseThreshold => {
                            hub::set_condense_threshold(config, false)?;
                        }
                        ContextSettingsChoice::SetInputPrice => {
                            hub::set_input_price(config, false)?;
                        }
                        ContextSettingsChoice::SetOutputPrice => {
                            hub::set_output_price(config, false)?;
                        }
                        ContextSettingsChoice::SetRateLimit => {
                            hub::set_rate_limit_rpm(config, false)?;
                        }
                        ContextSettingsChoice::Back => break,
                    }
                }
            }
            WorkerLLMSettingsChoice::AgenticSettings => {
                loop {
                    match hub::show_agentic_settings_menu(false)? {
                        AgenticSettingsChoice::SetAllowedCommands => {
                            hub::set_allowed_commands(config, false)?;
                        }
                        AgenticSettingsChoice::SetRestrictedCommands => {
                            hub::set_restricted_commands(config, false)?;
                        }
                        AgenticSettingsChoice::SetMaxActionsBeforeStall => {
                            hub::set_max_actions_before_stall(config, false)?;
                        }
                        AgenticSettingsChoice::PaCoReSettings => {
                            loop {
                                match hub::show_pacore_sub_settings_menu()? {
                                    PaCoReSubSettingsChoice::ToggleEnabled => {
                                        hub::toggle_pacore_enabled(config, false)?;
                                    }
                                    PaCoReSubSettingsChoice::SetRounds => {
                                        hub::set_pacore_rounds(config, false)?;
                                    }
                                    PaCoReSubSettingsChoice::Back => break,
                                }
                            }
                        }
                        AgenticSettingsChoice::Back => break,
                    }
                }
            }
            WorkerLLMSettingsChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// PROVIDER MENU
/// ============================================================================

async fn run_provider_menu(config: &mut Config) -> Result<()> {
    use hub::ProviderMenuChoice;
    
    loop {
        match hub::show_provider_menu()? {
            ProviderMenuChoice::AddProvider => {
                hub::handle_add_provider(config).await?;
            }
            ProviderMenuChoice::EditProvider => {
                hub::handle_edit_provider(config).await?;
            }
            ProviderMenuChoice::RemoveProvider => {
                hub::handle_remove_provider(config)?;
            }
            ProviderMenuChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// WEB SEARCH MENU
/// ============================================================================

async fn run_web_search_menu(config: &mut Config) -> Result<()> {
    // Sync features.web_search with profile.web_search.enabled on entry
    let profile_enabled = config.active_profile().web_search.enabled;
    if config.features.web_search != profile_enabled {
        log::debug!("[CONFIG] Syncing web_search: features={} profile={}", 
            config.features.web_search, profile_enabled);
        config.features.web_search = profile_enabled;
    }
    
    // Use the unified web search settings handler
    hub::handle_web_search_settings(config).await?;
    Ok(())
}

/// ============================================================================
/// APPLICATION SETTINGS - Global settings (tmux, alias, etc)
/// ============================================================================

async fn run_application_settings(config: &mut Config) -> Result<()> {
    use hub::ApplicationSettingsChoice;
    
    loop {
        match hub::show_application_settings_menu()? {
            ApplicationSettingsChoice::ToggleTmuxAutostart => {
                config.app.tmux_enabled = !config.app.tmux_enabled;
                config.save_default()?;
                println!("\n✅ Tmux autostart {}", 
                    if config.app.tmux_enabled { "enabled" } else { "disabled" });
            }
            ApplicationSettingsChoice::SetPreferredAlias => {
                let alias: String = dialoguer::Input::new()
                    .with_prompt("Enter preferred alias")
                    .default(config.active_profile.clone())
                    .interact()?;
                // Store alias in config (would need custom field)
                println!("\n[STUB] Set preferred alias to: {}\n", alias);
            }
            ApplicationSettingsChoice::Back => break,
        }
    }
    Ok(())
}

/// ============================================================================
/// SESSION MANAGER - Manage saved TUI Sessions (STUB)
/// ============================================================================

async fn run_session_manager(_config: &Config) -> Result<()> {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│                    SESSION MANAGER                          │");
    println!("├─────────────────────────────────────────────────────────────┤");
    println!("│                                                             │");
    println!("│  [STUB] Session management not yet implemented              │");
    println!("│                                                             │");
    println!("│  Planned features:                                          │");
    println!("│  • List saved sessions with metadata (date, cost, tokens)   │");
    println!("│  • Resume selected session                                  │");
    println!("│  • Delete old sessions                                      │");
    println!("│  • Rename sessions                                          │");
    println!("│  • View session statistics                                  │");
    println!("│                                                             │");
    println!("└─────────────────────────────────────────────────────────────┘\n");
    
    // TODO: Implement session manager menu
    // This would:
    // 1. Scan ~/.local/share/mylm/sessions/ for saved sessions
    // 2. Display list with metadata (name, date, cost, message count)
    // 3. Allow user to select a session to resume
    // 4. Allow deletion and renaming of sessions
    
    dialoguer::Input::<String>::new()
        .with_prompt("Press Enter to return to hub")
        .allow_empty(true)
        .interact()?;
    
    Ok(())
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

async fn run_tui_with_session(config: &Config) -> Result<tui::TuiResult> {
    use mylm_core::agent::AgentSessionFactory;
    use mylm_core::agent::contract::Session;
    use tokio::sync::mpsc;
    
    mylm_core::info_log!("[MAIN] Starting TUI session (profile: {})", config.active_profile);
    
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
    let job_registry = tui::types::JobRegistry::new();
    
    // Create App with PTY (but no session yet)
    let mut app = tui::app::App::new(
        pty_manager,
        config.clone(),
        job_registry,
        false, // not incognito
    );
    app.pty_rx = Some(pty_rx);
    
    // Create terminal executor that uses the App's parser
    // We use a shared reference so the executor can access the App's screen
    let app_ref = std::sync::Arc::new(std::sync::Mutex::new(()));
    let _ = app_ref; // Placeholder - we'll use a different approach
    
    // For now, use default terminal executor
    // In the future, we'd create a TuiTerminalExecutor here
    // let terminal_executor = Arc::new(tui::terminal_executor::TuiTerminalExecutor::from_app(app_ref));
    
    // Create approval capability for interactive tool approval
    let (approval_capability, _approval_rx) = tui::approval::TuiApprovalCapability::new();
    let approval_arc = Arc::new(approval_capability);
    
    // Create agent session factory
    let factory = AgentSessionFactory::new(config.clone());
    
    // Create session for default profile
    let mut session = match factory.create_default_session() {
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
    let (output_tx, output_rx) = mpsc::unbounded_channel::<mylm_core::agent::contract::session::OutputEvent>();
    
    // Spawn bridge task to forward events from broadcast to mpsc
    tokio::spawn(async move {
        let mut event_count = 0u64;
        let mut chunk_count = 0u64;
        loop {
            match broadcast_rx.recv().await {
                Ok(event) => {
                    event_count += 1;
                    use mylm_core::agent::contract::session::OutputEvent;
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
    match tui::run_tui_session(app, output_rx, session_handle).await {
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

/// ============================================================================
/// QUICK QUERY - Using new Agent Session Factory
/// ============================================================================

async fn quick_query(config: &Config, query: &str) -> Result<()> {
    println!("\n⚡ Quick Query: {}", query);
    
    // Create agent session factory
    use mylm_core::agent::AgentSessionFactory;
    use mylm_core::agent::contract::Session as ContractSession;
    
    let factory = AgentSessionFactory::new(config.clone());
    
    // Create session for default profile
    let mut session = match factory.create_default_session() {
        Ok(s) => s,
        Err(e) => {
            println!("❌ Failed to create agent session: {}", e);
            return Ok(());
        }
    };
    
    // Subscribe to output events
    let mut output_rx = session.subscribe_output();
    
    // Submit user input
    use mylm_core::agent::contract::session::UserInput;
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
                use mylm_core::agent::contract::session::OutputEvent;
                match event {
                    OutputEvent::ResponseChunk { content } => {
                        print!("{}", content);
                    }
                    OutputEvent::ResponseComplete => {
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
