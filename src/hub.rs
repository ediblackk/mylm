//! Hub V3 - Configuration Menu for mylm V3 Architecture
//!
//! EXACT replica of original hub menu structure.
//! All running logic is stubbed - to be implemented one by one.

use anyhow::Result;
use console::Style;
use dialoguer::{Input, Password, Select, Confirm};
use inquire::Select as InquireSelect;
use mylm_core::config::{
    Config, ProfileConfig, ProviderConfig, ProviderType, SearchProvider,
};


/// ============================================================================
/// MAIN HUB MENU - EXACT replica of original
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
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
                if is_tmux_available() {
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

/// ============================================================================
/// SETTINGS DASHBOARD MENU - REVISED
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsMenuChoice {
    ManageProviders,      // Add/Edit/Remove providers
    MainLLMSettings,      // Main LLM comprehensive settings
    WorkerLLMSettings,    // Worker LLM comprehensive settings
    TestMainConnection,   // Test Main LLM connection
    TestWorkerConnection, // Test Worker LLM connection
    WebSearchSettings,    // Web search provider config
    ApplicationSettings,  // Global application settings (tmux, alias, etc)
    Back,
}

impl std::fmt::Display for SettingsMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsMenuChoice::ManageProviders => write!(f, "🔌 [1] Manage Providers"),
            SettingsMenuChoice::MainLLMSettings => write!(f, "🧠 [2] Main LLM Settings"),
            SettingsMenuChoice::WorkerLLMSettings => write!(f, "⚡ [3] Worker LLM Settings"),
            SettingsMenuChoice::TestMainConnection => write!(f, "🧪 [4] Test Main Connection"),
            SettingsMenuChoice::TestWorkerConnection => write!(f, "🧪 [5] Test Worker Connection"),
            SettingsMenuChoice::WebSearchSettings => write!(f, "🌐 [6] Web Search"),
            SettingsMenuChoice::ApplicationSettings => write!(f, "🔧 [7] Application Settings"),
            SettingsMenuChoice::Back => write!(f, "⬅️  [8] Back"),
        }
    }
}

/// ============================================================================
/// MAIN LLM SETTINGS MENU
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MainLLMSettingsChoice {
    SelectModel,          // fetch models with filterable list
    ContextSettings,      // max tokens, condense threshold, prices, rate limit
    AgenticSettings,      // allowed commands, restricted commands, max actions, pacore
    Back,
}

impl std::fmt::Display for MainLLMSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MainLLMSettingsChoice::SelectModel => write!(f, "🎯 Select Model"),
            MainLLMSettingsChoice::ContextSettings => write!(f, "📊 Context Settings"),
            MainLLMSettingsChoice::AgenticSettings => write!(f, "🤖 Agentic Settings"),
            MainLLMSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// ============================================================================
/// WORKER LLM SETTINGS MENU
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorkerLLMSettingsChoice {
    SelectModel,          // fetch models with filterable list
    ContextSettings,      // max tokens, condense threshold, prices, rate limit
    AgenticSettings,      // allowed commands, restricted commands, max actions, pacore
    Back,
}

impl std::fmt::Display for WorkerLLMSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerLLMSettingsChoice::SelectModel => write!(f, "🎯 Select Model"),
            WorkerLLMSettingsChoice::ContextSettings => write!(f, "📊 Context Settings"),
            WorkerLLMSettingsChoice::AgenticSettings => write!(f, "🤖 Agentic Settings"),
            WorkerLLMSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// ============================================================================
/// APPLICATION SETTINGS SUBMENU
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApplicationSettingsChoice {
    ToggleTmuxAutostart,
    SetPreferredAlias,
    Back,
}

impl std::fmt::Display for ApplicationSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplicationSettingsChoice::ToggleTmuxAutostart => write!(f, "🔄 Toggle Tmux Autostart"),
            ApplicationSettingsChoice::SetPreferredAlias => write!(f, "🏷️  Set Preferred Alias"),
            ApplicationSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// ============================================================================
/// CONTEXT SETTINGS SUBMENU
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContextSettingsChoice {
    SetMaxTokens,
    SetCondenseThreshold,
    SetInputPrice,
    SetOutputPrice,
    SetRateLimit,
    Back,
}

impl std::fmt::Display for ContextSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextSettingsChoice::SetMaxTokens => write!(f, "🔢 Max Context Tokens"),
            ContextSettingsChoice::SetCondenseThreshold => write!(f, "📉 Condense Threshold"),
            ContextSettingsChoice::SetInputPrice => write!(f, "💰 Input Price (per 1M)"),
            ContextSettingsChoice::SetOutputPrice => write!(f, "💰 Output Price (per 1M)"),
            ContextSettingsChoice::SetRateLimit => write!(f, "⏱️  Rate Limit (RPM)"),
            ContextSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// ============================================================================
/// AGENTIC SETTINGS SUBMENU
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgenticSettingsChoice {
    SetAllowedCommands,
    SetRestrictedCommands,
    SetMaxActionsBeforeStall,
    PaCoReSettings,
    Back,
}

impl std::fmt::Display for AgenticSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgenticSettingsChoice::SetAllowedCommands => write!(f, "✅ Always Allowed Commands"),
            AgenticSettingsChoice::SetRestrictedCommands => write!(f, "🚫 Always Restricted Commands"),
            AgenticSettingsChoice::SetMaxActionsBeforeStall => write!(f, "🔢 Max Actions Before Stall"),
            AgenticSettingsChoice::PaCoReSettings => write!(f, "⚡ PaCoRe Settings"),
            AgenticSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// ============================================================================
/// PACORE SETTINGS SUBMENU (inside Agentic)
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaCoReSubSettingsChoice {
    ToggleEnabled,
    SetRounds,
    Back,
}

impl std::fmt::Display for PaCoReSubSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaCoReSubSettingsChoice::ToggleEnabled => write!(f, "✅ Toggle PaCoRe"),
            PaCoReSubSettingsChoice::SetRounds => write!(f, "🔢 Set Rounds (default: 4,1)"),
            PaCoReSubSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// ============================================================================
/// PROVIDER MANAGEMENT SUBMENU - EXACT replica of original
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
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

/// ============================================================================
/// WEB SEARCH SETTINGS SUBMENU - EXACT replica of original
/// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
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

/// ============================================================================
/// MAIN HUB - Show hub menu with EXACT original logic
/// ============================================================================

