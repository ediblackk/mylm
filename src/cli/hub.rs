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
pub enum ConfigChoice {
    SelectProfile,
    EditProfile,
    NewProfile,
    Back,
}

impl std::fmt::Display for ConfigChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigChoice::SelectProfile => write!(f, "👤 Select Active Profile"),
            ConfigChoice::EditProfile => write!(f, "📝 Edit Current Profile"),
            ConfigChoice::NewProfile => write!(f, "➕ Create New Profile"),
            ConfigChoice::Back => write!(f, "⬅️  Back to Main Menu"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ProfileEditChoice {
    EditPrompt,
    SelectEndpoint,
    EditEndpointDetails,
    Back,
}

impl std::fmt::Display for ProfileEditChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileEditChoice::EditPrompt => write!(f, "📝 Edit Prompt Instructions"),
            ProfileEditChoice::SelectEndpoint => write!(f, "🔗 Select Associated Endpoint"),
            ProfileEditChoice::EditEndpointDetails => write!(f, "⚙️  Edit Endpoint Details (Model/Key)"),
            ProfileEditChoice::Back => write!(f, "⬅️  Back"),
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

/// Show the configuration sub-menu
pub async fn show_config_menu() -> Result<ConfigChoice> {
    let options = vec![
        ConfigChoice::SelectProfile,
        ConfigChoice::EditProfile,
        ConfigChoice::NewProfile,
        ConfigChoice::Back,
    ];

    let ans: InquireResult<ConfigChoice> = Select::new("Configuration", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(ConfigChoice::Back),
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

/// Show profile edit menu
pub fn show_profile_edit_menu(profile_name: &str) -> Result<ProfileEditChoice> {
    let options = vec![
        ProfileEditChoice::EditPrompt,
        ProfileEditChoice::SelectEndpoint,
        ProfileEditChoice::EditEndpointDetails,
        ProfileEditChoice::Back,
    ];

    let ans: InquireResult<ProfileEditChoice> = Select::new(
        &format!("Editing Profile: {}", profile_name),
        options
    ).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(ProfileEditChoice::Back),
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
