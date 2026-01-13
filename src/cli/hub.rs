use inquire::{Select, error::InquireResult};
use mylm_core::config::Config;
use anyhow::Result;
use console::Style;

#[derive(Debug, PartialEq)]
pub enum HubChoice {
    PopTerminal,
    PopTerminalMissing,
    ResumeSession,
    StartTui,
    QuickQuery,
    ManageSessions,
    Configuration,
    Exit,
}

impl std::fmt::Display for HubChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HubChoice::PopTerminal => {
                if mylm_core::context::terminal::TerminalContext::is_inside_tmux() {
                    write!(f, "🚀 Pop Terminal with Seamless Context")
                } else {
                    write!(f, "🚀 Pop Terminal (Limited Context - Not in tmux)")
                }
            },
            HubChoice::PopTerminalMissing => write!(f, "🚀 Pop Terminal (tmux Required)"),
            HubChoice::ResumeSession => write!(f, "🔄 Resume Latest Session"),
            HubChoice::StartTui => write!(f, "✨ Start Fresh TUI Session"),
            HubChoice::QuickQuery => write!(f, "⚡ Quick Query"),
            HubChoice::Configuration => write!(f, "⚙️  Configuration"),
            HubChoice::ManageSessions => write!(f, "📂 Manage Sessions"),
            HubChoice::Exit => write!(f, "❌ Exit"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum SettingsChoice {
    SelectProfile,
    SelectEndpoint,
    EditEndpoint,
    EditPrompt,
    EditSearch,
    EditGeneral,
    EditApiKeys,
    NewProfile,
    ShellIntegration,
    Save,
    Back,
}

impl std::fmt::Display for SettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsChoice::SelectProfile => write!(f, "👤 Switch Profile"),
            SettingsChoice::SelectEndpoint => write!(f, "🔗 Change Endpoint for Profile"),
            SettingsChoice::EditEndpoint => write!(f, "🤖 Edit Endpoint Details"),
            SettingsChoice::EditGeneral => write!(f, "⚙️  General Settings"),
            SettingsChoice::EditApiKeys => write!(f, "🔑 API Keys"),
            SettingsChoice::EditSearch => write!(f, "🌐 Search Config"),
            SettingsChoice::EditPrompt => write!(f, "📝 System Prompt"),
            SettingsChoice::NewProfile => write!(f, "➕ Create New Profile"),
            SettingsChoice::ShellIntegration => write!(f, "🐚 Shell Integration"),
            SettingsChoice::Save => write!(f, "💾 Save & Exit"),
            SettingsChoice::Back => write!(f, "⬅️  Discard & Back"),
        }
    }
}

/// Check if tmux is installed and available in PATH
pub fn is_tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok()
}

/// Show the interactive hub menu
pub async fn show_hub(config: &Config) -> Result<HubChoice> {
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
        HubChoice::QuickQuery,
        HubChoice::ManageSessions,
        HubChoice::Configuration,
        HubChoice::Exit,
    ]);

    let ans: InquireResult<HubChoice> = Select::new("Welcome to mylm! What would you like to do?", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(HubChoice::Exit),
    }
}

/// Show profile selection menu with a back option
pub fn show_profile_select(profiles: Vec<String>) -> Result<Option<String>> {
    let mut options = profiles;
    options.push("⬅️  Back".to_string());

    let ans: InquireResult<String> = Select::new("Select Active Profile", options).prompt();

    match ans {
        Ok(choice) if choice == "⬅️  Back" => Ok(None),
        Ok(choice) => Ok(Some(choice)),
        Err(_) => Ok(None),
    }
}

/// Show endpoint selection menu
pub fn show_endpoint_select(endpoints: Vec<String>, _current: &str) -> Result<Option<String>> {
    let mut options = endpoints;
    options.push("⬅️  Back".to_string());

    let ans: InquireResult<String> = Select::new("Select Endpoint", options)
        .prompt();

    match ans {
        Ok(choice) if choice == "⬅️  Back" => Ok(None),
        Ok(choice) => Ok(Some(choice)),
        Err(_) => Ok(None),
    }
}

pub fn show_session_select(sessions: Vec<String>) -> Result<Option<String>> {
    let mut options = sessions;
    options.push("⬅️  Back".to_string());

    let ans: InquireResult<String> = Select::new("Select Session to Resume", options).prompt();

    match ans {
        Ok(choice) if choice == "⬅️  Back" => Ok(None),
        Ok(choice) => Ok(Some(choice)),
        Err(_) => Ok(None),
    }
}

