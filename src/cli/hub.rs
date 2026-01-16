use anyhow::Result;
use console::Style;
use dialoguer::{Confirm, Password};
use inquire::{Select as InquireSelect, Text};
use mylm_core::config::Config;

/// Main hub choice enum
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

/// Settings dashboard main menu choices
#[derive(Debug, PartialEq)]
pub enum SettingsMenuChoice {
    SwitchActiveProfile,
    ManageProfiles,
    ManageEndpoints,
    Back,
}

impl std::fmt::Display for SettingsMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsMenuChoice::SwitchActiveProfile => write!(f, "👤 [1] Switch Active Profile"),
            SettingsMenuChoice::ManageProfiles => write!(f, "📂 [2] Manage Profiles"),
            SettingsMenuChoice::ManageEndpoints => write!(f, "🔌 [3] Manage Endpoints"),
            SettingsMenuChoice::Back => write!(f, "⬅️  [4] Back"),
        }
    }
}

/// Profile management submenu choices
#[derive(Debug, PartialEq)]
pub enum ProfileMenuChoice {
    CreateProfile,
    EditProfile,
    DeleteProfile,
    Back,
}

impl std::fmt::Display for ProfileMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileMenuChoice::CreateProfile => write!(f, "➕ [1] Create New Profile"),
            ProfileMenuChoice::EditProfile => write!(f, "✏️  [2] Edit Profile (Model/Prompt)"),
            ProfileMenuChoice::DeleteProfile => write!(f, "🗑️  [3] Delete Profile"),
            ProfileMenuChoice::Back => write!(f, "⬅️  [4] Back"),
        }
    }
}

/// Endpoint management submenu choices
#[derive(Debug, PartialEq)]
pub enum EndpointMenuChoice {
    CreateEndpoint,
    EditEndpoint,
    DeleteEndpoint,
    Back,
}

impl std::fmt::Display for EndpointMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointMenuChoice::CreateEndpoint => write!(f, "➕ [1] Create New Endpoint"),
            EndpointMenuChoice::EditEndpoint => write!(f, "✏️  [2] Edit Endpoint (Provider/Key/URL)"),
            EndpointMenuChoice::DeleteEndpoint => write!(f, "🗑️  [3] Delete Endpoint"),
            EndpointMenuChoice::Back => write!(f, "⬅️  [4] Back"),
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

    let ans: Result<HubChoice, _> = InquireSelect::new("Welcome to mylm! What would you like to do?", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(HubChoice::Exit),
    }
}

/// Show session selection menu
pub fn show_session_select(sessions: Vec<String>) -> Result<Option<String>> {
    let mut options = sessions;
    options.push("⬅️  Back".to_string());

    let ans: Result<String, _> = InquireSelect::new("Select Session to Resume", options).prompt();

    match ans {
        Ok(choice) if choice == "⬅️  Back" => Ok(None),
        Ok(choice) => Ok(Some(choice)),
        Err(_) => Ok(None),
    }
}

/// Show profile selection menu
pub fn show_profile_select(profiles: Vec<String>) -> Result<Option<String>> {
    let mut options = profiles;
    options.push("⬅️  Back".to_string());

    let ans: Result<String, _> = InquireSelect::new("Select Active Profile", options).prompt();

    match ans {
        Ok(choice) if choice == "⬅️  Back" => Ok(None),
        Ok(choice) => Ok(Some(choice)),
        Err(_) => Ok(None),
    }
}

// ============================================================================
// SETTINGS DASHBOARD - Main Configuration Menu
// ============================================================================

/// Main settings dashboard - presents a clean Menu System
pub fn show_settings_dashboard(config: &Config) -> Result<SettingsMenuChoice> {
    // Display current state (Profile + linked Endpoint info)
    display_current_config(config);

    let options = vec![
        SettingsMenuChoice::SwitchActiveProfile,
        SettingsMenuChoice::ManageProfiles,
        SettingsMenuChoice::ManageEndpoints,
        SettingsMenuChoice::Back,
    ];

    let ans: Result<SettingsMenuChoice, _> = InquireSelect::new(
        "Configuration Menu",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(SettingsMenuChoice::Back),
    }
}

