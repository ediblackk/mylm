use anyhow::Result;
use console::Style;
use dialoguer::{Confirm, Password};
use inquire::{Select as InquireSelect, Text};
use mylm_core::config::{Config, ConfigUiExt, Provider, Profile};
use serde_json::Value;

/// Main hub choice enum
#[derive(Debug, PartialEq)]
pub enum HubChoice {
    PopTerminal,
    PopTerminalMissing,
    ResumeSession,
    StartTui,
    QuickQuery,
    ManageSessions,
    BackgroundJobs,
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
            HubChoice::BackgroundJobs => write!(f, "🕒 Background Jobs"),
            HubChoice::Exit => write!(f, "❌ Exit"),
        }
    }
}

/// Settings dashboard main menu choices
#[derive(Debug, PartialEq)]
pub enum SettingsMenuChoice {
    ManageProfiles,
    EndpointSetup,
    ToggleTmuxAutostart,
    WebSearch,
    GeneralSettings,
    Back,
}

impl std::fmt::Display for SettingsMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsMenuChoice::ManageProfiles => write!(f, "📂 [1] Manage Profiles"),
            SettingsMenuChoice::EndpointSetup => write!(f, "🔌 [2] Endpoint Setup"),
            SettingsMenuChoice::ToggleTmuxAutostart => write!(f, "🔄 [3] Toggle Tmux Autostart"),
            SettingsMenuChoice::WebSearch => write!(f, "🌐 [4] Web Search"),
            SettingsMenuChoice::GeneralSettings => write!(f, "⚙️  [5] General Settings"),
            SettingsMenuChoice::Back => write!(f, "⬅️  [6] Back"),
        }
    }
}

/// Web search configuration submenu choices
#[derive(Debug, PartialEq)]
pub enum WebSearchMenuChoice {
    ToggleEnabled,
    SetProvider,
    SetApiKey,
    Back,
}

impl std::fmt::Display for WebSearchMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebSearchMenuChoice::ToggleEnabled => write!(f, "✅ Toggle Enabled"),
            WebSearchMenuChoice::SetProvider => write!(f, "🧭 Set Provider"),
            WebSearchMenuChoice::SetApiKey => write!(f, "🔑 Set API Key"),
            WebSearchMenuChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Profile management submenu choices
#[derive(Debug, PartialEq)]
pub enum ProfileMenuChoice {
    SelectProfile,
    CreateProfile,
    EditProfile,
    DeleteProfile,
    Back,
}

impl std::fmt::Display for ProfileMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileMenuChoice::SelectProfile => write!(f, "👤 [1] Select Profile"),
            ProfileMenuChoice::CreateProfile => write!(f, "➕ [2] Create New Profile"),
            ProfileMenuChoice::EditProfile => write!(f, "✏️  [3] Edit Profile Overrides"),
            ProfileMenuChoice::DeleteProfile => write!(f, "🗑️  [4] Delete Profile"),
            ProfileMenuChoice::Back => write!(f, "⬅️  [5] Back"),
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
        HubChoice::BackgroundJobs,
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
    // Display current state
    display_current_config(config);

    let options = vec![
        SettingsMenuChoice::ManageProfiles,
        SettingsMenuChoice::EndpointSetup,
        SettingsMenuChoice::ToggleTmuxAutostart,
        SettingsMenuChoice::WebSearch,
        SettingsMenuChoice::GeneralSettings,
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

pub fn show_web_search_menu(config: &Config) -> Result<WebSearchMenuChoice> {
    let enabled = if config.features.web_search.enabled { "On" } else { "Off" };
    let provider = match config.features.web_search.provider {
        mylm_core::config::SearchProvider::Kimi => "Kimi",
        mylm_core::config::SearchProvider::Serpapi => "SerpApi",
        mylm_core::config::SearchProvider::Brave => "Brave",
    };
    let key_status = if config.features.web_search.api_key.as_ref().is_none_or(|k| k.is_empty()) {
        "Not set"
    } else {
        "Set"
    };

    println!("\n🌐 Web Search Settings");
    println!("  Enabled:   {}", enabled);
    println!("  Provider:  {}", provider);
    println!("  API Key:   {}", key_status);
    println!();

    let options = vec![
        WebSearchMenuChoice::ToggleEnabled,
        WebSearchMenuChoice::SetProvider,
        WebSearchMenuChoice::SetApiKey,
        WebSearchMenuChoice::Back,
    ];

    let ans: Result<WebSearchMenuChoice, _> = InquireSelect::new("Web Search", options).prompt();
    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(WebSearchMenuChoice::Back),
    }
}

