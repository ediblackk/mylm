use anyhow::Result;
use console::Style;
use dialoguer::Password;
use inquire::{Select as InquireSelect, Text};
use mylm_core::config::{Config, ConfigUiExt, Provider};
use serde_json::Value;

/// Main hub choice enum
#[derive(Debug, PartialEq)]
pub enum HubChoice {
    PopTerminal,
    PopTerminalMissing,
    ResumeSession,
    StartTui,
    StartIncognito,
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
                    write!(f, "🚀 Pop Terminal (tmux)")
                } else {
                    write!(f, "🚀 Pop Terminal (no tmux)")
                }
            },
            HubChoice::PopTerminalMissing => write!(f, "🚀 Pop Terminal (install tmux)"),
            HubChoice::ResumeSession => write!(f, "🔄 Resume Session"),
            HubChoice::StartTui => write!(f, "✨ TUI Session"),
            HubChoice::StartIncognito => write!(f, "🕵️  Incognito"),
            HubChoice::QuickQuery => write!(f, "⚡ Quick Query"),
            HubChoice::Configuration => write!(f, "⚙️  Config"),
            HubChoice::ManageSessions => write!(f, "📂 Sessions"),
            HubChoice::BackgroundJobs => write!(f, "🕒 Jobs"),
            HubChoice::Exit => write!(f, "❌ Exit"),
        }
    }
}

/// Settings dashboard main menu choices
#[derive(Debug, PartialEq)]
pub enum SettingsMenuChoice {
    ManageProviders,     // Add/Edit/Remove providers
    SelectMainModel,     // Choose provider + model
    SelectWorkerModel,   // Choose provider + model for worker
    WebSearchSettings,   // Web search provider config
    AgentSettings,       // Max iterations, tmux, etc
    Back,
}

impl std::fmt::Display for SettingsMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsMenuChoice::ManageProviders => write!(f, "🔌 [1] Manage Providers"),
            SettingsMenuChoice::SelectMainModel => write!(f, "🧠 [2] Select Main LLM"),
            SettingsMenuChoice::SelectWorkerModel => write!(f, "⚡ [3] Select Worker Model"),
            SettingsMenuChoice::WebSearchSettings => write!(f, "🌐 [4] Web Search"),
            SettingsMenuChoice::AgentSettings => write!(f, "⚙️  [5] Agent Settings"),
            SettingsMenuChoice::Back => write!(f, "⬅️  [6] Back"),
        }
    }
}

/// Provider management submenu
#[derive(Debug, PartialEq)]
pub enum ProviderMenuChoice {
    AddProvider,
    EditProvider,
    RemoveProvider,
    Back,
}

impl std::fmt::Display for ProviderMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderMenuChoice::AddProvider => write!(f, "➕ Add Provider"),
            ProviderMenuChoice::EditProvider => write!(f, "✏️  Edit Provider"),
            ProviderMenuChoice::RemoveProvider => write!(f, "🗑️  Remove Provider"),
            ProviderMenuChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Web search settings submenu
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

/// Agent settings submenu
#[derive(Debug, PartialEq)]
pub enum AgentSettingsChoice {
    IterationsSettings,
    ToggleTmuxAutostart,
    PaCoReSettings,
    Back,
}

impl std::fmt::Display for AgentSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentSettingsChoice::IterationsSettings => write!(f, "🔁 Iterations Settings"),
            AgentSettingsChoice::ToggleTmuxAutostart => write!(f, "🔄 Toggle Tmux Autostart"),
            AgentSettingsChoice::PaCoReSettings => write!(f, "⚡ PaCoRe Settings"),
            AgentSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Iterations settings submenu
#[derive(Debug, PartialEq)]
pub enum IterationsSettingsChoice {
    SetMaxIterations,
    SetRateLimit,
    Back,
}

impl std::fmt::Display for IterationsSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IterationsSettingsChoice::SetMaxIterations => write!(f, "🔢 Set Max Iterations"),
            IterationsSettingsChoice::SetRateLimit => write!(f, "⏱️  Set Rate Limit (ms)"),
            IterationsSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// PaCoRe settings submenu
#[derive(Debug, PartialEq)]
pub enum PaCoReSettingsChoice {
    TogglePaCoRe,
    SetPaCoReRounds,
    Back,
}

