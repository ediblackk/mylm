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
    SwitchProfile,
    EditProvider,
    EditApiUrl,
    EditApiKey,
    EditModel,
    EditPrompt,
    ManageEndpoints,
    Advanced,
    Save,
    Back,
}

#[derive(Debug, PartialEq)]
pub enum AdvancedSettingsChoice {
    WebSearch,
    General,
    ShellIntegration,
    Back,
}

#[derive(Debug, PartialEq)]
pub enum EndpointAction {
    SwitchConnection, // Link a different endpoint to current profile
    CreateNew,
    Delete,
    Back,
}

#[derive(Debug, PartialEq)]
pub enum ProfileAction {
    Select,
    Create,
    Duplicate,
    Rename,
    Delete,
    Back,
}

impl std::fmt::Display for SettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsChoice::SwitchProfile => write!(f, "👤 Switch Profile"),
            SettingsChoice::EditProvider => write!(f, "🏢 Provider (Connection)"),
            SettingsChoice::EditApiUrl => write!(f, "🔗 API Base URL"),
            SettingsChoice::EditApiKey => write!(f, "🔑 API Key"),
            SettingsChoice::EditModel => write!(f, "🧠 Model"),
            SettingsChoice::EditPrompt => write!(f, "📝 Custom Instructions (Prompt)"),
            SettingsChoice::ManageEndpoints => write!(f, "🔌 Manage Connections"),
            SettingsChoice::Advanced => write!(f, "⚙️  Advanced Settings"),
            SettingsChoice::Save => write!(f, "💾 Save & Exit"),
            SettingsChoice::Back => write!(f, "⬅️  Discard & Back"),
        }
    }
}

impl std::fmt::Display for AdvancedSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdvancedSettingsChoice::WebSearch => write!(f, "🌐 Web Search"),
            AdvancedSettingsChoice::General => write!(f, "⚙️  General Config"),
            AdvancedSettingsChoice::ShellIntegration => write!(f, "🐚 Shell Integration"),
            AdvancedSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

impl std::fmt::Display for EndpointAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointAction::SwitchConnection => write!(f, "🔗 Link Different Connection"),
            EndpointAction::CreateNew => write!(f, "➕ Create New Connection"),
            EndpointAction::Delete => write!(f, "🗑️  Delete Connection"),
            EndpointAction::Back => write!(f, "⬅️  Back"),
        }
    }
}

impl std::fmt::Display for ProfileAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileAction::Select => write!(f, "✅ Select Active Profile"),
            ProfileAction::Create => write!(f, "➕ Create New Profile"),
            ProfileAction::Duplicate => write!(f, "📋 Duplicate Profile"),
            ProfileAction::Rename => write!(f, "✏️  Rename Profile"),
            ProfileAction::Delete => write!(f, "🗑️  Delete Profile"),
            ProfileAction::Back => write!(f, "⬅️  Back"),
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
    
    // Calculate status strings
    let (provider_str, url_str, key_status, model_str) = match (profile, endpoint) {
        (Some(p), Some(e)) => {
            let effective_model = config.get_effective_model(p).unwrap_or_else(|_| e.model.clone());
            let model_source = if p.model.is_some() { "(Profile Override)" } else { "(Connection Default)" };
            
            let key_display = if e.api_key == "none" || e.api_key.is_empty() {
                "❌ Missing"
            } else {
                "✅ Set"
            };
            
            (
                e.provider.clone(),
                e.base_url.clone(),
                key_display,
                format!("{} {}", effective_model, model_source)
            )
        }
        _ => ("?".to_string(), "?".to_string(), "?", "?".to_string()),
    };

    let prompt_str = profile.map(|p| p.prompt.clone()).unwrap_or_else(|| "default".to_string());
    
    println!("\n📊 Active Configuration (Profile: '{}')", profile_name);
    println!("   ├─ Provider: {}", provider_str);
    println!("   ├─ Base URL: {}", url_str);
    println!("   ├─ API Key:  {}", key_status);
    println!("   ├─ Model:    {}", model_str);
    println!("   └─ Prompt:   {}", prompt_str);
    println!();

    let options = vec![
        SettingsChoice::SwitchProfile,
        SettingsChoice::EditProvider,
        SettingsChoice::EditApiUrl,
        SettingsChoice::EditApiKey,
        SettingsChoice::EditModel,
        SettingsChoice::EditPrompt,
        SettingsChoice::ManageEndpoints,
        SettingsChoice::Advanced,
        SettingsChoice::Save,
        SettingsChoice::Back,
    ];

    let ans: InquireResult<SettingsChoice> = Select::new(
        "Select a field to edit:",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(SettingsChoice::Back),
    }
}