/// Display current configuration summary
fn display_current_config(config: &Config) {
    let profile = config.get_active_profile();
    let profile_name = &config.active_profile;

    println!("\n📊 Current Configuration");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));

    if let Some(p) = profile {
        // Get the linked endpoint
        let endpoint_info = config.get_endpoint(Some(&p.endpoint)).ok();

        println!("  Profile: {}", Style::new().green().bold().apply_to(&profile_name));
        println!("  ├─ Linked Endpoint: {}", p.endpoint);

        if let Some(e) = endpoint_info {
            println!("  │   ├─ Provider: {}", e.provider);
            println!("  │   ├─ Base URL: {}", e.base_url);
            println!("  │   ├─ Model: {}", e.model);
            println!("  │   └─ API Key: {}",
                if e.api_key.is_empty() || e.api_key == "none" {
                    "❌ Not Set".to_string()
                } else {
                    "✅ Set".to_string()
                }
            );
        } else {
            println!("  │   └─ ⚠️  Endpoint not found!");
        }

        println!("  └─ Prompt: {}", p.prompt);
    } else {
        println!("  ⚠️  No profile selected!");
    }

    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    println!();
}

// ============================================================================
// PROFILE MANAGEMENT
// ============================================================================

/// Show profile management submenu
pub fn show_profiles_menu(_config: &Config) -> Result<ProfileMenuChoice> {
    let options = vec![
        ProfileMenuChoice::CreateProfile,
        ProfileMenuChoice::EditProfile,
        ProfileMenuChoice::DeleteProfile,
        ProfileMenuChoice::Back,
    ];

    let ans: Result<ProfileMenuChoice, _> = InquireSelect::new(
        "Manage Profiles",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(ProfileMenuChoice::Back),
    }
}

/// Handle profile creation wizard
pub fn handle_create_profile(config: &mut Config) -> Result<bool> {
    // Get profile name
    let name = Text::new("Profile name:").prompt()?;
    if name.trim().is_empty() {
        println!("⚠️  Profile name cannot be empty.");
        return Ok(false);
    }

    // Check for duplicate
    if config.profiles.iter().any(|p| p.name == name) {
        println!("⚠️  Profile '{}' already exists.", name);
        return Ok(false);
    }

    // Select endpoint
    let endpoint = select_endpoint(config)?;
    if endpoint.is_none() {
        println!("⚠️  Must select an endpoint to create a profile.");
        return Ok(false);
    }
    let endpoint = endpoint.unwrap();

    // Get model override (optional)
    let model = select_or_enter_model(config, &endpoint)?;

    // Get prompt
    let prompt = Text::new("Prompt name (without .md extension):")
        .with_default("default")
        .prompt()?;

    // Create and add profile
    config.profiles.push(mylm_core::config::Profile {
        name,
        endpoint,
        prompt,
        model,
    });

    println!("✅ Profile created successfully!");
    config.save_to_default_location()?;

    Ok(true)
}

/// Handle profile editing (Model/Prompt only - NOT Provider/Key)
pub fn handle_edit_profile(config: &mut Config) -> Result<bool> {
    let profiles: Vec<String> = config.profiles.iter().map(|p| p.name.clone()).collect();
    if profiles.is_empty() {
        println!("⚠️  No profiles to edit.");
        return Ok(false);
    }

    let profile_name = InquireSelect::new("Select profile to edit:", profiles).prompt()?;

    // Edit options for profile
    let edit_options = vec!["Model Override", "Prompt", "Back"];
    let edit_selection = InquireSelect::new("What to edit:", edit_options).prompt()?;

    match edit_selection {
        "Model Override" => {
            let profile = config.profiles.iter().find(|p| p.name == profile_name).unwrap();
            let new_model = select_or_enter_model(config, &profile.endpoint)?;

            if let Some(p) = config.profiles.iter_mut().find(|p| p.name == profile_name) {
                p.model = new_model;
            }
            println!("✅ Model updated!");
        }
        "Prompt" => {
            let new_prompt = Text::new("Prompt name (without .md extension):").prompt()?;

            if let Some(p) = config.profiles.iter_mut().find(|p| p.name == profile_name) {
                p.prompt = new_prompt;
            }
            println!("✅ Prompt updated!");
        }
        _ => return Ok(false),
    }

    config.save_to_default_location()?;
    Ok(true)
}