/// Display current configuration summary
fn display_current_config(config: &Config) {
    let profile_name = &config.profile;
    let endpoint_info = config.get_endpoint_info();
    let effective_info = config.get_effective_endpoint_info();
    let profile_info = config.get_active_profile_info();

    println!("\n📊 Current Configuration");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));

    println!("  Active Profile: {}", Style::new().green().bold().apply_to(profile_name));
    
    if let Some(ref p) = profile_info {
        if let Some(ref model) = p.model_override {
            println!("  ├─ Model Override: {}", model);
        }
        if let Some(iters) = p.max_iterations {
            println!("  ├─ Max Iterations: {}", iters);
        }
    }

    println!("\n  Base Endpoint:");
    println!("  ├─ Provider: {}", endpoint_info.provider);
    println!("  ├─ Base URL: {}", endpoint_info.base_url);
    println!("  ├─ Model: {}", endpoint_info.model);
    println!("  ├─ API Key: {}", 
        if endpoint_info.api_key_set { "✅ Set" } else { "❌ Not Set" }
    );

    println!("\n  Effective Configuration (Profile Applied):");
    println!("  ├─ Model: {}", effective_info.model);
    println!("  ├─ Provider: {}", effective_info.provider);
    println!("  └─ API Key: {}", 
        if effective_info.api_key_set { "✅ Set" } else { "❌ Not Set" }
    );

    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    println!();
}

// ============================================================================
// PROFILE MANAGEMENT
// ============================================================================