/// Show Advanced Settings Submenu
pub fn show_advanced_submenu() -> Result<AdvancedSettingsChoice> {
    let options = vec![
        AdvancedSettingsChoice::WebSearch,
        AdvancedSettingsChoice::General,
        AdvancedSettingsChoice::ShellIntegration,
        AdvancedSettingsChoice::Back,
    ];

    let ans: InquireResult<AdvancedSettingsChoice> = Select::new(
        "Advanced Settings",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(AdvancedSettingsChoice::Back),
    }
}

/// Show Connection Management Submenu
pub fn show_endpoints_submenu() -> Result<EndpointAction> {
    let options = vec![
        EndpointAction::SwitchConnection,
        EndpointAction::CreateNew,
        EndpointAction::Delete,
        EndpointAction::Back,
    ];

    let ans: InquireResult<EndpointAction> = Select::new(
        "Connection Management",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(EndpointAction::Back),
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

/// Show the profiles submenu
pub fn show_profiles_submenu(_config: &Config) -> Result<ProfileAction> {
    let options = vec![
        ProfileAction::Select,
        ProfileAction::Create,
        ProfileAction::Duplicate,
        ProfileAction::Rename,
        ProfileAction::Delete,
        ProfileAction::Back,
    ];

    let ans: InquireResult<ProfileAction> = Select::new("Profiles Management", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(ProfileAction::Back),
    }
}

/// Show profile selection menu
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

/// Show profile creation wizard
pub fn show_profile_wizard(config: &Config) -> Result<Option<(String, String, Option<String>, String)>> {
    // Get profile name
    let name = inquire::Text::new("Profile name:").prompt()?;
    if name.trim().is_empty() {
        return Ok(None);
    }
    
    // Get endpoint selection
    let endpoints: Vec<String> = config.endpoints.iter().map(|e| e.name.clone()).collect();
    if endpoints.is_empty() {
        println!("⚠️  No endpoints configured. Please create an endpoint first.");
        return Ok(None);
    }
    
    let endpoint = inquire::Select::new("Select endpoint:", endpoints).prompt()?;
    
    // Get model (optional)
    let model_choice = inquire::Select::new("Select model:", vec!["Use endpoint default", "Choose specific model"]).prompt()?;
    let model = if model_choice == "Choose specific model" {
        let model_name = inquire::Text::new("Model name:").prompt()?;
        if model_name.trim().is_empty() {
            None
        } else {
            Some(model_name)
        }
    } else {
        None
    };
    
    // Get prompt
    let prompt = inquire::Text::new("Prompt name (without .md extension):")
        .with_default("default")
        .prompt()?;
    
    Ok(Some((name, endpoint, model, prompt)))
}

/// Show profile duplication wizard
pub fn show_profile_duplicate_wizard(config: &Config, source_profile: &str) -> Result<Option<(String, String, Option<String>, String)>> {
    // Get new profile name
    let name = inquire::Text::new("New profile name:").prompt()?;
    if name.trim().is_empty() {
        return Ok(None);
    }
    
    // Get source profile details
    let source = config.profiles.iter().find(|p| p.name == source_profile)
        .ok_or_else(|| anyhow::anyhow!("Profile not found"))?;
    
    // Get endpoint (default to source)
    let endpoints: Vec<String> = config.endpoints.iter().map(|e| e.name.clone()).collect();
    let endpoint = inquire::Select::new("Select endpoint:", endpoints).prompt()?;
    
    // Get model (default to source)
    let model = if let Some(model) = &source.model {
        let model_choice = inquire::Select::new("Model:", vec!["Keep source model", "Choose different model"]).prompt()?;
        if model_choice == "Choose different model" {
            let model_name = inquire::Text::new("Model name:").prompt()?;
            if model_name.trim().is_empty() {
                None
            } else {
                Some(model_name)
            }
        } else {
            Some(model.clone())
        }
    } else {
        None
    };
    
    // Get prompt (default to source)
    let prompt = inquire::Text::new("Prompt name (without .md extension):")
        .prompt()?;
    
    Ok(Some((name, endpoint, model, prompt)))
}

/// Show profile rename wizard
pub fn show_profile_rename_wizard(config: &Config, _old_name: &str) -> Result<Option<String>> {
    let new_name = inquire::Text::new("New profile name:").prompt()?;
    if new_name.trim().is_empty() {
        return Ok(None);
    }
    
    // Check if name already exists
    if config.profiles.iter().any(|p| p.name == new_name) {
        println!("⚠️  Profile with this name already exists.");
        return Ok(None);
    }
    
    Ok(Some(new_name))
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