impl std::fmt::Display for PaCoReSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaCoReSettingsChoice::TogglePaCoRe => write!(f, "⚡ Toggle PaCoRe"),
            PaCoReSettingsChoice::SetPaCoReRounds => write!(f, "📊 Set PaCoRe Rounds"),
            PaCoReSettingsChoice::Back => write!(f, "⬅️  Back"),
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
    // Clear the loading line, then use alternate screen buffer
    print!("\r\x1B[K");  // Clear current line
    print!("\x1B[?1049h");  // Enter alternate screen
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

    let ans: Result<HubChoice, _> = InquireSelect::new("Welcome to mylm! What would you like to do?", options)
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

    let ans: Result<String, _> = InquireSelect::new("Select Session to Resume", options).prompt();

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

    let ans: Result<SettingsMenuChoice, _> = InquireSelect::new(
        "Configuration Menu",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(SettingsMenuChoice::Back),
    }
}

/// Display compact configuration summary
fn display_config_summary(config: &Config) {
    let effective = config.resolve_profile();
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let dim = Style::new().dim();
    
    // Single line status bar
    let ws_status = if config.features.web_search.enabled { "🌐" } else { "·" };
    let tmux_status = if config.tmux_autostart { "🔄" } else { "·" };
    let pacore_status = if config.features.pacore.enabled { format!("⚡{}", config.features.pacore.rounds) } else { "·".to_string() };
    let rate_display = if effective.agent.iteration_rate_limit > 0 { format!("⏱️{}", effective.agent.iteration_rate_limit) } else { "·".to_string() };
    
    println!("  {} {} {} {} {} {} {} {} {} {} {} {}",
        dim.apply_to("Iter:"), yellow.apply_to(format!("{}", effective.agent.max_iterations)),
        dim.apply_to("│"), rate_display,
        dim.apply_to("│ Web:"), ws_status,
        dim.apply_to("│ Tmux:"), tmux_status,
        dim.apply_to("│ PaCoRe:"), pacore_status,
        dim.apply_to("│ Key:"), if effective.api_key.is_some() { green.apply_to("✓") } else { Style::new().red().apply_to("✗") }
    );
    println!();
}

// ============================================================================
// PROVIDER MANAGEMENT
// ============================================================================

/// Show provider management menu
pub fn show_provider_menu() -> Result<ProviderMenuChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;
    
    let options = vec![
        ProviderMenuChoice::AddProvider,
        ProviderMenuChoice::EditProvider,
        ProviderMenuChoice::RemoveProvider,
        ProviderMenuChoice::Back,
    ];

    let ans: Result<ProviderMenuChoice, _> = InquireSelect::new(
        "Manage Providers",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(ProviderMenuChoice::Back),
    }
}

/// Provider preset information
struct ProviderPreset {
    name: &'static str,
    display_name: &'static str,
    base_url: &'static str,
    provider_type: Provider,
    api_key_required: bool,
}