/// Show profile management submenu
pub fn show_profiles_menu(_config: &Config) -> Result<ProfileMenuChoice> {
    let options = vec![
        ProfileMenuChoice::SelectProfile,
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
pub async fn handle_create_profile(config: &mut Config) -> Result<bool> {
    // Get profile name
    let name = Text::new("Profile name:").prompt()?;
    if name.trim().is_empty() {
        println!("⚠️  Profile name cannot be empty.");
        return Ok(false);
    }

    // Check for duplicate
    if config.profiles.contains_key(&name) {
        println!("⚠️  Profile '{}' already exists.", name);
        return Ok(false);
    }

    // Ask for model override
    let model_choice = InquireSelect::new(
        "Model override:",
        vec!["Use base endpoint model", "Override with specific model", "Skip for now"]
    ).prompt()?;

    let model_override = match model_choice {
        "Override with specific model" => {
            let model = Text::new("Model name:").prompt()?;
            if model.trim().is_empty() { None } else { Some(model) }
        }
        _ => None,
    };

    // Ask for max_iterations override
    let iter_choice = InquireSelect::new(
        "Max iterations override:",
        vec!["Use default (10)", "Override with custom value", "Skip for now"]
    ).prompt()?;

    let max_iterations = match iter_choice {
        "Override with custom value" => {
            let iters: String = Text::new("Max iterations:").prompt()?;
            iters.parse::<usize>().ok()
        }
        _ => None,
    };

    // Create profile with overrides
    let mut profile = Profile::default();
    
    if model_override.is_some() || max_iterations.is_some() {
        profile.endpoint = model_override.clone().map(|m| mylm_core::config::EndpointOverride {
            model: Some(m),
            api_key: None,
        });
        profile.agent = max_iterations.map(|i| mylm_core::config::AgentOverride {
            max_iterations: Some(i),
            main_model: None,
            worker_model: None,
        });
    }

    config.profiles.insert(name.clone(), profile);

    println!("✅ Profile '{}' created successfully!", name);
    config.save_to_default_location()?;

    Ok(true)
}

/// Handle profile editing
pub fn handle_edit_profile(config: &mut Config) -> Result<bool> {
    let profiles: Vec<String> = config.profile_names();
    if profiles.is_empty() {
        println!("⚠️  No profiles to edit.");
        return Ok(false);
    }

    let profile_name = InquireSelect::new("Select profile to edit:", profiles).prompt()?;

    // Edit options for profile
    let edit_options = vec!["Model Override", "Max Iterations", "Back"];
    let edit_selection = InquireSelect::new("What to edit:", edit_options).prompt()?;

    match edit_selection {
        "Model Override" => {
            let current = config.get_profile_info(&profile_name)
                .and_then(|p| p.model_override)
                .unwrap_or_else(|| "(none)".to_string());
            
            println!("Current model override: {}", current);
            
            let new_model: String = Text::new("New model override (empty to clear):")
                .with_initial_value(&current)
                .prompt()?;
            
            let override_value = if new_model.trim().is_empty() || new_model == "(none)" {
                None
            } else {
                Some(new_model)
            };
            
            config.set_profile_model_override(&profile_name, override_value)?;
            println!("✅ Model override updated!");
        }
        "Max Iterations" => {
            let current = config.get_profile_info(&profile_name)
                .and_then(|p| p.max_iterations.map(|i| i.to_string()))
                .unwrap_or_else(|| "(default)".to_string());
            
            println!("Current max iterations: {}", current);
            
            let new_iters: String = Text::new("New max iterations (empty to clear):")
                .with_initial_value(&current)
                .prompt()?;
            
            let override_value = if new_iters.trim().is_empty() || new_iters == "(default)" {
                None
            } else {
                new_iters.parse::<usize>().ok()
            };
            
            config.set_profile_max_iterations(&profile_name, override_value)?;
            println!("✅ Max iterations updated!");
        }
        _ => return Ok(false),
    }

    config.save_to_default_location()?;
    Ok(true)
}

/// Handle profile deletion
pub fn handle_delete_profile(config: &mut Config) -> Result<bool> {
    let profiles: Vec<String> = config.profile_names();
    if profiles.is_empty() {
        println!("⚠️  No profiles to delete.");
        return Ok(false);
    }

    let profile_name = InquireSelect::new("Select profile to delete:", profiles).prompt()?;

    // Cannot delete active profile
    if profile_name == config.profile {
        println!("⚠️  Cannot delete the active profile. Switch to another profile first.");
        return Ok(false);
    }

    if Confirm::new()
        .with_prompt(format!("Delete profile '{}'?", profile_name))
        .default(false)
        .interact()?
    {
        config.delete_profile(&profile_name)?;
        println!("✅ Profile deleted.");
        config.save_to_default_location()?;
    }

    Ok(true)
}

// ============================================================================
// ENDPOINT SETUP (V2 - Single Base Endpoint)
// ============================================================================

/// Handle base endpoint configuration (V2)
pub async fn handle_setup_endpoint(config: &mut Config) -> Result<bool> {
    let current = config.get_endpoint_info();
    
    println!("\n🔌 Endpoint Configuration");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    println!("Current settings:");
    println!("  Provider: {}", current.provider);
    println!("  Model: {}", current.model);
    println!("  Base URL: {}", current.base_url);
    println!("  API Key: {}", if current.api_key_set { "✅ Set" } else { "❌ Not Set" });
    println!();

    let options = vec![
        "Change Provider",
        "Change Model",
        "Change Base URL",
        "Change API Key",
        "Test Connection",
        "Back",
    ];

    loop {
        let choice = InquireSelect::new("Endpoint Setup:", options.clone()).prompt()?;

        match choice {
            "Change Provider" => {
                let (provider_id, provider_display) = select_provider()?;
                if let Ok(provider) = provider_id.parse::<Provider>() {
                    // Auto-update base URL when provider changes
                    let new_url = provider.default_url();
                    config.endpoint.provider = provider;
                    config.endpoint.base_url = Some(new_url);
                    println!("✅ Provider updated to {}", provider_display);
                    config.save_to_default_location()?;
                }
            }
            "Change Model" => {
                let method = InquireSelect::new("Model Selection:", vec!["Fetch from API", "Enter Manually"]).prompt()?;
                
                let new_model = if method == "Fetch from API" && config.get_endpoint_info().api_key_set {
                    let base_url = config.endpoint.base_url.clone()
                        .unwrap_or_else(|| config.endpoint.provider.default_url());
                    let api_key = config.endpoint.api_key.clone().unwrap_or_default();
                    
                    match fetch_models(&base_url, &api_key).await {
                        Ok(models) => {
                            InquireSelect::new("Select Model:", models).prompt()?
                        }
                        Err(e) => {
                            println!("⚠️  Failed to fetch models: {}. Falling back to manual entry.", e);
                            Text::new("Model name:").with_initial_value(&config.endpoint.model).prompt()?
                        }
                    }
                } else {
                    Text::new("Model name:").with_initial_value(&config.endpoint.model).prompt()?
                };
                
                config.endpoint.model = new_model;
                println!("✅ Model updated!");
                config.save_to_default_location()?;
            }
            "Change Base URL" => {
                let current_url = config.endpoint.base_url.clone()
                    .unwrap_or_else(|| config.endpoint.provider.default_url());
                let new_url = Text::new("Base URL:")
                    .with_initial_value(&current_url)
                    .prompt()?;
                config.endpoint.base_url = Some(new_url);
                println!("✅ Base URL updated!");
                config.save_to_default_location()?;
            }
            "Change API Key" => {
                let new_key = Password::new()
                    .with_prompt("API Key (leave empty to clear)")
                    .allow_empty_password(true)
                    .interact()?;
                
                config.endpoint.api_key = if new_key.is_empty() { None } else { Some(new_key) };
                println!("✅ API Key updated!");
                config.save_to_default_location()?;
            }
            "Test Connection" => {
                let base_url = config.endpoint.base_url.clone()
                    .unwrap_or_else(|| config.endpoint.provider.default_url());
                let api_key = config.endpoint.api_key.clone().unwrap_or_default();
                
                println!("🔄 Testing connection to {}...", base_url);
                match fetch_models(&base_url, &api_key).await {
                    Ok(models) => {
                        println!("✅ Connection successful! Found {} models.", models.len());
                    }
                    Err(e) => {
                        println!("❌ Connection failed: {}", e);
                    }
                }
            }
            _ => break,
        }
    }

    Ok(true)
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Select LLM provider
fn select_provider() -> Result<(String, String)> {
    let providers = vec!["OpenAI", "Google (Gemini)", "Ollama", "OpenRouter", "Kimi (Moonshot)", "Custom"];
    let selection = InquireSelect::new("Select provider:", providers).prompt()?;

    match selection {
        "OpenAI" => Ok(("openai".to_string(), "OpenAI".to_string())),
        "Google (Gemini)" => Ok(("google".to_string(), "Google (Gemini)".to_string())),
        "Ollama" => Ok(("ollama".to_string(), "Ollama".to_string())),
        "OpenRouter" => Ok(("openrouter".to_string(), "OpenRouter".to_string())),
        "Kimi (Moonshot)" => Ok(("kimi".to_string(), "Kimi (Moonshot)".to_string())),
        _ => Ok(("custom".to_string(), "Custom".to_string())),
    }
}

/// Fetch models from the API
async fn fetch_models(base_url: &str, api_key: &str) -> Result<Vec<String>> {
    println!("🔄 Fetching models from {}...", base_url);
    let client = reqwest::Client::new();
    
    // Construct URL - handle trailing slashes and ensure /models is attached correctly
    let url = if base_url.ends_with('/') {
        format!("{}models", base_url)
    } else {
        format!("{}/models", base_url)
    };

    let mut request = client.get(&url);

    if !api_key.is_empty() && api_key != "none" {
        request = request.header("Authorization", format!("Bearer {}", api_key));
    }
    
    // Add User-Agent as some APIs require it
    request = request.header("User-Agent", "mylm-cli/0.1.0");

    let response = request.send().await?;
    
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("API request failed with status: {}", response.status()));
    }

    let body: Value = response.json().await?;
    
    let mut models = Vec::new();
    if let Some(data) = body.get("data").and_then(|v| v.as_array()) {
        for model in data {
            if let Some(id) = model.get("id").and_then(|v| v.as_str()) {
                models.push(id.to_string());
            }
        }
    }
    
    if models.is_empty() {
        // Fallback: Check if top level has "models" key (some proxies)
        if let Some(data) = body.get("models").and_then(|v| v.as_array()) {
             for model in data {
                if let Some(id) = model.get("id").and_then(|v| v.as_str()) {
                    models.push(id.to_string());
                }
            }
        }
    }

    if models.is_empty() {
        return Err(anyhow::anyhow!("No models found in response"));
    }
    
    models.sort();
    Ok(models)
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
    let effective = config.get_effective_endpoint_info();
    let profile_name = &config.profile;

    let llm_info = if config.is_initialized() {
        format!("{} @ {} ({})", effective.model, effective.provider, profile_name)
    } else {
        "Not Configured".to_string()
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
        dim.apply_to("Model:"), green.apply_to(llm_info)
    );
    println!("  {} [{}] | {} [{}] | {} [{}]",
        dim.apply_to("Context:"), cyan.apply_to(cwd),
        dim.apply_to("Git:"), cyan.apply_to(branch),
        dim.apply_to("Tmux:"), tmux_status
    );
    println!("  {}", dim.apply_to("-".repeat(60).as_str()));
}
