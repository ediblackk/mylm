use anyhow::Result;
use console::Style;
use dialoguer::{Confirm, Password};
use inquire::{Select as InquireSelect, Text};
use mylm_core::config::Config;
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
    ManageProfiles,
    EndpointSetup,
    ToggleTmuxAutostart,
    WebSearch,
    GeneralSettings,
    Back,
}

/// Web search configuration submenu choices
#[derive(Debug, PartialEq)]
pub enum WebSearchMenuChoice {
    ToggleEnabled,
    SetProvider,
    SetApiKey,
    SetModel,
    Back,
}

impl std::fmt::Display for WebSearchMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebSearchMenuChoice::ToggleEnabled => write!(f, "✅ Toggle Enabled"),
            WebSearchMenuChoice::SetProvider => write!(f, "🧭 Set Provider"),
            WebSearchMenuChoice::SetApiKey => write!(f, "🔑 Set API Key"),
            WebSearchMenuChoice::SetModel => write!(f, "🧠 Set Model (Kimi only)"),
            WebSearchMenuChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

impl std::fmt::Display for SettingsMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsMenuChoice::ManageProfiles => write!(f, "📂 [1] Manage Profiles"),
            SettingsMenuChoice::EndpointSetup => write!(f, "🔌 [2] Endpoint Setup"),
            SettingsMenuChoice::ToggleTmuxAutostart => write!(f, "🔄 [3] Toggle Tmux Autostart"),
            SettingsMenuChoice::WebSearch => write!(f, "🌐 [4] Web Search"),
            SettingsMenuChoice::GeneralSettings => write!(f, "⚙️  [5] General Settings (Context, Memory, etc.)"),
            SettingsMenuChoice::Back => write!(f, "⬅️  [6] Back"),
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
            ProfileMenuChoice::EditProfile => write!(f, "✏️  [3] Edit Profile (Endpoint/Model/Prompt)"),
            ProfileMenuChoice::DeleteProfile => write!(f, "🗑️  [4] Delete Profile"),
            ProfileMenuChoice::Back => write!(f, "⬅️  [5] Back"),
        }
    }
}

/// Endpoint management submenu choices
#[derive(Debug, PartialEq)]
pub enum EndpointMenuChoice {
    SetActiveProfileEndpoint,
    SetDefaultEndpoint,
    CreateEndpoint,
    EditEndpoint,
    DeleteEndpoint,
    Back,
}

impl std::fmt::Display for EndpointMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointMenuChoice::SetActiveProfileEndpoint => write!(f, "🎯 [1] Set Active Profile Endpoint"),
            EndpointMenuChoice::SetDefaultEndpoint => write!(f, "🌍 [2] Set Global Default Endpoint (Fallback)"),
            EndpointMenuChoice::CreateEndpoint => write!(f, "➕ [3] Create New Endpoint"),
            EndpointMenuChoice::EditEndpoint => write!(f, "✏️  [4] Edit Endpoint (Provider/Key/URL)"),
            EndpointMenuChoice::DeleteEndpoint => write!(f, "🗑️  [5] Delete Endpoint"),
            EndpointMenuChoice::Back => write!(f, "⬅️  [6] Back"),
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

fn inquire_default_index(options: &[String], current: Option<&str>) -> usize {
    let Some(cur) = current else {
        return 0;
    };
    options.iter().position(|x| x == cur).unwrap_or(0)
}

// ============================================================================
// SETTINGS DASHBOARD - Main Configuration Menu
// ============================================================================