/// Show the unified settings dashboard
pub fn show_settings_dashboard(config: &Config) -> Result<SettingsChoice> {
    let profile = config.get_active_profile();
    let endpoint = config.get_endpoint(None).ok();
    
    let profile_name = config.active_profile.clone();
    let llm_status = match (profile, endpoint) {
        (Some(_), Some(e)) => format!("{} ({} / {})", e.name, e.provider, e.model),
        _ => "Not Configured".to_string(),
    };
    
    let search_status = if config.web_search.enabled {
        format!("Enabled ({})", config.web_search.provider)
    } else {
        "Disabled".to_string()
    };

    let prompt_status = profile.map(|p| p.prompt.clone()).unwrap_or_else(|| "default".to_string());

    let options = vec![
        SettingsChoice::SelectProfile,
        SettingsChoice::SelectEndpoint,
        SettingsChoice::EditEndpoint,
        SettingsChoice::EditPrompt,
        SettingsChoice::EditSearch,
        SettingsChoice::EditGeneral,
        SettingsChoice::EditApiKeys,
        SettingsChoice::NewProfile,
        SettingsChoice::ShellIntegration,
        SettingsChoice::Save,
        SettingsChoice::Back,
    ];

    let ans: InquireResult<SettingsChoice> = Select::new(
        &format!(
            "⚙️  Settings Dashboard | Profile: {} | Endpoint: {} | Search: {} | Prompt: {}",
            profile_name, llm_status, search_status, prompt_status
        ),
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(SettingsChoice::Back),
    }
}

#[derive(Debug, PartialEq)]
pub enum ApiKeyEditChoice {
    LlmKey,
    SearchKey,
    Back,
}

impl std::fmt::Display for ApiKeyEditChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiKeyEditChoice::LlmKey => write!(f, "🤖 LLM API Key"),
            ApiKeyEditChoice::SearchKey => write!(f, "🌐 Search API Key"),
            ApiKeyEditChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ShellIntegrationChoice {
    ToggleTmuxAutoStart,
    Back,
}

impl std::fmt::Display for ShellIntegrationChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellIntegrationChoice::ToggleTmuxAutoStart => write!(f, "🪟 Toggle tmux Auto-Start (Seamless Context)"),
            ShellIntegrationChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

pub fn show_shell_integration_menu() -> Result<ShellIntegrationChoice> {
    let options = vec![
        ShellIntegrationChoice::ToggleTmuxAutoStart,
        ShellIntegrationChoice::Back,
    ];

    let ans: InquireResult<ShellIntegrationChoice> = Select::new("Shell Integration Settings", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(ShellIntegrationChoice::Back),
    }
}

pub fn show_api_key_menu() -> Result<ApiKeyEditChoice> {
    let options = vec![
        ApiKeyEditChoice::LlmKey,
        ApiKeyEditChoice::SearchKey,
        ApiKeyEditChoice::Back,
    ];

    let ans: InquireResult<ApiKeyEditChoice> = Select::new("Edit API Keys", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(ApiKeyEditChoice::Back),
    }
}

async fn print_banner(config: &Config) {
    let blue = Style::new().blue().bold();
    let green = Style::new().green().bold();
    let cyan = Style::new().cyan();
    let dim = Style::new().dim();

    let banner = r#"
    __  ___  __  __  __     __  ___
   /  |/  / / / / / / /    /  |/  /
  / /|_/ / / / / / / /    / /|_/ /
 / /  / / / /_/ / / /___ / /  / /
/_/  /_/  \__, / /_____//_/  /_/
         /____/
    "#;

    println!("{}", green.apply_to(banner));
    println!("          {}", cyan.apply_to("My Language Model"));
    println!("          {}", dim.apply_to("Rust-Powered Terminal AI"));
    println!();

    // Compact Provider/Context Info
    let profile = config.get_active_profile();
    let endpoint = config.get_endpoint(None).ok();
    
    let profile_name = &config.active_profile;
    let llm_info = match (profile, endpoint) {
        (Some(_), Some(e)) => format!("{} ({} / {})", e.name, e.provider, e.model),
        _ => "Not Configured".to_string(),
    };

    let ctx = mylm_core::context::TerminalContext::collect_sync();
    let cwd = ctx.cwd().unwrap_or_else(|| "unknown".to_string());
    let branch = ctx.git_branch().unwrap_or_else(|| "none".to_string());
    let tmux_status = if mylm_core::context::terminal::TerminalContext::is_inside_tmux() {
        green.apply_to("Active")
    } else {
        Style::new().yellow().apply_to("Inactive (No Scrollback)")
    };

    println!("  {} [{}] | {} [{}]",
        dim.apply_to("Profile:"), blue.apply_to(profile_name),
        dim.apply_to("Endpoint:"), green.apply_to(llm_info)
    );
    println!("  {} [{}] | {} [{}] | {} [{}]",
        dim.apply_to("Context:"), cyan.apply_to(cwd),
        dim.apply_to("Git:"), cyan.apply_to(branch),
        dim.apply_to("Tmux:"), tmux_status
    );
    println!("  {}", dim.apply_to("-".repeat(60).as_str()));
}