fn get_provider_presets() -> Vec<ProviderPreset> {
    vec![
        ProviderPreset {
            name: "openai",
            display_name: "🟢 OpenAI",
            base_url: "https://api.openai.com/v1",
            provider_type: Provider::Openai,
            api_key_required: true,
        },
        ProviderPreset {
            name: "anthropic",
            display_name: "🟡 Anthropic",
            base_url: "https://api.anthropic.com/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "openrouter",
            display_name: "🔵 OpenRouter",
            base_url: "https://openrouter.ai/api/v1",
            provider_type: Provider::Openrouter,
            api_key_required: true,
        },
        ProviderPreset {
            name: "google",
            display_name: "🔴 Google Gemini",
            base_url: "https://generativelanguage.googleapis.com/v1beta",
            provider_type: Provider::Google,
            api_key_required: true,
        },
        ProviderPreset {
            name: "deepseek",
            display_name: "🟣 DeepSeek",
            base_url: "https://api.deepseek.com/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "mistral",
            display_name: "🟠 Mistral AI",
            base_url: "https://api.mistral.ai/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "cohere",
            display_name: "⚫ Cohere",
            base_url: "https://api.cohere.com/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "ai21",
            display_name: "⚪ AI21 Labs",
            base_url: "https://api.ai21.com/studio/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "groq",
            display_name: "🟤 Groq",
            base_url: "https://api.groq.com/openai/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "perplexity",
            display_name: "🔷 Perplexity",
            base_url: "https://api.perplexity.ai",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "together",
            display_name: "🔶 Together AI",
            base_url: "https://api.together.xyz/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "fireworks",
            display_name: "🎆 Fireworks AI",
            base_url: "https://api.fireworks.ai/inference/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "replicate",
            display_name: "🔄 Replicate",
            base_url: "https://api.replicate.com/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "moonshot",
            display_name: "🌙 Moonshot AI (Kimi)",
            base_url: "https://api.moonshot.cn/v1",
            provider_type: Provider::Kimi,
            api_key_required: true,
        },
        ProviderPreset {
            name: "zai",
            display_name: "🇨🇭 Z AI",
            base_url: "https://api.z.ai/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "minimax",
            display_name: "📊 MiniMax",
            base_url: "https://api.minimax.chat/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "cerebras",
            display_name: "🧠 Cerebras",
            base_url: "https://api.cerebras.ai/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "ollama",
            display_name: "🏠 Ollama",
            base_url: "http://localhost:11434/v1",
            provider_type: Provider::Ollama,
            api_key_required: false,
        },
        ProviderPreset {
            name: "lmstudio",
            display_name: "💻 LM Studio",
            base_url: "http://localhost:1234/v1",
            provider_type: Provider::Custom,
            api_key_required: false,
        },
        ProviderPreset {
            name: "custom",
            display_name: "⚙️  Custom / Other",
            base_url: "",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
    ]
}

/// Handle adding a new provider
pub async fn handle_add_provider(config: &mut Config) -> Result<bool> {
    println!("\n🔌 Add New Provider");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    
    // Get provider presets
    let presets = get_provider_presets();
    let preset_names: Vec<&str> = presets.iter().map(|p| p.display_name).collect();
    
    // Let user select from presets
    let selected_preset = InquireSelect::new(
        "Select provider:",
        preset_names
    ).prompt()?;
    
    // Find the preset
    let preset = presets.iter()
        .find(|p| p.display_name == selected_preset)
        .unwrap();
    
    // Check if already exists
    if config.providers.contains_key(preset.name) {
        println!("⚠️  Provider '{}' already exists. Use Edit to modify it.", preset.name);
        return Ok(false);
    }
    
    // Get base URL (use preset default or allow custom)
    let base_url = if preset.name == "custom" {
        Text::new("Base URL:").prompt()?
    } else {
        let url = Text::new("Base URL:")
            .with_initial_value(preset.base_url)
            .prompt()?;
        url
    };
    
    // Get API key
    let api_key_prompt = if preset.api_key_required {
        "API Key:"
    } else {
        "API Key (optional for local):"
    };
    
    let api_key = Password::new()
        .with_prompt(api_key_prompt)
        .allow_empty_password(!preset.api_key_required)
        .interact()?;

    // Create provider config
    let provider_config = mylm_core::config::v2::ProviderConfig {
        provider_type: preset.provider_type.clone(),
        base_url,
        api_key: if api_key.is_empty() { None } else { Some(api_key) },
        timeout_secs: 30,
    };

    config.providers.insert(preset.name.to_string(), provider_config);
    
    // If this is the first provider, make it active
    if config.providers.len() == 1 {
        config.active_provider = preset.name.to_string();
    }

    println!("✅ Provider '{}' added successfully!", preset.name);
    config.save_to_default_location()?;
    Ok(true)
}

/// Handle editing a provider
pub async fn handle_edit_provider(config: &mut Config) -> Result<bool> {
    if config.providers.is_empty() {
        println!("⚠️  No providers configured. Add one first.");
        return Ok(false);
    }
    
    // Select provider to edit
    let provider_names: Vec<String> = config.providers.keys().cloned().collect();
    let name = InquireSelect::new("Select provider to edit:", provider_names).prompt()?;
    
    let provider_config = config.providers.get(&name).cloned().unwrap();
    
    println!("\n✏️  Editing Provider: {}", name);
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    println!("Current: {} @ {}", 
        format!("{:?}", provider_config.provider_type),
        provider_config.base_url
    );
    println!();

    let options = vec![
        "Change Base URL",
        "Change API Key",
        "Test Connection",
        "Back",
    ];

    loop {
        let choice = InquireSelect::new("What to edit:", options.clone()).prompt()?;

        match choice {
            "Change Base URL" => {
                let new_url = Text::new("Base URL:")
                    .with_initial_value(&provider_config.base_url)
                    .prompt()?;
                if let Some(cfg) = config.providers.get_mut(&name) {
                    cfg.base_url = new_url;
                }
                config.save_to_default_location()?;
                println!("✅ Base URL updated!");
            }
            "Change API Key" => {
                let new_key = Password::new()
                    .with_prompt("New API Key (empty to remove)")
                    .allow_empty_password(true)
                    .interact()?;
                if let Some(cfg) = config.providers.get_mut(&name) {
                    cfg.api_key = if new_key.is_empty() { None } else { Some(new_key) };
                }
                config.save_to_default_location()?;
                println!("✅ API Key updated!");
            }
            "Test Connection" => {
                let cfg = config.providers.get(&name).unwrap();
                println!("🔄 Testing connection to {}...", cfg.base_url);
                match fetch_models(&cfg.base_url, &cfg.api_key.clone().unwrap_or_default()).await {
                    Ok(models) => println!("✅ Success! Found {} models.", models.len()),
                    Err(e) => println!("❌ Failed: {}", e),
                }
            }
            _ => break,
        }
    }

    Ok(true)
}

/// Handle removing a provider
pub fn handle_remove_provider(config: &mut Config) -> Result<bool> {
    if config.providers.is_empty() {
        println!("⚠️  No providers to remove.");
        return Ok(false);
    }
    
    let provider_names: Vec<String> = config.providers.keys().cloned().collect();
    let name = InquireSelect::new("Select provider to remove:", provider_names).prompt()?;
    
    // Don't allow removing the active provider
    if name == config.active_provider {
        println!("⚠️  Cannot remove the active provider. Switch to another provider first.");
        return Ok(false);
    }
    
    config.providers.remove(&name);
    println!("✅ Provider '{}' removed.", name);
    config.save_to_default_location()?;
    Ok(true)
}

// ============================================================================
// MODEL SELECTION (Provider -> Model)
// ============================================================================

/// Select main LLM model - first choose provider, then model
pub async fn handle_select_main_model(config: &mut Config) -> Result<bool> {
    println!("\n🧠 Select Main LLM");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    
    // Step 1: Select Provider
    if config.providers.is_empty() {
        println!("⚠️  No providers configured. Add a provider first.");
        return Ok(false);
    }
    
    let provider_names: Vec<String> = config.providers.keys().cloned().collect();
    let selected_provider = InquireSelect::new("Select provider:", provider_names).prompt()?;
    
    // Make it the active provider
    config.active_provider = selected_provider.clone();
    
    // Update legacy endpoint for compatibility
    if let Some(provider_cfg) = config.providers.get(&selected_provider) {
        config.endpoint.provider = provider_cfg.provider_type.clone();
        config.endpoint.base_url = Some(provider_cfg.base_url.clone());
        config.endpoint.api_key = provider_cfg.api_key.clone();
    }
    
    // Step 2: Select Model from this provider
    println!("\n🔄 Fetching models from {}...", selected_provider);
    
    let provider_cfg = config.providers.get(&selected_provider).unwrap();
    let models = match fetch_models(&provider_cfg.base_url, 
                      &provider_cfg.api_key.clone().unwrap_or_default()).await {
        Ok(m) => m,
        Err(e) => {
            println!("⚠️  Could not fetch models: {}", e);
            println!("   Falling back to manual entry.");
            Vec::new()
        }
    };

    let selected_model = if models.is_empty() {
        Text::new("Model name:")
            .with_initial_value(&config.endpoint.model)
            .prompt()?
    } else {
        if models.len() > 20 {
            println!("   (Type to search through {} models)", models.len());
        }
        
        let initial = models.iter()
            .position(|m| m == &config.endpoint.model)
            .unwrap_or(0);
            
        InquireSelect::new("Select model:", models)
            .with_starting_cursor(initial)
            .prompt()?
    };

    config.endpoint.model = selected_model.clone();
    config.save_to_default_location()?;
    
    println!("✅ Main LLM set to: {} @ {}", selected_model, selected_provider);
    Ok(true)
}

/// Select worker model - can be from different provider than main
pub async fn handle_select_worker_model(config: &mut Config) -> Result<bool> {
    println!("\n⚡ Select Worker Model");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    println!("Worker model handles sub-tasks and simpler operations.");
    println!("Can be from same or different provider than main LLM.");
    println!();
    
    if config.providers.is_empty() {
        println!("⚠️  No providers configured. Add a provider first.");
        return Ok(false);
    }
    
    // Step 1: Select Provider for worker
    let provider_names: Vec<String> = config.providers.keys().cloned().collect();
    let selected_provider = InquireSelect::new("Select provider for worker:", provider_names).prompt()?;
    
    // Step 2: Select Model from this provider
    let provider_cfg = config.providers.get(&selected_provider).unwrap();
    
    println!("🔄 Fetching models from {}...", selected_provider);
    let mut models = match fetch_models(&provider_cfg.base_url, 
                      &provider_cfg.api_key.clone().unwrap_or_default()).await {
        Ok(m) => m,
        Err(_) => Vec::new(),
    };
    
    // Add "Same as Main LLM" option
    let same_as_main = format!("🔄 Same as Main ({})", config.endpoint.model);
    models.insert(0, same_as_main.clone());

    let selected = InquireSelect::new("Select worker model:", models).prompt()?;

    let worker_model = if selected == same_as_main {
        None // Use main model
    } else {
        Some(format!("{}/{}", selected_provider, selected))
    };

    // Update profile with worker model
    let profile = config.profiles.entry(config.profile.clone()).or_default();
    let current_agent = profile.agent.clone().unwrap_or_default();
    profile.agent = Some(mylm_core::config::AgentOverride {
        max_iterations: current_agent.max_iterations,
        iteration_rate_limit: current_agent.iteration_rate_limit,
        main_model: current_agent.main_model,
        worker_model: worker_model.clone(),
    });

    config.save_to_default_location()?;
    
    match worker_model {
        Some(m) => println!("✅ Worker model set to: {}", m),
        None => println!("✅ Worker model set to use Main LLM"),
    }
    Ok(true)
}

// ============================================================================
// WEB SEARCH SETTINGS
// ============================================================================

/// Show web search settings menu
pub fn show_web_search_menu(config: &Config) -> Result<WebSearchMenuChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;
    
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

/// Handle web search settings
pub async fn handle_web_search_settings(config: &mut Config) -> Result<bool> {
    loop {
        let action = show_web_search_menu(config)?;

        match action {
            WebSearchMenuChoice::ToggleEnabled => {
                config.features.web_search.enabled = !config.features.web_search.enabled;
                config.save_to_default_location()?;
                println!("✅ Web search {}", 
                    if config.features.web_search.enabled { "enabled" } else { "disabled" });
            }
            WebSearchMenuChoice::SetProvider => {
                let providers = vec![
                    "Kimi (Moonshot AI)",
                    "SerpAPI (Google/Bing)",
                    "Brave Search",
                ];
                let choice = InquireSelect::new("Select web search provider:", providers).prompt()?;
                
                config.features.web_search.provider = match choice {
                    "Kimi (Moonshot AI)" => mylm_core::config::SearchProvider::Kimi,
                    "Brave Search" => mylm_core::config::SearchProvider::Brave,
                    _ => mylm_core::config::SearchProvider::Serpapi,
                };
                config.features.web_search.enabled = true;
                config.save_to_default_location()?;
                println!("✅ Web search provider updated!");
            }
            WebSearchMenuChoice::SetApiKey => {
                let key = Password::new()
                    .with_prompt("Web Search API Key")
                    .allow_empty_password(true)
                    .interact()?;
                if !key.trim().is_empty() {
                    config.features.web_search.api_key = Some(key);
                    config.save_to_default_location()?;
                    println!("✅ API Key saved!");
                }
            }
            WebSearchMenuChoice::Back => break,
        }
    }
    Ok(true)
}

// ============================================================================
// AGENT SETTINGS
// ============================================================================

/// Show agent settings menu
pub fn show_agent_settings_menu(config: &Config) -> Result<AgentSettingsChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;
    
    let resolved = config.resolve_profile();
    let pacore_status = if config.features.pacore.enabled { "On" } else { "Off" };
    let rate_limit = resolved.agent.iteration_rate_limit;
    
    println!("\n⚙️  Agent Settings");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    println!("  Current Values:");
    println!("    Max Iterations:      {}", resolved.agent.max_iterations);
    println!("    Rate Limit:          {} ms", if rate_limit == 0 { "0 (no delay)".to_string() } else { rate_limit.to_string() });
    println!("    Tmux Autostart:      {}", if config.tmux_autostart { "On" } else { "Off" });
    println!("    PaCoRe:              {} (rounds: {})", pacore_status, config.features.pacore.rounds);
    println!();

    let options = vec![
        AgentSettingsChoice::IterationsSettings,
        AgentSettingsChoice::ToggleTmuxAutostart,
        AgentSettingsChoice::PaCoReSettings,
        AgentSettingsChoice::Back,
    ];

    let ans: Result<AgentSettingsChoice, _> = InquireSelect::new(
        "Select setting to change:",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(AgentSettingsChoice::Back),
    }
}