/// Handle profile deletion
pub fn handle_delete_profile(config: &mut Config) -> Result<bool> {
    let profiles: Vec<String> = config.profiles.iter().map(|p| p.name.clone()).collect();
    if profiles.is_empty() {
        println!("⚠️  No profiles to delete.");
        return Ok(false);
    }

    let profile_name = InquireSelect::new("Select profile to delete:", profiles).prompt()?;

    // Cannot delete active profile
    if profile_name == config.active_profile {
        println!("⚠️  Cannot delete the active profile. Switch to another profile first.");
        return Ok(false);
    }

    if Confirm::new()
        .with_prompt(format!("Delete profile '{}'?", profile_name))
        .default(false)
        .interact()?
    {
        if let Some(idx) = config.profiles.iter().position(|p| p.name == profile_name) {
            config.profiles.remove(idx);
        }
        println!("✅ Profile deleted.");
        config.save_to_default_location()?;
    }

    Ok(true)
}

// ============================================================================
// ENDPOINT MANAGEMENT
// ============================================================================

/// Show endpoint management submenu
pub fn show_endpoints_menu(_config: &Config) -> Result<EndpointMenuChoice> {
    let options = vec![
        EndpointMenuChoice::CreateEndpoint,
        EndpointMenuChoice::EditEndpoint,
        EndpointMenuChoice::DeleteEndpoint,
        EndpointMenuChoice::Back,
    ];

    let ans: Result<EndpointMenuChoice, _> = InquireSelect::new(
        "Manage Endpoints",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(EndpointMenuChoice::Back),
    }
}

/// Handle endpoint creation (full details: Provider, API Key, URL, Model)
pub async fn handle_create_endpoint(config: &mut Config) -> Result<bool> {
    let name = Text::new("Endpoint name:").prompt()?;
    if name.trim().is_empty() {
        println!("⚠️  Endpoint name cannot be empty.");
        return Ok(false);
    }

    if config.endpoints.iter().any(|e| e.name == name) {
        println!("⚠️  Endpoint '{}' already exists.", name);
        return Ok(false);
    }

    // Get provider
    let provider = select_provider()?;

    // Get base URL
    let base_url = Text::new("Base URL:")
        .with_initial_value(&get_default_url(&provider))
        .prompt()?;

    // Get API key
    let api_key = if provider != "Ollama" {
        Password::new()
            .with_prompt("API Key")
            .interact()?
    } else {
        "none".to_string()
    };

    // Get model
    let model = Text::new("Model name:").prompt()?;

    // Create endpoint
    config.endpoints.push(mylm_core::config::endpoints::EndpointConfig {
        name,
        provider,
        base_url,
        model,
        api_key,
        timeout_seconds: 60,
        input_price_per_1m: 0.0,
        output_price_per_1m: 0.0,
        max_context_tokens: 32768,
        condense_threshold: 0.8,
    });

    println!("✅ Endpoint created successfully!");
    config.save_to_default_location()?;

    Ok(true)
}

/// Handle endpoint editing (Provider, API Key, URL, Model)
pub async fn handle_edit_endpoint(config: &mut Config) -> Result<bool> {
    let endpoints: Vec<String> = config.endpoints.iter().map(|e| e.name.clone()).collect();
    if endpoints.is_empty() {
        println!("⚠️  No endpoints to edit.");
        return Ok(false);
    }

    let endpoint_name = InquireSelect::new("Select endpoint to edit:", endpoints).prompt()?;

    // Edit options
    let edit_options = vec!["Provider", "Base URL", "API Key", "Model", "Back"];
    let edit_selection = InquireSelect::new("What to edit:", edit_options).prompt()?;

    match edit_selection {
        "Provider" => {
            let new_provider = select_provider()?;
            if let Some(e) = config.endpoints.iter_mut().find(|ep| ep.name == endpoint_name) {
                e.provider = new_provider;
            }
            println!("✅ Provider updated!");
        }
        "Base URL" => {
            let new_url = Text::new("Base URL:").prompt()?;
            if let Some(e) = config.endpoints.iter_mut().find(|ep| ep.name == endpoint_name) {
                e.base_url = new_url;
            }
            println!("✅ Base URL updated!");
        }
        "API Key" => {
            let new_key = Password::new()
                .with_prompt("API Key (leave empty to keep existing)")
                .allow_empty_password(true)
                .interact()?;

            if !new_key.is_empty() {
                if let Some(e) = config.endpoints.iter_mut().find(|ep| ep.name == endpoint_name) {
                    e.api_key = new_key;
                }
            }
            println!("✅ API Key updated!");
        }
        "Model" => {
            let new_model = Text::new("Model name:").prompt()?;
            if let Some(e) = config.endpoints.iter_mut().find(|ep| ep.name == endpoint_name) {
                e.model = new_model;
            }
            println!("✅ Model updated!");
        }
        _ => return Ok(false),
    }

    config.save_to_default_location()?;
    Ok(true)
}

