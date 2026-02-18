//! Hub V3 - Configuration Menu for mylm V3 Architecture
//!
//! Main hub entry point with menu loops and handlers.

pub mod choices;
pub mod display;
pub mod handlers;
pub mod settings;

use anyhow::Result;
use dialoguer::{Confirm, Input, Select};

use crate::tui::hub::choices::HubChoice;
use crate::tui::hub::display::{is_tmux_available, print_hub_banner, session_exists};
use crate::tui::TuiResult;
use mylm_core::config::Config;

/// Run the main hub menu loop
pub async fn run(config: &mut Config) -> Result<()> {
    loop {
        match show_hub(config).await? {
            HubChoice::PopTerminal => {
                // Pop Terminal = TUI Session with tmux context injected
                println!("\n[STUB] Pop Terminal - Starting TUI with tmux context injection...\n");
                if let Err(e) = crate::session::run_tui_with_session(config).await {
                    eprintln!("TUI error: {}", e);
                }
            }
            HubChoice::PopTerminalMissing => {
                println!("\n⚠️  tmux is not installed. Please install tmux to use Pop Terminal.\n");
            }
            HubChoice::ResumeSession => {
                if session_exists() {
                    println!("\n[STUB] Resume Session - Loading last saved TUI session...\n");
                    match crate::session::run_tui_with_session(config).await {
                        Ok(TuiResult::ReturnToHub) => {}
                        Ok(TuiResult::Exit) => {
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
                match crate::session::run_tui_with_session(config).await {
                    Ok(TuiResult::ReturnToHub) => {
                        // Continue to next hub iteration
                    }
                    Ok(TuiResult::Exit) => {
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
                match crate::session::run_tui_with_session(config).await {
                    Ok(TuiResult::ReturnToHub) => {}
                    Ok(TuiResult::Exit) => {
                        println!("\n👋 Goodbye!\n");
                        return Ok(());
                    }
                    Err(e) => eprintln!("TUI error: {}", e),
                }
            }
            HubChoice::QuickQuery => {
                let query: String = Input::new()
                    .with_prompt("Enter your query")
                    .interact()?;
                crate::query::run(config, &query).await?;
            }
            HubChoice::ManageSessions => {
                run_session_manager(config).await?;
            }
            HubChoice::BackgroundJobs => {
                run_background_jobs_manager(config).await?;
            }
            HubChoice::Configuration => {
                settings::run(config).await?;
            }
            HubChoice::Exit => {
                println!("\n👋 Goodbye!\n");
                break;
            }
        }
    }

    Ok(())
}

/// Show hub menu
async fn show_hub(_config: &Config) -> Result<HubChoice> {
    print_hub_banner();

    let mut options = Vec::new();

    // Check if session file exists
    let session_exists = dirs::data_dir()
        .map(|d| d.join("mylm").join("sessions").join("latest.json").exists())
        .unwrap_or(false);

    // Pop Terminal option
    if is_tmux_available() {
        options.push(HubChoice::PopTerminal);
    } else {
        options.push(HubChoice::PopTerminalMissing);
    }

    // Resume Session if exists
    if session_exists {
        options.push(HubChoice::ResumeSession);
    }

    // Main options
    options.extend(vec![
        HubChoice::StartTui,
        HubChoice::StartIncognito,
        HubChoice::QuickQuery,
        HubChoice::ManageSessions,
        HubChoice::BackgroundJobs,
        HubChoice::Configuration,
        HubChoice::Exit,
    ]);

    let selection = Select::new()
        .with_prompt("Welcome to mylm! What would you like to do?")
        .items(&options)
        .default(0)
        .interact()?;

    Ok(options[selection])
}

/// Setup wizard
pub async fn setup_wizard(config: &mut Config) -> Result<()> {
    println!("\n⚙️  Setup Wizard\n");

    if Confirm::new()
        .with_prompt("Would you like to add an LLM provider?")
        .default(true)
        .interact()?
    {
        handlers::handle_add_provider(config).await?;
    }

    Ok(())
}

/// Session manager (stub)
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

    dialoguer::Input::<String>::new()
        .with_prompt("Press Enter to return to hub")
        .allow_empty(true)
        .interact()?;

    Ok(())
}

/// Background jobs manager (stub)
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

    dialoguer::Input::<String>::new()
        .with_prompt("Press Enter to return to hub")
        .allow_empty(true)
        .interact()?;

    Ok(())
}