/// Show iterations settings submenu
pub fn show_iterations_settings_menu(config: &Config) -> Result<IterationsSettingsChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;
    
    let resolved = config.resolve_profile();
    let rate_limit = resolved.agent.iteration_rate_limit;
    
    println!("\n🔁 Iterations Settings");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    println!("  Current Values:");
    println!("    Max Iterations:      {}", resolved.agent.max_iterations);
    println!("    Rate Limit:          {} ms", if rate_limit == 0 { "0 (no delay)".to_string() } else { rate_limit.to_string() });
    println!();
    println!("  Rate Limit adds a pause between agent actions.");
    println!("  Useful for rate limiting or observing behavior.");
    println!();

    let options = vec![
        IterationsSettingsChoice::SetMaxIterations,
        IterationsSettingsChoice::SetRateLimit,
        IterationsSettingsChoice::Back,
    ];

    let ans: Result<IterationsSettingsChoice, _> = InquireSelect::new(
        "Select setting to change:",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(IterationsSettingsChoice::Back),
    }
}

/// Show PaCoRe settings submenu
pub fn show_pacore_settings_menu(config: &Config) -> Result<PaCoReSettingsChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;
    
    let status = if config.features.pacore.enabled { "On" } else { "Off" };
    
    println!("\n⚡ PaCoRe Settings");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    println!("  Current Values:");
    println!("    PaCoRe:              {}", status);
    println!("    Rounds:              {}", config.features.pacore.rounds);
    println!();
    println!("  PaCoRe uses parallel LLM calls to improve reasoning.");
    println!("  Format: comma-separated numbers (e.g., '4,1' or '16,4,1')");
    println!();

    let options = vec![
        PaCoReSettingsChoice::TogglePaCoRe,
        PaCoReSettingsChoice::SetPaCoReRounds,
        PaCoReSettingsChoice::Back,
    ];

    let ans: Result<PaCoReSettingsChoice, _> = InquireSelect::new(
        "Select setting to change:",
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(PaCoReSettingsChoice::Back),
    }
}

