use inquire::{Select, error::InquireResult};
use crate::config::Config;
use anyhow::Result;

#[derive(Debug, PartialEq)]
pub enum HubChoice {
    ResumeSession,
    StartTui,
    QuickQuery,
    Configuration,
    Exit,
}

impl std::fmt::Display for HubChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HubChoice::ResumeSession => write!(f, "🔄 Resume Latest Session"),
            HubChoice::StartTui => write!(f, "🚀 Start Fresh TUI Session"),
            HubChoice::QuickQuery => write!(f, "⚡ Quick Query"),
            HubChoice::Configuration => write!(f, "⚙️  Configuration"),
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
            SettingsChoice::Save => write!(f, "💾 Save & Exit"),
            SettingsChoice::Back => write!(f, "⬅️  Discard & Back"),
        }
    }
}

/// Show the interactive hub menu
pub async fn show_hub(_config: &Config) -> Result<HubChoice> {
    let mut options = Vec::new();

    // Check if session file exists
    let session_exists = dirs::data_dir()
        .map(|d| d.join("mylm").join("sessions").join("latest.json").exists())
        .unwrap_or(false);

    if session_exists {
        options.push(HubChoice::ResumeSession);
    }

    options.extend(vec![
        HubChoice::StartTui,
        HubChoice::QuickQuery,
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
        SettingsChoice::Save,
        SettingsChoice::Back,
    ];

    let ans: InquireResult<SettingsChoice> = Select::new(
        &format!(
            "⚙️  Settings Dashboard\n  Profile:  {}\n  Endpoint: {}\n  Search:   {}\n  Prompt:   {}\n",
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
