//! Hub module - Configuration UI and menu system
//!
//! This module has been refactored into focused submodules:
//! - `hub/menus.rs` - Menu enums and Display implementations
//! - `hub/providers.rs` - Provider management (add, edit, remove)
//! - `hub/models.rs` - Main and worker model selection
//! - `hub/web_search.rs` - Web search settings
//! - `hub/agent_settings.rs` - Agent settings (iterations, PaCoRe, tmux)
//! - `hub/utils.rs` - Shared utilities and helpers

pub mod agent_settings;
pub mod menus;
pub mod models;
pub mod providers;
pub mod utils;
pub mod web_search;

use anyhow::Result;
use inquire::Select as InquireSelect;
use mylm_core::config::Config;

pub use menus::*;
pub use menus::PermissionsMenuChoice;
pub use utils::{display_config_summary, is_tmux_available, print_banner};

// Provider management
pub use providers::{
    handle_add_provider, handle_edit_provider, handle_remove_provider,
    show_provider_menu,
};

// Model selection
pub use models::{handle_select_main_model, handle_select_worker_model};

// Web search
pub use web_search::handle_web_search_settings;

// Agent settings
pub use agent_settings::{
    handle_max_iterations, handle_set_allowed_tools, handle_set_auto_approve_commands,
    handle_set_forbidden_commands, handle_set_main_rpm, handle_set_pacore_rounds,
    handle_set_rate_limit, handle_set_workers_rpm, handle_toggle_pacore,
    handle_toggle_tmux_autostart, show_agent_settings_menu, show_iterations_settings_menu,
    show_pacore_settings_menu, show_permissions_menu, show_rate_limit_settings_menu,
};

/// Show the interactive hub menu
pub async fn show_hub(config: &Config) -> Result<HubChoice> {
    // Clear the loading line, then use alternate screen buffer
    print!("\r\x1B[K"); // Clear current line
    print!("\x1B[?1049h"); // Enter alternate screen
    let _ = std::io::Write::flush(&mut std::io::stdout());

    print_banner(config).await;

    let mut options = Vec::new();

    // Check if session file exists
    let session_exists = dirs::data_dir()
        .map(|d| d.join("mylm").join("sessions").join("latest.json").exists())
        .unwrap_or(false);

    if is_tmux_available() {
        options.push(HubChoice::PopTerminal);
    } else {
        options.push(HubChoice::PopTerminalMissing);
    }

    if session_exists {
        options.push(HubChoice::ResumeSession);
    }

    options.extend(vec![
        HubChoice::StartTui,
        HubChoice::StartIncognito,
        HubChoice::QuickQuery,
        HubChoice::ManageSessions,
        HubChoice::BackgroundJobs,
        HubChoice::Configuration,
        HubChoice::Exit,
    ]);

    let ans: Result<HubChoice, _> =
        InquireSelect::new("Welcome to mylm! What would you like to do?", options)
            .with_page_size(20)
            .prompt();

    let result = match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(HubChoice::Exit),
    };

    // Exit alternate screen buffer
    print!("\x1B[?1049l");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    result
}

/// Show session selection menu
pub fn show_session_select(sessions: Vec<String>) -> Result<Option<String>> {
    let mut options = sessions;
    options.push("⬅️  Back".to_string());

    let ans: Result<String, _> =
        InquireSelect::new("Select Session to Resume", options).prompt();

    match ans {
        Ok(choice) if choice == "⬅️  Back" => Ok(None),
        Ok(choice) => Ok(Some(choice)),
        Err(_) => Ok(None),
    }
}

// ============================================================================
// SETTINGS DASHBOARD - Main Configuration Menu
// ============================================================================

/// Main settings dashboard - shows current config summary
pub fn show_settings_dashboard(config: &Config) -> Result<SettingsMenuChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");

    // Display current state summary
    display_config_summary(config);

    let options = vec![
        SettingsMenuChoice::ManageProviders,
        SettingsMenuChoice::SelectMainModel,
        SettingsMenuChoice::SelectWorkerModel,
        SettingsMenuChoice::WebSearchSettings,
        SettingsMenuChoice::AgentSettings,
        SettingsMenuChoice::Back,
    ];

    let ans: Result<SettingsMenuChoice, _> =
        InquireSelect::new("Configuration Menu", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(SettingsMenuChoice::Back),
    }
}