/// Handle changing max iterations
pub fn handle_max_iterations(config: &mut Config) -> Result<bool> {
    let current = config.resolve_profile().agent.max_iterations;
    
    let input = Text::new("Max iterations:")
        .with_initial_value(&current.to_string())
        .prompt()?;
    
    match input.parse::<usize>() {
        Ok(iters) if iters > 0 && iters <= 100 => {
            let profile_name = config.profile.clone();
            config.set_profile_max_iterations(&profile_name, Some(iters))?;
            config.save_to_default_location()?;
            println!("✅ Max iterations set to: {}", iters);
            Ok(true)
        }
        _ => {
            println!("⚠️  Invalid value. Must be between 1 and 100.");
            Ok(false)
        }
    }
}

/// Handle setting iteration rate limit
pub fn handle_set_rate_limit(config: &mut Config) -> Result<bool> {
    let current = config.resolve_profile().agent.iteration_rate_limit;
    
    let input = Text::new("Rate limit (ms between iterations):")
        .with_help_message("0 = no delay, higher values add pause between actions")
        .with_initial_value(&current.to_string())
        .prompt()?;
    
    match input.parse::<u64>() {
        Ok(ms) => {
            let profile_name = config.profile.clone();
            config.set_profile_iteration_rate_limit(&profile_name, Some(ms))?;
            config.save_to_default_location()?;
            if ms == 0 {
                println!("✅ Rate limit disabled (no delay between iterations)");
            } else {
                println!("✅ Rate limit set to: {} ms between iterations", ms);
            }
            Ok(true)
        }
        _ => {
            println!("⚠️  Invalid value. Must be a positive number.");
            Ok(false)
        }
    }
}