/// Handle endpoint deletion
pub fn handle_delete_endpoint(config: &mut Config) -> Result<bool> {
    let endpoints: Vec<String> = config.endpoints.iter().map(|e| e.name.clone()).collect();
    if endpoints.is_empty() {
        println!("⚠️  No endpoints to delete.");
        return Ok(false);
    }

    let endpoint_name = InquireSelect::new("Select endpoint to delete:", endpoints).prompt()?;

    // Check if any profile uses this endpoint
    let usage: Vec<&str> = config.profiles
        .iter()
        .filter(|p| p.endpoint == endpoint_name)
        .map(|p| p.name.as_str())
        .collect();

    if !usage.is_empty() {
        println!("⚠️  Endpoint '{}' is used by profiles: {:?}", endpoint_name, usage);
        println!("   Delete those profiles first, or re-link them to another endpoint.");
        return Ok(false);
    }

    if Confirm::new()
        .with_prompt(format!("Delete endpoint '{}'?", endpoint_name))
        .default(false)
        .interact()?
    {
        if let Some(idx) = config.endpoints.iter().position(|e| e.name == endpoint_name) {
            config.endpoints.remove(idx);
        }
        println!("✅ Endpoint deleted.");
        config.save_to_default_location()?;
    }

    Ok(true)
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Select an endpoint from available endpoints
fn select_endpoint(config: &Config) -> Result<Option<String>> {
    let endpoints: Vec<String> = config.endpoints.iter().map(|e| e.name.clone()).collect();

    if endpoints.is_empty() {
        println!("⚠️  No endpoints configured. Create an endpoint first.");
        return Ok(None);
    }

    let endpoint = InquireSelect::new("Select endpoint:", endpoints).prompt()?;
    Ok(Some(endpoint))
}

/// Select or enter a model for the given endpoint
fn select_or_enter_model(_config: &Config, _endpoint_name: &str) -> Result<Option<String>> {
    let model_options = vec!["Use endpoint default", "Enter model manually"];
    let selection = InquireSelect::new("Model:", model_options).prompt()?;

    if selection == "Use endpoint default" {
        Ok(None)
    } else {
        let model = Text::new("Model name:").prompt()?;
        Ok(Some(model))
    }
}

/// Select LLM provider
fn select_provider() -> Result<String> {
    let providers = vec!["OpenAI", "Google (Gemini)", "Ollama", "OpenRouter", "Custom"];
    let selection = InquireSelect::new("Select provider:", providers).prompt()?;

    match selection {
        "OpenAI" => Ok("openai".to_string()),
        "Google (Gemini)" => Ok("google".to_string()),
        "Ollama" => Ok("openai".to_string()), // Ollama uses OpenAI-compatible API
        "OpenRouter" => Ok("openrouter".to_string()),
        _ => Ok("openai".to_string()),
    }
}

/// Get default URL for a provider
fn get_default_url(provider: &str) -> String {
    match provider {
        "OpenAI" => "https://api.openai.com/v1".to_string(),
        "Google (Gemini)" => "https://generativelanguage.googleapis.com/v1".to_string(),
        "Ollama" => "http://localhost:11434/v1".to_string(),
        "OpenRouter" => "https://openrouter.ai/api/v1".to_string(),
        _ => "".to_string(),
    }
}

// ============================================================================
// BANNER & UTILS
// ============================================================================

async fn print_banner(config: &Config) {
    let blue = Style::new().blue().bold();
    let green = Style::new().green().bold();
    let cyan = Style::new().cyan();
    let dim = Style::new().dim();

    let banner = r#"
    __  ___  __  __  __     __  ___
   /  |/  / / / / / / /    /  |/  /
  / /|_/ / / / / / / /    / /|_/ /
 / /  / / / /____ / / / / / / / /
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