/// Main settings dashboard - presents a clean Menu System
pub fn show_settings_dashboard(config: &Config) -> Result<SettingsMenuChoice> {
    // Display current state (Profile + linked Endpoint info)
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
    let enabled = if config.web_search.enabled { "On" } else { "Off" };
    let provider = if config.web_search.provider.trim().is_empty() {
        "(unset)"
    } else {
        config.web_search.provider.as_str()
    };
    let key_status = if config.web_search.api_key.trim().is_empty() {
        "Not set"
    } else {
        "Set"
    };
    let model = if config.web_search.model.trim().is_empty() {
        "(unset)"
    } else {
        config.web_search.model.as_str()
    };

    println!("\n🌐 Web Search Settings");
    println!("  Enabled:   {}", enabled);
    println!("  Provider:  {}", provider);
    println!("  API Key:   {}", key_status);
    println!("  Model:     {}", model);
    println!();

    let options = vec![
        WebSearchMenuChoice::ToggleEnabled,
        WebSearchMenuChoice::SetProvider,
        WebSearchMenuChoice::SetApiKey,
        WebSearchMenuChoice::SetModel,
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
    let profile = config.get_active_profile();
    let profile_name = &config.active_profile;

    // Effective endpoint = profile-linked endpoint (if set), otherwise global default (fallback).
    // Note: Config::get_endpoint(None) has additional fallback behavior (single endpoint convenience);
    // for the UI, we label the intent explicitly.
    let (effective_endpoint_name, effective_source_label) = if let Some(p) = profile {
        if !p.endpoint.is_empty() {
            (p.endpoint.clone(), "profile-linked")
        } else if !config.default_endpoint.is_empty() {
            (config.default_endpoint.clone(), "global default")
        } else {
            ("(none)".to_string(), "unconfigured")
        }
    } else if !config.default_endpoint.is_empty() {
        (config.default_endpoint.clone(), "global default")
    } else {
        ("(none)".to_string(), "unconfigured")
    };

    println!("\n📊 Current Configuration");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));

    if let Some(p) = profile {
        // Get the linked endpoint - if profile has no endpoint, fallback to default
        let endpoint_info = config.get_endpoint(if p.endpoint.is_empty() { None } else { Some(&p.endpoint) }).ok();

        println!("  Profile: {}", Style::new().green().bold().apply_to(&profile_name));
        println!("  ├─ Active Profile Endpoint: {}", if p.endpoint.is_empty() { "(uses global default)" } else { &p.endpoint });
        println!("  ├─ Global Default Endpoint: {}", if config.default_endpoint.is_empty() { "(none)" } else { &config.default_endpoint });
        println!("  ├─ Effective Endpoint: {} ({})", effective_endpoint_name, effective_source_label);

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
        println!("  ├─ Global Default Endpoint: {}", if config.default_endpoint.is_empty() { "(none)" } else { &config.default_endpoint });
        println!("  └─ Effective Endpoint: {} ({})", effective_endpoint_name, effective_source_label);
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
    if config.profiles.iter().any(|p| p.name == name) {
        println!("⚠️  Profile '{}' already exists.", name);
        return Ok(false);
    }

    // Select endpoint
    let endpoint = if config.endpoints.is_empty() {
        if Confirm::new()
            .with_prompt("No endpoints defined. Create one now?")
            .default(true)
            .interact()?
        {
            // Inline endpoint creation
            // We need to release the mutable borrow to call handle_create_endpoint
            // BUT handle_create_endpoint takes &mut Config.
            // Since we are inside a function taking &mut Config, we can call it.
            // However, we need to handle the return value and then re-check endpoints.
            handle_create_endpoint(config).await?;
            // Now try selecting again
            select_endpoint(config)?
        } else {
             println!("⚠️  Creating profile without an endpoint. You must link one later.");
             None
        }
    } else {
        select_endpoint(config)?
    };

    let endpoint_name = endpoint.unwrap_or_default();

    // Get model override (optional)
    // If we have no endpoint, we can't fetch models, so manual entry only or skip
    let model = if !endpoint_name.is_empty() {
        select_or_enter_model(config, &endpoint_name)?
    } else {
        None
    };

    // Get prompt
    let prompt = Text::new("Prompt name (without .md extension):")
        .with_default("default")
        .prompt()?;

    // Create and add profile
    config.profiles.push(mylm_core::config::Profile {
        name,
        endpoint: endpoint_name,
        prompt,
        model,
    });

    println!("✅ Profile created successfully!");
    config.save_to_default_location()?;

    Ok(true)
}