/// Handle toggling PaCoRe
pub fn handle_toggle_pacore(config: &mut Config) -> Result<bool> {
    config.features.pacore.enabled = !config.features.pacore.enabled;
    config.save_to_default_location()?;
    let status = if config.features.pacore.enabled { "enabled" } else { "disabled" };
    println!("✅ PaCoRe {}", status);
    Ok(true)
}

/// Handle setting PaCoRe rounds
pub fn handle_set_pacore_rounds(config: &mut Config) -> Result<bool> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;
    
    let current = config.features.pacore.rounds.clone();
    
    println!("\n📊 PaCoRe Rounds Configuration");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
    println!("Format: comma-separated numbers (e.g., '4,1' or '16,4,1')");
    println!("  - First number: parallel calls in round 1");
    println!("  - Last number: should be 1 for final synthesis");
    println!("  - Example: 4,1 = 4 parallel calls, then 1 synthesis");
    println!("  - Example: 16,4,1 = 16 calls → 4 synthesis → 1 final");
    println!();
    
    let input = Text::new("Rounds:")
        .with_initial_value(&current)
        .prompt()?;
    
    // Validate format
    let parts: Vec<&str> = input.split(',').collect();
    let mut valid = true;
    
    for part in &parts {
        if part.trim().parse::<usize>().is_err() {
            valid = false;
            break;
        }
    }
    
    if !valid || parts.is_empty() {
        println!("⚠️  Invalid format. Use comma-separated numbers (e.g., 4,1)");
        return Ok(false);
    }
    
    // Warn if last number isn't 1
    if let Some(last) = parts.last() {
        if last.trim() != "1" {
            println!("⚠️  Warning: Last round should be 1 for proper synthesis");
        }
    }
    
    config.features.pacore.rounds = input.clone();
    config.save_to_default_location()?;
    println!("✅ PaCoRe rounds set to: {}", input);
    Ok(true)
}