pub async fn show_hub(_config: &Config) -> Result<HubChoice> {
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

/// ============================================================================
/// SETTINGS DASHBOARD - EXACT replica of original
/// ============================================================================

pub fn show_settings_dashboard(config: &Config) -> Result<SettingsMenuChoice> {
    print_config_banner(config);
    
    let choices = vec![
        SettingsMenuChoice::ManageProviders,
        SettingsMenuChoice::MainLLMSettings,
        SettingsMenuChoice::WorkerLLMSettings,
        SettingsMenuChoice::TestMainConnection,
        SettingsMenuChoice::TestWorkerConnection,
        SettingsMenuChoice::WebSearchSettings,
        SettingsMenuChoice::ApplicationSettings,
        SettingsMenuChoice::Back,
    ];
    
    let selection = Select::new()
        .with_prompt("Select setting to configure")
        .items(&choices)
        .default(0)
        .interact()?;
    
    Ok(choices[selection])
}

/// ============================================================================
/// MAIN LLM SETTINGS MENU
/// ============================================================================

pub fn show_main_llm_settings_menu(_config: &Config) -> Result<MainLLMSettingsChoice> {
    print!("\x1B[2J\x1B[1;1H");
    
    println!("\n{}", Style::new().bold().apply_to("Main LLM Settings"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    
    let choices = vec![
        MainLLMSettingsChoice::SelectModel,
        MainLLMSettingsChoice::ContextSettings,
        MainLLMSettingsChoice::AgenticSettings,
        MainLLMSettingsChoice::Back,
    ];
    
    let selection = Select::new()
        .with_prompt("Configure Main LLM")
        .items(&choices)
        .default(0)
        .interact()?;
    
    Ok(choices[selection])
}

/// ============================================================================
/// WORKER LLM SETTINGS MENU
/// ============================================================================

pub fn show_worker_llm_settings_menu(_config: &Config) -> Result<WorkerLLMSettingsChoice> {
    print!("\x1B[2J\x1B[1;1H");
    
    println!("\n{}", Style::new().bold().apply_to("Worker LLM Settings"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    
    let choices = vec![
        WorkerLLMSettingsChoice::SelectModel,
        WorkerLLMSettingsChoice::ContextSettings,
        WorkerLLMSettingsChoice::AgenticSettings,
        WorkerLLMSettingsChoice::Back,
    ];
    
    let selection = Select::new()
        .with_prompt("Configure Worker LLM")
        .items(&choices)
        .default(0)
        .interact()?;
    
    Ok(choices[selection])
}

/// ============================================================================
/// APPLICATION SETTINGS MENU
/// ============================================================================

pub fn show_application_settings_menu() -> Result<ApplicationSettingsChoice> {
    print!("\x1B[2J\x1B[1;1H");
    
    println!("\n{}", Style::new().bold().apply_to("Application Settings"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    
    let choices = vec![
        ApplicationSettingsChoice::ToggleTmuxAutostart,
        ApplicationSettingsChoice::SetPreferredAlias,
        ApplicationSettingsChoice::Back,
    ];
    
    let selection = Select::new()
        .with_prompt("Select option")
        .items(&choices)
        .default(0)
        .interact()?;
    
    Ok(choices[selection])
}

/// ============================================================================
/// CONTEXT SETTINGS MENU
/// ============================================================================

pub fn show_context_settings_menu(is_main: bool) -> Result<ContextSettingsChoice> {
    print!("\x1B[2J\x1B[1;1H");
    
    let title = if is_main { "Main LLM - Context Settings" } else { "Worker LLM - Context Settings" };
    println!("\n{}", Style::new().bold().apply_to(title));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    
    let choices = vec![
        ContextSettingsChoice::SetMaxTokens,
        ContextSettingsChoice::SetCondenseThreshold,
        ContextSettingsChoice::SetInputPrice,
        ContextSettingsChoice::SetOutputPrice,
        ContextSettingsChoice::SetRateLimit,
        ContextSettingsChoice::Back,
    ];
    
    let selection = Select::new()
        .with_prompt("Select option")
        .items(&choices)
        .default(0)
        .interact()?;
    
    Ok(choices[selection])
}

/// ============================================================================
/// AGENTIC SETTINGS MENU
/// ============================================================================

pub fn show_agentic_settings_menu(is_main: bool) -> Result<AgenticSettingsChoice> {
    print!("\x1B[2J\x1B[1;1H");
    
    let title = if is_main { "Main LLM - Agentic Settings" } else { "Worker LLM - Agentic Settings" };
    println!("\n{}", Style::new().bold().apply_to(title));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    
    let choices = vec![
        AgenticSettingsChoice::SetAllowedCommands,
        AgenticSettingsChoice::SetRestrictedCommands,
        AgenticSettingsChoice::SetMaxActionsBeforeStall,
        AgenticSettingsChoice::PaCoReSettings,
        AgenticSettingsChoice::Back,
    ];
    
    let selection = Select::new()
        .with_prompt("Select option")
        .items(&choices)
        .default(0)
        .interact()?;
    
    Ok(choices[selection])
}

/// ============================================================================
/// PACORE SUB-SETTINGS MENU
/// ============================================================================

pub fn show_pacore_sub_settings_menu() -> Result<PaCoReSubSettingsChoice> {
    print!("\x1B[2J\x1B[1;1H");
    
    println!("\n{}", Style::new().bold().apply_to("PaCoRe Settings"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    
    let choices = vec![
        PaCoReSubSettingsChoice::ToggleEnabled,
        PaCoReSubSettingsChoice::SetRounds,
        PaCoReSubSettingsChoice::Back,
    ];
    
    let selection = Select::new()
        .with_prompt("Configure PaCoRe")
        .items(&choices)
        .default(0)
        .interact()?;
    
    Ok(choices[selection])
}

/// ============================================================================
/// PROVIDER MENU - EXACT replica of original
/// ============================================================================

pub fn show_provider_menu() -> Result<ProviderMenuChoice> {
    let choices = vec![
        ProviderMenuChoice::AddProvider,
        ProviderMenuChoice::EditProvider,
        ProviderMenuChoice::RemoveProvider,
        ProviderMenuChoice::Back,
    ];
    
    let selection = Select::new()
        .with_prompt("Provider Management")
        .items(&choices)
        .default(0)
        .interact()?;
    
    Ok(choices[selection])
}

/// ============================================================================
/// WEB SEARCH MENU - EXACT replica of original
/// ============================================================================

pub fn show_web_search_menu(config: &Config) -> Result<WebSearchMenuChoice> {
    print!("\x1B[2J\x1B[1;1H");
    
    // Show status from profile.web_search.enabled (what the tool actually uses)
    let profile_enabled = config.active_profile().web_search.enabled;
    let provider = &config.active_profile().web_search.provider;
    let has_api_key = config.active_profile().web_search.api_key.is_some();
    
    println!("\n{}", Style::new().bold().apply_to("Web Search Settings"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    println!("Status: {}", 
        if profile_enabled { "✅ Enabled" } else { "❌ Disabled" });
    println!("Provider: {:?}", provider);
    println!("API Key: {}\n", 
        if has_api_key { "✅ Set" } else { "❌ Not set (using env var or N/A)" });
    
    let choices = vec![
        WebSearchMenuChoice::ToggleEnabled,
        WebSearchMenuChoice::SetProvider,
        WebSearchMenuChoice::SetApiKey,
        WebSearchMenuChoice::Back,
    ];
    
    let selection = Select::new()
        .with_prompt("Select option")
        .items(&choices)
        .default(0)
        .interact()?;
    
    Ok(choices[selection])
}

/// ============================================================================
/// HANDLER FUNCTIONS - All stubbed for later implementation
/// ============================================================================

/// Handle add provider
pub async fn handle_add_provider(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Add LLM Provider"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    println!("{}\n", Style::new().italic().dim().apply_to("Tip: Type to filter the list, ↑/↓ to navigate, Enter to select"));
    
    // Provider presets
    // First: Generic/Local options (no score needed)
    // Then: Cloud providers sorted by Artificial Index Intelligence Score (higher = better)
    // Format: (display_name, base_url, provider_type, ai_score)
    let presets = vec![
        // === GENERIC / LOCAL OPTIONS FIRST ===
        ("OpenAI Compatible (Custom)", "", ProviderType::Custom, 0),
        ("Ollama (Local Models)", "http://localhost:11434/v1", ProviderType::Ollama, 0),
        ("LM Studio (Local)", "http://localhost:1234/v1", ProviderType::Custom, 0),
        
        // === CLOUD PROVIDERS BY AI SCORE ===
        // Tier 1: Best AI Score (48-53) - Elite Providers
        ("Anthropic (Claude)", "https://api.anthropic.com/v1", ProviderType::Custom, 53),
        ("OpenAI", "https://api.openai.com/v1", ProviderType::OpenAi, 51),
        ("Google Gemini", "https://generativelanguage.googleapis.com/v1beta", ProviderType::Google, 50),
        
        // Tier 2: High AI Score (40-47) - Excellent Providers
        ("Moonshot (Kimi)", "https://api.moonshot.cn/v1", ProviderType::Kimi, 47),
        ("DeepSeek", "https://api.deepseek.com/v1", ProviderType::Custom, 42),
        ("MiniMax", "https://api.minimax.chat/v1", ProviderType::Custom, 42),
        ("xAI (Grok)", "https://api.x.ai/v1", ProviderType::Custom, 41),
        ("AWS Bedrock", "", ProviderType::Custom, 36),
        
        // Tier 3: Good AI Score (25-35) - Solid Providers
        ("Alibaba Qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1", ProviderType::Custom, 32),
        ("Mistral", "https://api.mistral.ai/v1", ProviderType::Custom, 23),
        
        // Routers / Aggregators (provide access to multiple providers)
        ("OpenRouter", "https://openrouter.ai/api/v1", ProviderType::OpenRouter, 0),
        ("Together AI", "https://api.together.xyz/v1", ProviderType::Custom, 0),
        ("Fireworks", "https://api.fireworks.ai/inference/v1", ProviderType::Custom, 0),
        ("Groq", "https://api.groq.com/openai/v1", ProviderType::OpenAi, 0),
        
        // Other Notable Providers
        ("Cerebras", "https://api.cerebras.ai/v1", ProviderType::Custom, 0),
        ("SambaNova", "https://api.sambanova.ai/v1", ProviderType::Custom, 0),
        ("Azure OpenAI", "", ProviderType::Custom, 0),
        ("GCP Vertex AI", "", ProviderType::Custom, 0),
        
        // Chinese Providers
        ("Doubao", "https://ark.cn-beijing.volces.com/api/v3", ProviderType::Custom, 0),
        
        // Other Routers
        ("Vercel AI Gateway", "", ProviderType::Custom, 0),
        ("LiteLLM", "", ProviderType::Custom, 0),
        ("Hugging Face", "https://api-inference.huggingface.co", ProviderType::Custom, 0),
        ("Unbound", "https://api.unbound.com/v1", ProviderType::Custom, 0),
        ("Requesty", "https://router.requesty.ai/v1", ProviderType::Custom, 0),
        ("DeepInfra", "https://api.deepinfra.com/v1/openai", ProviderType::OpenAi, 0),
        ("Baseten", "https://app.baseten.co/v1", ProviderType::Custom, 0),
        ("Featherless", "https://api.featherless.ai/v1", ProviderType::Custom, 0),
    ];
    
    let preset_names: Vec<&str> = presets.iter().map(|(name, _, _, _)| *name).collect();
    
    // Use inquire's Select which has built-in filtering
    let ans = InquireSelect::new("Search or select provider (type to filter, ESC to cancel):", preset_names)
        .with_page_size(15)
        .with_help_message("↑↓ to navigate, type to filter, Enter to select")
        .prompt();
    
    // Handle cancellation
    let selection = match ans {
        Ok(s) => {
            // Find the index of the selected item
            match presets.iter().position(|(name, _, _, _)| *name == s) {
                Some(idx) => idx,
                None => return Ok(false), // Not found (shouldn't happen)
            }
        }
        Err(_) => return Ok(false), // User cancelled with ESC
    };
    
    let (preset_name, preset_url, provider_type, _) = &presets[selection];
    
    // Get provider name
    let name: String = Input::new()
        .with_prompt("Provider name (for reference)")
        .default(preset_name.to_string())
        .interact()?;
    
    // Get base URL
    let base_url: String = if !preset_url.is_empty() {
        Input::new()
            .with_prompt("Base URL")
            .default(preset_url.to_string())
            .interact()?
    } else {
        Input::new()
            .with_prompt("Base URL")
            .interact()?
    };
    
    // Get API key (optional for local providers)
    let is_local = preset_name.contains("Local") || preset_name.contains("Ollama") || preset_name.contains("LM Studio");
    let prompt_text = if is_local {
        "API key (optional for local providers)"
    } else {
        "API key"
    };
    
    let api_key: String = Password::new()
        .with_prompt(prompt_text)
        .allow_empty_password(true)
        .interact()?;
    
    let api_key = if api_key.is_empty() { None } else { Some(api_key) };
    
    // Create provider config (use placeholder model - user selects later)
    let provider_config = ProviderConfig {
        provider_type: provider_type.clone(),
        base_url,
        api_key,
        default_model: "default".to_string(),
        models: vec![],
        timeout_secs: 120,
    };
    
    // Add to config
    config.providers.insert(name.clone(), provider_config);
    
    // If this is the first provider, set it as active
    if config.providers.len() == 1 {
        config.active_profile = "default".to_string();
        let profile = ProfileConfig {
            provider: name.clone(),
            model: None, // User will select model later
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
        };
        config.profiles.insert("default".to_string(), profile);
    }
    
    // Save config
    config.save_default()?;
    
    println!("\n✅ Provider '{}' added successfully!", name);
    println!("   Base URL: {}", config.providers.get(&name).unwrap().base_url);
    println!("   You can select a model from this provider in 'Main LLM Settings'");
    
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    
    Ok(true)
}

/// Handle edit provider
pub async fn handle_edit_provider(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Edit Provider"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    
    if config.providers.is_empty() {
        println!("\n❌ No providers configured.");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        return Ok(false);
    }
    
    let mut provider_names: Vec<String> = config.providers.keys().cloned().collect();
    provider_names.sort();
    provider_names.push("⬅️  Back".to_string());
    
    let ans = InquireSelect::new("Select provider to edit:", provider_names.clone())
        .with_page_size(15)
        .with_help_message("↑↓ to navigate, type to filter, Enter to select")
        .prompt();
    
    // Handle cancellation or back
    let selection = match ans {
        Ok(s) => {
            if s == "⬅️  Back" {
                return Ok(false);
            }
            match provider_names.iter().position(|name| *name == s) {
                Some(idx) => idx,
                None => return Ok(false),
            }
        }
        Err(_) => return Ok(false),
    };
    
    let name = provider_names[selection].clone();
    let provider = config.providers.get(&name).cloned();
    
    if let Some(mut provider) = provider {
        // Edit base URL
        provider.base_url = Input::new()
            .with_prompt("Base URL")
            .default(provider.base_url)
            .interact()?;
        
        // Edit API key
        let new_key: String = Password::new()
            .with_prompt("API key (leave empty to keep current)")
            .allow_empty_password(true)
            .interact()?;
        if !new_key.is_empty() {
            provider.api_key = Some(new_key);
        }
        
        // Edit default model
        provider.default_model = Input::new()
            .with_prompt("Default model")
            .default(provider.default_model)
            .interact()?;
        
        // Update provider
        config.providers.insert(name.clone(), provider);
        config.save_default()?;
        
        println!("\n✅ Provider '{}' updated!", name);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    
    Ok(true)
}

/// Handle remove provider
pub fn handle_remove_provider(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Remove Provider"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    
    if config.providers.is_empty() {
        println!("\n❌ No providers configured.");
        return Ok(false);
    }
    
    let mut provider_names: Vec<String> = config.providers.keys().cloned().collect();
    provider_names.sort();
    provider_names.push("⬅️  Back".to_string());
    
    let ans = InquireSelect::new("Select provider to remove:", provider_names.clone())
        .with_page_size(15)
        .with_help_message("↑↓ to navigate, type to filter, Enter to select")
        .prompt();
    
    // Handle cancellation or back
    let selection = match ans {
        Ok(s) => {
            if s == "⬅️  Back" {
                return Ok(false);
            }
            match provider_names.iter().position(|name| *name == s) {
                Some(idx) => idx,
                None => return Ok(false),
            }
        }
        Err(_) => return Ok(false),
    };
    
    let name = provider_names[selection].clone();
    
    if Confirm::new()
        .with_prompt(format!("Are you sure you want to remove '{}'?", name))
        .default(false)
        .interact()?
    {
        config.providers.remove(&name);
        config.save_default()?;
        println!("\n✅ Provider '{}' removed!", name);
    } else {
        println!("\nCancelled.");
    }
    
    Ok(true)
}

/// Handle select main model
pub async fn handle_select_main_model(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Select Main Model"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    
    // Step 1: Select Provider
    if config.providers.is_empty() {
        println!("\n❌ No providers configured. Add a provider first.");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        return Ok(false);
    }
    
    let mut provider_names: Vec<String> = config.providers.keys().cloned().collect();
    provider_names.sort();
    
    let ans = InquireSelect::new("Select provider:", provider_names.clone())
        .with_page_size(15)
        .with_help_message("↑↓ to navigate, type to filter, Enter to select")
        .prompt();
    
    let selected_provider = match ans {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    
    // Get provider config
    let provider_cfg = match config.providers.get(&selected_provider) {
        Some(p) => p.clone(),
        None => return Ok(false),
    };
    
    // Update active profile to use this provider
    {
        let profile_name = config.active_profile.clone();
        if let Some(profile) = config.profiles.get_mut(&profile_name) {
            profile.provider = selected_provider.clone();
        }
    }
    
    // Step 2: Fetch models from provider
    println!("\n🔄 Fetching models from {}...", selected_provider);
    
    let models = match fetch_models(&provider_cfg.base_url, 
                      &provider_cfg.api_key.clone().unwrap_or_default()).await {
        Ok(m) => {
            if m.is_empty() {
                println!("   No models returned, using manual entry.");
            } else {
                println!("   Found {} models", m.len());
            }
            m
        }
        Err(e) => {
            println!("   ⚠️  Could not fetch models: {}", e);
            println!("   Falling back to manual entry.");
            Vec::new()
        }
    };
    
    // Step 3: Select or enter model
    let current_model = config.active_profile().model.clone().unwrap_or_default();
    
    let selected_model = if models.is_empty() {
        // Manual entry
        let model: String = Input::new()
            .with_prompt("Enter model name")
            .default(current_model)
            .interact()?;
        model
    } else {
        // Select from list
        let ans = InquireSelect::new("Select model:", models)
            .with_page_size(15)
            .with_help_message("↑↓ to navigate, type to filter, Enter to select")
            .prompt();
        
        match ans {
            Ok(s) => s,
            Err(_) => return Ok(false),
        }
    };
    
    // Save to profile
    let profile_name = config.active_profile.clone();
    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.model = Some(selected_model.clone());
        // Mark as needing re-test since provider/model changed
        config.mark_profile_needs_test(&profile_name);
        config.save_default()?;
        println!("\n✅ Main LLM set to: {} @ {}", selected_model, selected_provider);
        println!("   ⚠️  Run Test Connection to verify configuration");
    }
    
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    Ok(true)
}

/// Handle select worker model
pub async fn handle_select_worker_model(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Select Worker Model"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    println!("{}", Style::new().italic().dim().apply_to("Worker model can be from a different provider than main model\n"));
    
    // Step 1: Select Provider
    if config.providers.is_empty() {
        println!("\n❌ No providers configured. Add a provider first.");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        return Ok(false);
    }
    
    let mut provider_names: Vec<String> = config.providers.keys().cloned().collect();
    provider_names.sort();
    
    let ans = InquireSelect::new("Select provider for worker:", provider_names.clone())
        .with_page_size(15)
        .with_help_message("↑↓ to navigate, type to filter, Enter to select")
        .prompt();
    
    let selected_provider = match ans {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    
    // Get provider config
    let provider_cfg = match config.providers.get(&selected_provider) {
        Some(p) => p.clone(),
        None => return Ok(false),
    };
    
    // Step 2: Fetch models from provider
    println!("\n🔄 Fetching models from {}...", selected_provider);
    
    let models = match fetch_models(&provider_cfg.base_url, 
                      &provider_cfg.api_key.clone().unwrap_or_default()).await {
        Ok(m) => {
            if m.is_empty() {
                println!("   No models returned, using manual entry.");
            } else {
                println!("   Found {} models", m.len());
            }
            m
        }
        Err(e) => {
            println!("   ⚠️  Could not fetch models: {}", e);
            println!("   Falling back to manual entry.");
            Vec::new()
        }
    };
    
    // Step 3: Select or enter model
    let selected_model = if models.is_empty() {
        // Manual entry
        let model: String = Input::new()
            .with_prompt("Enter worker model name")
            .interact()?;
        model
    } else {
        // Select from list
        let ans = InquireSelect::new("Select worker model:", models)
            .with_page_size(15)
            .with_help_message("↑↓ to navigate, type to filter, Enter to select")
            .prompt();
        
        match ans {
            Ok(s) => s,
            Err(_) => return Ok(false),
        }
    };
    
    // For now, store worker model in a separate field or profile
    // We'll create a "worker" profile if it doesn't exist
    if !config.profiles.contains_key("worker") {
        let worker_profile = ProfileConfig {
            provider: selected_provider.clone(),
            model: Some(selected_model.clone()),
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
        };
        config.profiles.insert("worker".to_string(), worker_profile);
    } else {
        if let Some(profile) = config.profiles.get_mut("worker") {
            profile.provider = selected_provider.clone();
            profile.model = Some(selected_model.clone());
        }
    }
    
    // Mark worker profile as needing re-test
    config.mark_profile_needs_test("worker");
    config.save_default()?;
    println!("\n✅ Worker LLM set to: {} @ {}", selected_model, selected_provider);
    println!("   ⚠️  Run Test Connection to verify configuration");
    
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    Ok(true)
}

/// Test connection for a profile (main or worker)
pub async fn test_profile_connection(config: &mut Config, profile_name: &str) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    
    let profile_label = if profile_name == "default" { "Main" } else { profile_name };
    println!("\n{}", Style::new().bold().apply_to(format!("Test Connection - {} LLM", profile_label)));
    println!("{}", Style::new().dim().apply_to("─".repeat(50)));
    
    // Get profile
    let profile = match config.profiles.get(profile_name) {
        Some(p) => p.clone(),
        None => {
            println!("\n❌ Profile '{}' not found", profile_name);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            return Ok(false);
        }
    };
    
    // Get provider config
    let provider_cfg = match config.providers.get(&profile.provider) {
        Some(p) => p.clone(),
        None => {
            println!("\n❌ Provider '{}' not found", profile.provider);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            return Ok(false);
        }
    };
    
    println!("\n  Provider: {}", profile.provider);
    println!("  Model: {}", profile.model.as_deref().unwrap_or("Not set"));
    println!("  Base URL: {}", provider_cfg.base_url);
    
    // Test 1: Check if API key is present (if needed)
    println!("\n🔄 Testing connection...");
    
    let api_key = provider_cfg.api_key.clone().unwrap_or_default();
    if api_key.is_empty() && !provider_cfg.base_url.contains("localhost") && !provider_cfg.base_url.contains("127.0.0.1") {
        println!("   ⚠️  No API key configured (may fail for cloud providers)");
    }
    
    // Test 2: Try to fetch models
    match fetch_models(&provider_cfg.base_url, &api_key).await {
        Ok(models) => {
            println!("   ✅ API endpoint reachable");
            println!("   ✅ Found {} models", models.len());
            
            // Test 3: Check if selected model exists in list
            if let Some(ref selected_model) = profile.model {
                if models.contains(selected_model) {
                    println!("   ✅ Selected model '{}' found", selected_model);
                } else if !models.is_empty() {
                    println!("   ⚠️  Selected model '{}' not in available models", selected_model);
                    println!("      Available: {:?}", &models[..models.len().min(5)]);
                }
            } else {
                println!("   ⚠️  No model selected");
            }
            
            // Mark as tested
            config.mark_profile_tested(profile_name);
            config.save_default()?;
            
            println!("\n✅ {} LLM configuration verified!", profile_label);
        }
        Err(e) => {
            let error_msg = format!("{}", e);
            println!("   ❌ Connection failed: {}", e);
            println!("\n⚠️  Check your:");
            println!("   - Base URL (should end with /v1 for OpenAI-compatible)");
            println!("   - API key");
            println!("   - Network connection");
            
            // Mark as tested with error
            config.mark_profile_test_failed(profile_name, error_msg);
            config.save_default()?;
        }
    }
    
    println!("\nPress Enter to continue...");
    let _ = std::io::stdin().read_line(&mut String::new());
    Ok(true)
}

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

    let body: serde_json::Value = response.json().await?;
    
    let mut models = Vec::new();
    if let Some(data) = body.get("data").and_then(|v| v.as_array()) {
        for model in data {
            if let Some(id) = model.get("id").and_then(|v| v.as_str()) {
                models.push(id.to_string());
            }
        }
    }
    
    // Fallback: try "models" key (some providers use this)
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

/// Handle web search settings
pub async fn handle_web_search_settings(config: &mut Config) -> Result<bool> {
    use mylm_core::config::SearchProvider;
    
    loop {
        print!("\x1B[2J\x1B[1;1H");
        println!("\n{}", Style::new().bold().apply_to("Web Search Settings"));
        println!("{}", Style::new().dim().apply_to("─".repeat(40)));
        
        // Get current web search config from active profile
        let web_search = &config.active_profile().web_search;
        
        println!("Status: {}", 
            if web_search.enabled { "✅ Enabled" } else { "❌ Disabled" });
        println!("Provider: {:?}\n", web_search.provider);
        
        let has_extra_params = web_search.extra_params.as_ref().map(|p| !p.is_empty()).unwrap_or(false);
        
        let choices = vec![
            "Toggle Web Search",
            "Select Provider",
            "Set API Key",
            if has_extra_params { "⚙️  Configure Extra Parameters" } else { "Configure Extra Parameters" },
            "Back",
        ];
        
        let selection = Select::new()
            .with_prompt("Select option")
            .items(&choices)
            .default(0)
            .interact()?;
        
        match selection {
            0 => {
                // Toggle web search - sync both config.features and profile.web_search
                let new_status = {
                    let profile = config.active_profile_mut();
                    profile.web_search.enabled = !profile.web_search.enabled;
                    profile.web_search.enabled
                };
                // Sync with global features flag
                config.features.web_search = new_status;
                config.save_default()?;
                
                log::info!("[CONFIG] Web search {} for profile '{}'", 
                    if new_status { "enabled" } else { "disabled" },
                    config.active_profile);
                
                println!("\n✅ Web search {}", 
                    if new_status { "enabled" } else { "disabled" });
                println!("   Profile: {}", config.active_profile);
                println!("   Provider: {:?}", config.active_profile().web_search.provider);
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            }
            1 => {
                // Select provider
                let providers = vec![
                    ("DuckDuckGo (Free, no API key)", SearchProvider::DuckDuckGo),
                    ("SerpApi (Google/Bing results)", SearchProvider::Serpapi),
                    ("Brave Search", SearchProvider::Brave),
                    ("OpenAI", SearchProvider::Openai),
                    ("Exa (Neural Search)", SearchProvider::Exa),
                    ("Google Custom Search", SearchProvider::Google),
                    ("Tavily (AI-native)", SearchProvider::Tavily),
                    ("Kimi (Moonshot AI)", SearchProvider::Kimi),
                    ("Custom", SearchProvider::Custom),
                ];
                
                let provider_names: Vec<&str> = providers.iter().map(|(name, _)| *name).collect();
                
                let provider_selection = Select::new()
                    .with_prompt("Select search provider")
                    .items(&provider_names)
                    .default(0)
                    .interact()?;
                
                let selected_provider = providers[provider_selection].1.clone();
                {
                    let profile = config.active_profile_mut();
                    profile.web_search.provider = selected_provider;
                }
                config.save_default()?;
                
                println!("\n✅ Provider set to: {}", provider_names[provider_selection]);
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
            }
            2 => {
                // Set API key
                let api_key = Password::new()
                    .with_prompt("Enter API key (or leave empty to use env var)")
                    .allow_empty_password(true)
                    .interact()?;
                
                let provider = config.active_profile().web_search.provider.clone();
                
                {
                    let profile = config.active_profile_mut();
                    if api_key.is_empty() {
                        profile.web_search.api_key = None;
                        config.save_default()?;
                        log::info!("[CONFIG] Web search API key cleared for provider {:?}", provider);
                        println!("\n✅ API key cleared (will use environment variable)");
                        tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                    } else {
                        profile.web_search.api_key = Some(api_key.clone());
                        config.save_default()?;
                        log::info!("[CONFIG] Web search API key set for provider {:?}", provider);
                        println!("\n✅ API key set");
                        
                        // Test the API key if provider requires it
                        if provider != SearchProvider::DuckDuckGo {
                            println!("   Testing API key...");
                            match test_web_search_api_key(&provider, &api_key).await {
                                Ok(()) => {
                                    log::info!("[CONFIG] Web search API key test passed for {:?}", provider);
                                    println!("   ✅ API key is valid!");
                                }
                                Err(e) => {
                                    log::warn!("[CONFIG] Web search API key test failed for {:?}: {}", provider, e);
                                    println!("   ⚠️  API key test failed: {}", e);
                                    println!("   The key was saved, but may not work correctly.");
                                }
                            }
                        } else {
                            println!("   ℹ️  DuckDuckGo doesn't require an API key");
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                    }
                }
            }
            3 => {
                // Configure extra parameters
                handle_web_search_extra_params(config).await?;
            }
            _ => return Ok(true),
        }
    }
}

/// Handle web search extra parameters configuration
async fn handle_web_search_extra_params(config: &mut Config) -> Result<()> {
    use mylm_core::config::SearchProvider;
    use dialoguer::{Input, Select, Confirm};
    
    let provider = config.active_profile().web_search.provider.clone();
    
    // Define which parameters are supported by each provider
    // All parameters are shown, but unsupported ones are disabled
    let supported_params: std::collections::HashMap<&str, Vec<SearchProvider>> = [
        ("type", vec![SearchProvider::Exa]),
        ("numResults", vec![SearchProvider::Exa, SearchProvider::Serpapi]),
        ("category", vec![SearchProvider::Exa]),
        ("maxAgeHours", vec![SearchProvider::Exa]),
        ("includeDomains", vec![SearchProvider::Exa]),
        ("excludeDomains", vec![SearchProvider::Exa]),
        ("contents.text", vec![SearchProvider::Exa]),
        ("contents.highlights.maxCharacters", vec![SearchProvider::Exa]),
    ].into_iter().collect();
    
    let is_supported = |param: &str| -> bool {
        supported_params.get(param).map(|providers| providers.contains(&provider)).unwrap_or(false)
    };
    
    loop {
        print!("\x1B[2J\x1B[1;1H");
        println!("\n{}", Style::new().bold().apply_to("Web Search Extra Parameters"));
        println!("{}", Style::new().dim().apply_to("─".repeat(50)));
        println!("Provider: {:?}", provider);
        println!("{}", Style::new().dim().apply_to("(Parameters marked with ✗ are not supported by this provider)"));
        println!();
        
        // Get current extra params
        let extra_params = config.active_profile().web_search.extra_params.clone().unwrap_or_default();
        
        // Build menu items showing current values
        let mut menu_items = vec![];
        let mut param_keys = vec![];
        
        macro_rules! add_param {
            ($key:expr, $label:expr, $default:expr) => {
                let supported = is_supported($key);
                let current = extra_params.get($key).cloned().unwrap_or_else(|| $default.to_string());
                let status = if supported { "✓" } else { "✗" };
                menu_items.push(format!("{} {}: {} (current: {})", status, $label, if supported { "" } else { "[not supported]" }, current));
                param_keys.push($key);
            };
        }
        
        add_param!("type", "Search Type (auto/instant/deep)", "auto");
        add_param!("numResults", "Number of Results", "5");
        add_param!("category", "Category Filter", "none");
        add_param!("maxAgeHours", "Max Age Hours (-1=cache, 0=live)", "default");
        add_param!("includeDomains", "Include Domains (comma-separated)", "");
        add_param!("excludeDomains", "Exclude Domains (comma-separated)", "");
        add_param!("contents.text", "Use Full Text (true/false)", "false");
        add_param!("contents.highlights.maxCharacters", "Highlight Max Characters", "2000");
        
        menu_items.push("Clear All Parameters".to_string());
        menu_items.push("Back".to_string());
        
        let selection = Select::new()
            .with_prompt("Select parameter to configure")
            .items(&menu_items)
            .default(0)
            .interact()?;
        
        if selection == menu_items.len() - 1 {
            // Back
            return Ok(());
        }
        
        if selection == menu_items.len() - 2 {
            // Clear all parameters
            let profile = config.active_profile_mut();
            profile.web_search.extra_params = None;
            config.save_default()?;
            println!("\n✅ All extra parameters cleared");
            tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
            continue;
        }
        
        let param_key = param_keys[selection];
        
        if !is_supported(param_key) {
            println!("\n⚠️  This parameter is not supported by {:?}", provider);
            println!("   Supported providers: {:?}", supported_params.get(param_key).unwrap_or(&vec![]));
            tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
            continue;
        }
        
        // Get current value
        let current_value = extra_params.get(param_key).cloned().unwrap_or_default();
        
        // Prompt for new value
        let new_value: String = Input::new()
            .with_prompt(format!("Enter value for '{}' (leave empty to remove)", param_key))
            .allow_empty(true)
            .default(current_value)
            .interact()?;
        
        // Update the config
        let profile = config.active_profile_mut();
        let params = profile.web_search.extra_params.get_or_insert_with(std::collections::HashMap::new);
        
        if new_value.is_empty() {
            params.remove(param_key);
        } else {
            params.insert(param_key.to_string(), new_value);
        }
        
        config.save_default()?;
        println!("\n✅ Parameter '{}' updated", param_key);
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}

/// Test web search API key by making a simple request
async fn test_web_search_api_key(provider: &SearchProvider, api_key: &str) -> Result<()> {
    use mylm_core::agent::runtime::tools::web_search::WebSearchTool;
    use mylm_core::agent::runtime::tools::web_search::WebSearchConfig;
    use mylm_core::agent::runtime::capability::ToolCapability;
    use mylm_core::agent::types::intents::ToolCall;
    
    log::debug!("[CONFIG] Testing web search API key for provider {:?}", provider);
    
    let config = WebSearchConfig {
        enabled: true,
        api_key: Some(api_key.to_string()),
        provider: match provider {
            SearchProvider::DuckDuckGo => mylm_core::agent::runtime::tools::web_search::SearchProvider::DuckDuckGo,
            SearchProvider::Serpapi => mylm_core::agent::runtime::tools::web_search::SearchProvider::SerpApi,
            SearchProvider::Brave => mylm_core::agent::runtime::tools::web_search::SearchProvider::Brave,
            SearchProvider::Openai => mylm_core::agent::runtime::tools::web_search::SearchProvider::OpenAi,
            SearchProvider::Exa => mylm_core::agent::runtime::tools::web_search::SearchProvider::Exa,
            SearchProvider::Google => mylm_core::agent::runtime::tools::web_search::SearchProvider::Google,
            SearchProvider::Tavily => mylm_core::agent::runtime::tools::web_search::SearchProvider::Tavily,
            SearchProvider::Kimi => mylm_core::agent::runtime::tools::web_search::SearchProvider::DuckDuckGo, // Fallback
            SearchProvider::Custom => mylm_core::agent::runtime::tools::web_search::SearchProvider::Custom,
        },
    };
    
    let tool = WebSearchTool::with_config(config);
    let call = ToolCall {
        name: "web_search".to_string(),
        arguments: serde_json::json!("test query"),
        working_dir: None,
        timeout_secs: Some(10),
    };
    
    // Create a dummy runtime context
    let ctx = mylm_core::agent::runtime::context::RuntimeContext::default();
    
    match tool.execute(&ctx, call).await {
        Ok(result) => {
            match result {
                mylm_core::agent::types::events::ToolResult::Success { .. } => Ok(()),
                mylm_core::agent::types::events::ToolResult::Error { message, .. } => {
                    Err(anyhow::anyhow!("{}", message))
                }
                mylm_core::agent::types::events::ToolResult::Cancelled => {
                    Err(anyhow::anyhow!("Request was cancelled"))
                }
            }
        }
        Err(e) => Err(anyhow::anyhow!("Request failed: {}", e)),
    }
}

/// Set max context tokens
pub fn set_max_tokens(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { config.active_profile.clone() } else { "worker".to_string() };
    let current = config.profiles.get(&profile_name)
        .map(|p| p.context_window)
        .unwrap_or(8192);
    
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Max Context Tokens"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    println!("\n  Current value: {}", Style::new().green().apply_to(current));
    println!("  {}\n", Style::new().dim().apply_to("(Size of the context window in tokens)"));
    
    let new_value: usize = Input::new()
        .with_prompt("New value (e.g., 4096, 8192, 32768, 128000)")
        .default(current.to_string())
        .interact()?
        .parse()
        .unwrap_or(current);
    
    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }
    
    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.context_window = new_value;
        config.save_default()?;
        println!("\n✅ Max context tokens set to {}", new_value);
    }
    Ok(true)
}

/// Set condense threshold
pub fn set_condense_threshold(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { config.active_profile.clone() } else { "worker".to_string() };
    
    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }
    
    let current = config.profiles.get(&profile_name)
        .and_then(|p| p.condense_threshold)
        .unwrap_or(0);
    
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Condense Threshold"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    let current_display = if current == 0 { "Disabled".to_string() } else { current.to_string() };
    println!("\n  Current value: {}", Style::new().green().apply_to(&current_display));
    println!("  {}\n", Style::new().dim().apply_to("(When to condense conversation history, 0 = disabled)"));
    
    let input: String = Input::new()
        .with_prompt("New value (0 = disabled, e.g., 4000)")
        .default(current.to_string())
        .interact()?;
    
    let new_value: usize = input.parse().unwrap_or(0);
    
    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.condense_threshold = if new_value == 0 { None } else { Some(new_value) };
        config.save_default()?;
        if new_value == 0 {
            println!("\n✅ Condense threshold disabled");
        } else {
            println!("\n✅ Condense threshold set to {}", new_value);
        }
    }
    Ok(true)
}

/// Set input price
pub fn set_input_price(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { config.active_profile.clone() } else { "worker".to_string() };
    
    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }
    
    let current = config.profiles.get(&profile_name)
        .and_then(|p| p.input_price)
        .unwrap_or(0.0);
    
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Input Price"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    let current_display = if current == 0.0 { "Not set".to_string() } else { format!("${:.2}", current) };
    println!("\n  Current value: {}", Style::new().green().apply_to(current_display));
    println!("  {}\n", Style::new().dim().apply_to("(Cost per 1 million input tokens in USD)"));
    
    let input: String = Input::new()
        .with_prompt("New value (USD per 1M tokens, e.g., 0.50, 3.00, 0 = not set)")
        .default(current.to_string())
        .interact()?;
    
    let new_value: f64 = input.parse().unwrap_or(0.0);
    
    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.input_price = if new_value == 0.0 { None } else { Some(new_value) };
        config.save_default()?;
        println!("\n✅ Input price set to ${:.2} per 1M tokens", new_value);
    }
    Ok(true)
}

/// Set output price
pub fn set_output_price(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { config.active_profile.clone() } else { "worker".to_string() };
    
    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }
    
    let current = config.profiles.get(&profile_name)
        .and_then(|p| p.output_price)
        .unwrap_or(0.0);
    
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Output Price"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    let current_display = if current == 0.0 { "Not set".to_string() } else { format!("${:.2}", current) };
    println!("\n  Current value: {}", Style::new().green().apply_to(current_display));
    println!("  {}\n", Style::new().dim().apply_to("(Cost per 1 million output tokens in USD)"));
    
    let input: String = Input::new()
        .with_prompt("New value (USD per 1M tokens, e.g., 1.50, 15.00, 0 = not set)")
        .default(current.to_string())
        .interact()?;
    
    let new_value: f64 = input.parse().unwrap_or(0.0);
    
    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.output_price = if new_value == 0.0 { None } else { Some(new_value) };
        config.save_default()?;
        println!("\n✅ Output price set to ${:.2} per 1M tokens", new_value);
    }
    Ok(true)
}

/// Set rate limit (RPM)
pub fn set_rate_limit_rpm(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { config.active_profile.clone() } else { "worker".to_string() };
    
    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }
    
    let current = config.profiles.get(&profile_name)
        .map(|p| p.rate_limit_rpm)
        .unwrap_or(60);
    
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Rate Limit (RPM)"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    let current_display = if current == 0 { "Unlimited".to_string() } else { format!("{} RPM", current) };
    println!("\n  Current value: {}", Style::new().green().apply_to(current_display));
    println!("  {}\n", Style::new().dim().apply_to("(Maximum API requests per minute, 0 = unlimited)"));
    
    let input: String = Input::new()
        .with_prompt("New value (requests per minute, 0 = unlimited)")
        .default(current.to_string())
        .interact()?;
    
    let new_value: u32 = input.parse().unwrap_or(60);
    
    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.rate_limit_rpm = new_value;
        config.save_default()?;
        if new_value == 0 {
            println!("\n✅ Rate limit disabled (unlimited)");
        } else {
            println!("\n✅ Rate limit set to {} requests per minute", new_value);
        }
    }
    Ok(true)
}

/// STUB: Set always allowed commands
pub fn set_allowed_commands(_config: &mut Config, _is_main: bool) -> Result<bool> {
    println!("\n[STUB] set_allowed_commands - to be implemented\n");
    Ok(true)
}

/// STUB: Set always restricted commands
pub fn set_restricted_commands(_config: &mut Config, _is_main: bool) -> Result<bool> {
    println!("\n[STUB] set_restricted_commands - to be implemented\n");
    Ok(true)
}

/// STUB: Toggle PaCoRe enabled
pub fn toggle_pacore_enabled(_config: &mut Config, _is_main: bool) -> Result<bool> {
    println!("\n[STUB] toggle_pacore_enabled - to be implemented\n");
    Ok(true)
}

/// STUB: Set PaCoRe rounds
pub fn set_pacore_rounds(_config: &mut Config, _is_main: bool) -> Result<bool> {
    println!("\n[STUB] set_pacore_rounds - to be implemented\n");
    Ok(true)
}

/// STUB: Set max actions before stall
pub fn set_max_actions_before_stall(_config: &mut Config, _is_main: bool) -> Result<bool> {
    println!("\n[STUB] set_max_actions_before_stall - to be implemented\n");
    Ok(true)
}



/// ============================================================================
/// UTILITY FUNCTIONS
/// ============================================================================

/// Check if tmux is available
pub fn is_tmux_available() -> bool {
    which::which("tmux").is_ok()
}

/// Check if session file exists
pub fn session_exists() -> bool {
    dirs::data_dir()
        .map(|d| d.join("mylm").join("sessions").join("latest.json"))
        .map(|p| p.exists())
        .unwrap_or(false)
}



/// Print hub banner
fn print_hub_banner() {
    // Clear screen to prevent leftover content from previous menus
    print!("\x1B[2J\x1B[1;1H");
    
    let blue = Style::new().blue().bold();
    let dim = Style::new().dim();
    let cyan = Style::new().cyan();
    
    // Build info from build.rs
    let build_number = env!("BUILD_NUMBER");
    let git_hash = env!("GIT_HASH");
    
    println!();
    println!("  {} {}  {} {}", 
        blue.apply_to("◉ mylm"), 
        dim.apply_to("v3"),
        cyan.apply_to(format!("(build {})", build_number)),
        dim.apply_to(format!("[{}]", git_hash))
    );
    println!("  {}", dim.apply_to("Terminal AI Assistant"));
    println!();
}

/// Print config banner
fn print_config_banner(config: &Config) {
    print!("\x1B[2J\x1B[1;1H");
    
    let blue = Style::new().blue().bold();
    let dim = Style::new().dim();
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let red = Style::new().red();
    
    // Helper to format test status with optional error
    let format_test_status = |config: &Config, profile_name: &str| -> Vec<String> {
        let mut lines = Vec::new();
        if let Some((tested, has_error, error_msg)) = config.get_profile_test_status(profile_name) {
            if tested && !has_error {
                lines.push(format!("     Status: {}", green.apply_to("✅ Working")));
            } else if tested && has_error {
                lines.push(format!("     Status: {}", red.apply_to("❌ Error")));
                if let Some(msg) = error_msg {
                    // Truncate long error messages
                    let display_msg = if msg.len() > 50 {
                        format!("{}...", &msg[..50])
                    } else {
                        msg.to_string()
                    };
                    lines.push(format!("     Error: {}", red.apply_to(display_msg)));
                }
            } else {
                lines.push(format!("     Status: {}", yellow.apply_to("⚠️ Untested")));
            }
        } else {
            lines.push(format!("     Status: {}", red.apply_to("❌ Not Configured")));
        }
        lines
    };
    
    println!();
    println!("  {} {}", blue.apply_to("⚙️  Configuration"), dim.apply_to("─".repeat(50)));
    
    // === MAIN LLM ===
    let main_profile = config.active_profile();
    let main_provider = &main_profile.provider;
    let main_model = main_profile.model.clone().unwrap_or_else(|| "Not set".to_string());
    
    println!();
    println!("  {} {}", yellow.apply_to("🧠 Main LLM"), dim.apply_to("─".repeat(40)));
    println!("     Provider: {}", green.apply_to(main_provider));
    println!("     Model: {}", green.apply_to(&main_model));
    for line in format_test_status(config, &config.active_profile) {
        println!("{}", line);
    }
    println!("     Context: {} tokens", green.apply_to(main_profile.context_window));
    if main_profile.condense_threshold.unwrap_or(0) > 0 {
        println!("     Condense: {} tokens", green.apply_to(main_profile.condense_threshold.unwrap()));
    }
    if main_profile.rate_limit_rpm > 0 {
        println!("     Rate Limit: {} RPM", green.apply_to(main_profile.rate_limit_rpm));
    }
    if let Some(price) = main_profile.input_price {
        if price > 0.0 {
            println!("     Cost: ${}/1M in, ${}/1M out", 
                green.apply_to(price), 
                green.apply_to(main_profile.output_price.unwrap_or(0.0)));
        }
    }
    
    // === WORKER LLM ===
    if let Some(worker) = config.profiles.get("worker") {
        println!();
        println!("  {} {}", yellow.apply_to("⚡ Worker LLM"), dim.apply_to("─".repeat(40)));
        println!("     Provider: {}", green.apply_to(&worker.provider));
        println!("     Model: {}", green.apply_to(worker.model.clone().unwrap_or_else(|| "Not set".to_string())));
        for line in format_test_status(config, "worker") {
            println!("{}", line);
        }
        println!("     Context: {} tokens", green.apply_to(worker.context_window));
        if worker.condense_threshold.unwrap_or(0) > 0 {
            println!("     Condense: {} tokens", green.apply_to(worker.condense_threshold.unwrap()));
        }
        if worker.rate_limit_rpm > 0 {
            println!("     Rate Limit: {} RPM", green.apply_to(worker.rate_limit_rpm));
        }
    }
    
    // === WEB SEARCH ===
    println!();
    println!("  {} {}", yellow.apply_to("🌐 Web Search"), dim.apply_to("─".repeat(40)));
    let web_search = if config.features.web_search {
        green.apply_to("Enabled").to_string()
    } else {
        dim.apply_to("Disabled").to_string()
    };
    println!("     Status: {}", web_search);
    
    println!();
}