/// Handle profile editing (Endpoint/Model/Prompt)
pub fn handle_edit_profile(config: &mut Config) -> Result<bool> {
    let profiles: Vec<String> = config.profiles.iter().map(|p| p.name.clone()).collect();
    if profiles.is_empty() {
        println!("⚠️  No profiles to edit.");
        return Ok(false);
    }

    let profile_name = InquireSelect::new("Select profile to edit:", profiles).prompt()?;

    // Edit options for profile
    let edit_options = vec!["Endpoint", "Model Override", "Prompt", "Back"];
    let edit_selection = InquireSelect::new("What to edit:", edit_options).prompt()?;

    match edit_selection {
        "Endpoint" => {
            let new_endpoint = select_endpoint(config)?;

            if let Some(p) = config.profiles.iter_mut().find(|p| p.name == profile_name) {
                p.endpoint = new_endpoint.unwrap_or_default();
            }
            println!("✅ Endpoint updated!");
        }
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
        EndpointMenuChoice::SetActiveProfileEndpoint,
        EndpointMenuChoice::SetDefaultEndpoint,
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
    let (provider_id, provider_display) = select_provider()?;

    // Get base URL
    // Skip URL prompt for known providers where URL is fixed
    let base_url = if ["OpenAI", "Google (Gemini)", "OpenRouter"].contains(&provider_display.as_str()) {
        get_default_url(&provider_display)
    } else {
        Text::new("Base URL:")
            .with_initial_value(&get_default_url(&provider_display))
            .prompt()?
    };

    // Get API key
    let api_key = if provider_display != "Ollama" {
        Password::new()
            .with_prompt("API Key")
            .interact()?
    } else {
        "none".to_string()
    };

    // Get model
    let method = InquireSelect::new("Model Selection Method:", vec!["Fetch from API", "Enter Manually"]).prompt()?;
    
    let model = if method == "Fetch from API" {
        match fetch_models(&base_url, &api_key).await {
            Ok(models) => {
                InquireSelect::new("Select Model:", models).prompt()?
            }
            Err(e) => {
                println!("⚠️  Failed to fetch models: {}. Falling back to manual entry.", e);
                Text::new("Model name:").prompt()?
            }
        }
    } else {
        Text::new("Model name:").prompt()?
    };

    // Create endpoint
    config.endpoints.push(mylm_core::config::endpoints::EndpointConfig {
        name: name.clone(),
        provider: provider_id,
        base_url,
        model,
        api_key,
        timeout_seconds: 60,
        input_price_per_1m: 0.0,
        output_price_per_1m: 0.0,
        max_context_tokens: 32768,
        condense_threshold: 0.8,
    });

    // If this is the first endpoint or no default is set, make it the default
    if config.default_endpoint.is_empty() {
        config.default_endpoint = name.clone();
    }

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
            let (new_provider_id, new_provider_display) = select_provider()?;
            if let Some(e) = config.endpoints.iter_mut().find(|ep| ep.name == endpoint_name) {
                e.provider = new_provider_id;
                
                // Auto-update Base URL if switching to a known provider
                if ["OpenAI", "Google (Gemini)", "OpenRouter"].contains(&new_provider_display.as_str()) {
                     e.base_url = get_default_url(&new_provider_display);
                     println!("ℹ️  Base URL automatically updated to {}", e.base_url);
                }
            }
            println!("✅ Provider updated!");
        }
        "Base URL" => {
            let current_url = config.endpoints.iter().find(|e| e.name == endpoint_name).map(|e| e.base_url.clone()).unwrap_or_default();
            let new_url = Text::new("Base URL:")
                .with_initial_value(&current_url)
                .prompt()?;
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
            let method = InquireSelect::new("Model Selection Method:", vec!["Fetch from API", "Enter Manually"]).prompt()?;
            
            let new_model = if method == "Fetch from API" {
                // We need the endpoint's current config to fetch
                let endpoint = config.endpoints.iter().find(|e| e.name == endpoint_name).unwrap();
                match fetch_models(&endpoint.base_url, &endpoint.api_key).await {
                    Ok(models) => InquireSelect::new("Select Model:", models).prompt()?,
                    Err(e) => {
                        println!("⚠️  Failed to fetch models: {}. Falling back to manual entry.", e);
                        Text::new("Model name:").prompt()?
                    }
                }
            } else {
                Text::new("Model name:").prompt()?
            };
            
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
pub fn select_endpoint(config: &Config) -> Result<Option<String>> {
    let mut endpoints: Vec<String> = config.endpoints.iter().map(|e| e.name.clone()).collect();

    if endpoints.is_empty() {
        return Ok(None);
    }
    
    // Add option to skip linking
    endpoints.push("(Skip / Link Later)".to_string());

    // Preselect effective endpoint if possible: active profile endpoint, else global default.
    let current = config
        .get_active_profile()
        .map(|p| p.endpoint.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if config.default_endpoint.is_empty() {
                None
            } else {
                Some(config.default_endpoint.as_str())
            }
        });

    let default_idx = inquire_default_index(&endpoints, current);
    let endpoint = InquireSelect::new("Select endpoint:", endpoints)
        .with_starting_cursor(default_idx)
        .prompt()?;
    
    if endpoint == "(Skip / Link Later)" {
        Ok(None)
    } else {
        Ok(Some(endpoint))
    }
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
fn select_provider() -> Result<(String, String)> {
    let providers = vec!["OpenAI", "Google (Gemini)", "Ollama", "OpenRouter", "Custom"];
    let selection = InquireSelect::new("Select provider:", providers).prompt()?;

    match selection {
        "OpenAI" => Ok(("openai".to_string(), "OpenAI".to_string())),
        "Google (Gemini)" => Ok(("google".to_string(), "Google (Gemini)".to_string())),
        "Ollama" => Ok(("openai".to_string(), "Ollama".to_string())), // Ollama uses OpenAI-compatible API
        "OpenRouter" => Ok(("openrouter".to_string(), "OpenRouter".to_string())),
        _ => Ok(("openai".to_string(), "Custom".to_string())),
    }
}

/// Get default URL for a provider
fn get_default_url(provider_display_name: &str) -> String {
    match provider_display_name {
        "OpenAI" => "https://api.openai.com/v1".to_string(),
        "Google (Gemini)" => "https://generativelanguage.googleapis.com/v1".to_string(),
        "Ollama" => "http://localhost:11434/v1".to_string(),
        "OpenRouter" => "https://openrouter.ai/api/v1".to_string(),
        _ => "".to_string(),
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
    let profile = config.get_active_profile();
    // Use the safe fallback we added to get_endpoint or just try get_endpoint(None)
    // We already made get_endpoint return Result.
    let endpoint = config.get_endpoint(None).ok();

    let profile_name = if config.active_profile.is_empty() {
        "None"
    } else {
        &config.active_profile
    };

    let llm_info = match (profile, endpoint) {
        (Some(_), Some(e)) => format!("{} ({} / {})", e.name, e.provider, e.model),
        (Some(p), None) => format!("Profile: {} (Missing Endpoint: '{}')", p.name, if p.endpoint.is_empty() { "Default" } else { &p.endpoint }),
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