/// Handle toggling tmux autostart
pub fn handle_toggle_tmux_autostart(config: &mut Config) -> Result<bool> {
    config.tmux_autostart = !config.tmux_autostart;
    config.save_to_default_location()?;
    println!("✅ Tmux autostart {}", 
        if config.tmux_autostart { "enabled" } else { "disabled" });
    Ok(true)
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Fetch models from the API
async fn fetch_models(base_url: &str, api_key: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    
    let url = if base_url.ends_with('/') {
        format!("{}models", base_url)
    } else {
        format!("{}/models", base_url)
    };

    let mut request = client.get(&url);

    if !api_key.is_empty() && api_key != "none" {
        request = request.header("Authorization", format!("Bearer {}", api_key));
    }
    
    request = request.header("User-Agent", "mylm-cli/0.1.0");

    let response = request.send().await?;
    
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("API request failed: {}", response.status()));
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
        if let Some(data) = body.get("models").and_then(|v| v.as_array()) {
            for model in data {
                if let Some(id) = model.get("id").and_then(|v| v.as_str()) {
                    models.push(id.to_string());
                }
            }
        }
    }

    models.sort();
    Ok(models)
}

// ============================================================================
// BANNER & UTILS
// ============================================================================

async fn print_banner(_config: &Config) {
    let green = Style::new().green().bold();
    let cyan = Style::new().cyan();
    let dim = Style::new().dim();

    // Full mylm ASCII art banner (7 lines)
    let banner = r#"
    __  ___  __  __  __     __  ___
   /  |/  / / / / / / /    /  |/  /
  / /|_/ / / / / / / /    / /|_/ /
 / /  / / / /____ / / / / / / / /
/_/  /_/  \__, / /_____//_/  /_/
         /____/
    "#;

    println!("{}", green.apply_to(banner));
    
    // Just version info - no model/ctx/git
    println!("           {} {}-{} {}", 
        dim.apply_to("mylm"),
        cyan.apply_to(format!("v{}", env!("CARGO_PKG_VERSION"))),
        cyan.apply_to(format!("{} ({})", env!("BUILD_NUMBER"), env!("GIT_HASH"))),
        dim.apply_to("— Terminal AI, Built in Rust")
    );
}
