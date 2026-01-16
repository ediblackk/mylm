//! Configuration management using config-rs
//!
//! Supports YAML configuration files with support for multiple endpoints,
//! environment variables, and command-line overrides.
//! test for ccache

use anyhow::{Context, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Password, Select};
use dirs::config_dir;
use home::home_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub mod endpoints;
pub mod prompt;

/// Default configuration file name
const CONFIG_FILE_NAME: &str = "mylm.yaml";

/// Default config directory name
const CONFIG_DIR_NAME: &str = "mylm";

/// A configuration profile combining an endpoint and a prompt
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Profile {
    /// Name of the profile
    pub name: String,
    /// Name of the endpoint to use
    pub endpoint: String,
    /// Name of the prompt file to use (without .md extension)
    pub prompt: String,
    /// Optional model override for this profile (None = use endpoint default)
    #[serde(default)]
    pub model: Option<String>,
}

/// Main configuration structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// Name of the currently active profile
    #[serde(default = "default_profile_name")]
    pub active_profile: String,

    /// List of available profiles
    #[serde(default)]
    pub profiles: Vec<Profile>,

    /// Default endpoint to use when none specified
    #[serde(default = "default_endpoint")]
    pub default_endpoint: String,

    /// List of configured LLM endpoints
    pub endpoints: Vec<endpoints::EndpointConfig>,

    /// Command allowlist settings
    #[serde(default)]
    pub commands: CommandConfig,

    /// Web search configuration
    #[serde(default)]
    pub web_search: WebSearchConfig,

    /// Memory configuration
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Agent configuration
    #[serde(default)]
    pub agent: AgentConfig,

    /// Maximum context tokens to keep in history
    ///
    /// - None => use endpoint/model max context
    /// - Some(n) => override context window to n
    #[serde(default = "default_context_limit")]
    pub context_limit: Option<usize>,

    /// Whether to show intermediate steps (thoughts/actions)
    #[serde(default = "default_verbose_mode")]
    pub verbose_mode: bool,

    /// Context budgeting profile
    #[serde(default)]
    pub context_profile: ContextProfile,
}

/// Context budgeting profile
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ContextProfile {
    /// Minimal context: Core system info only
    Minimal,
    /// Balanced context: Core + Git Summary + Adaptive Memory + Terminal (on demand)
    Balanced,
    /// Verbose context: Full Git + Full Terminal History + Detailed Memory
    Verbose,
}

impl Default for ContextProfile {
    fn default() -> Self {
        ContextProfile::Balanced
    }
}

impl std::fmt::Display for ContextProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextProfile::Minimal => write!(f, "Minimal"),
            ContextProfile::Balanced => write!(f, "Balanced"),
            ContextProfile::Verbose => write!(f, "Verbose"),
        }
    }
}

pub fn default_context_limit() -> Option<usize> {
    None
}

fn default_verbose_mode() -> bool {
    false
}

fn default_profile_name() -> String {
    "default".to_string()
}

/// Web search configuration
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct WebSearchConfig {
    /// Whether web search is enabled
    pub enabled: bool,
    /// Search provider (kimi, google, serpapi)
    pub provider: String,
    /// API key for the search provider
    pub api_key: String,
    /// Model name (specifically for Kimi web search)
    pub model: String,
}

/// Memory configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MemoryConfig {
    /// Whether to automatically record commands and interactions
    #[serde(default = "default_true")]
    pub auto_record: bool,
    /// Whether to inject relevant memories into the context
    #[serde(default = "default_true")]
    pub auto_context: bool,
    /// Whether to automatically categorize new memories
    #[serde(default = "default_true")]
    pub auto_categorize: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            auto_record: true,
            auto_context: true,
            auto_categorize: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Agent version/loop implementation
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum AgentVersion {
    /// Classic ReAct loop (V1)
    V1,
    /// Multi-layered cognitive architecture (V2)
    V2,
}

impl Default for AgentVersion {
    fn default() -> Self {
        AgentVersion::V1
    }
}

impl std::fmt::Display for AgentVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentVersion::V1 => write!(f, "V1 (Classic ReAct)"),
            AgentVersion::V2 => write!(f, "V2 (Cognitive Submodule)"),
        }
    }
}

/// Agent configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentConfig {
    /// Version of the agent loop to use
    #[serde(default)]
    pub version: AgentVersion,
    /// Maximum number of iterations for the agent
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    /// Maximum number of driver loops for the agent
    #[serde(default = "default_max_driver_loops")]
    pub max_driver_loops: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            version: AgentVersion::default(),
            max_iterations: default_max_iterations(),
            max_driver_loops: default_max_driver_loops(),
        }
    }
}

fn default_max_iterations() -> usize {
    10
}

fn default_max_driver_loops() -> usize {
    30
}

/// Command execution configuration
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CommandConfig {
    /// Whether to allow execution of commands
    #[serde(default)]
    pub allow_execution: bool,

    /// Paths to custom allowlist files
    #[serde(default)]
    pub allowlist_paths: Vec<PathBuf>,

    /// Additional explicitly allowed commands (exact command names, e.g. "bash", "git")
    #[serde(default)]
    pub allowed_commands: Vec<String>,

    /// Explicitly blocked commands (exact command names, e.g. "rm")
    #[serde(default)]
    pub blocked_commands: Vec<String>,
}

/// Get the default endpoint name
fn default_endpoint() -> String {
    "default".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            active_profile: default_profile_name(),
            profiles: vec![Profile {
                name: default_profile_name(),
                endpoint: default_endpoint(),
                prompt: "default".to_string(),
                model: None,
            }],
            default_endpoint: default_endpoint(),
            endpoints: Vec::new(),
            commands: CommandConfig::default(),
            web_search: WebSearchConfig::default(),
            memory: MemoryConfig::default(),
            agent: AgentConfig::default(),
            context_limit: None,
            verbose_mode: default_verbose_mode(),
            context_profile: ContextProfile::default(),
        }
    }
}

impl Config {
    /// Load configuration from file, with fallback to defaults
    pub fn load() -> Result<Self> {
        // Try to load from config file
        if let Some(config_path) = find_config_file() {
            if config_path.exists() {
                return Self::load_from_file(&config_path);
            }
        }

        // Try config dir as well
        if let Some(config_dir) = get_config_dir() {
            let config_path = config_dir.join(CONFIG_FILE_NAME);
            if config_path.exists() {
                return Self::load_from_file(&config_path);
            }
        }

        // Return default config if no file found
        Ok(Self::default())
    }

    /// Load configuration from a specific file path
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;

        let config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", path.as_ref()))?;

        Ok(config)
    }

    /// Get endpoint configuration by name
    pub fn get_endpoint(&self, name: Option<&str>) -> Result<&endpoints::EndpointConfig> {
        let name = name.unwrap_or_else(|| {
            self.get_active_profile()
                .map(|p| p.endpoint.as_str())
                .unwrap_or(&self.default_endpoint)
        });

        self.endpoints
            .iter()
            .find(|e| e.name == name)
            .with_context(|| format!("Endpoint '{}' not found in configuration", name))
    }

    /// Get the default endpoint
    #[allow(dead_code)]
    pub fn get_default_endpoint(&self) -> Result<&endpoints::EndpointConfig> {
        self.get_endpoint(Some(&self.default_endpoint))
    }

    /// Get the active profile
    pub fn get_active_profile(&self) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == self.active_profile)
    }

    /// Get the effective model for a profile (profile override or endpoint default)
    pub fn get_effective_model(&self, profile: &Profile) -> Result<String> {
        if let Some(model) = &profile.model {
            return Ok(model.clone());
        }
        
        let endpoint = self.get_endpoint(Some(&profile.endpoint))?;
        Ok(endpoint.model.clone())
    }

    /// Save configuration to file
    #[allow(dead_code)]
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let content = serde_yaml::to_string(self)
            .with_context(|| "Failed to serialize configuration")?;

        fs::write(path.as_ref(), content)
            .with_context(|| format!("Failed to write config file: {:?}", path.as_ref()))?;

        Ok(())
    }

    /// Interactive setup wizard
    /// Edit the LLM endpoint configuration with optional pre-fill from existing endpoint
    pub async fn edit_endpoint_details(&mut self, endpoint_name: &str) -> Result<()> {
        self.edit_endpoint_with_prefill(endpoint_name, None).await
    }

    /// Edit endpoint with optional pre-fill from existing endpoint
    pub async fn edit_endpoint_with_prefill(&mut self, endpoint_name: &str, existing: Option<&endpoints::EndpointConfig>) -> Result<()> {
        let theme = ColorfulTheme::default();
        let providers = vec!["OpenAI", "Google (Gemini)", "Ollama", "OpenRouter", "Custom"];
        
        // Determine provider selection (pre-select from existing if available)
        let provider_idx = if let Some(existing_endpoint) = existing {
            match existing_endpoint.provider.as_str() {
                "google" => 1,
                "openrouter" => 3,
                _ => 0, // Default to OpenAI for unknown/custom
            }
        } else {
            0
        };

        let selection = Select::with_theme(&theme)
            .with_prompt("Select your LLM provider")
            .items(&providers)
            .default(provider_idx)
            .interact()?;

        let provider_name = providers[selection];
        let (provider_id, mut base_url, mut model) = match provider_name {
            "OpenAI" => ("openai".to_string(), "https://api.openai.com/v1".to_string(), "gpt-4o".to_string()),
            "Google (Gemini)" => ("google".to_string(), "https://generativelanguage.googleapis.com".to_string(), "gemini-3-flash-preview".to_string()),
            "Ollama" => ("openai".to_string(), "http://localhost:11434/v1".to_string(), "llama3.2".to_string()),
            "OpenRouter" => ("openai".to_string(), "https://openrouter.ai/api/v1".to_string(), "google/gemini-2.0-flash-001".to_string()),
            _ => ("openai".to_string(), String::new(), String::new()),
        };

        // Pre-fill from existing endpoint if available
        if let Some(existing_endpoint) = existing {
            base_url = existing_endpoint.base_url.clone();
            model = existing_endpoint.model.clone();
        }

        let mut api_key = if provider_name != "Ollama" {
            // For editing, ask if user wants to change API key
            if existing.is_some() {
                let change_key = Confirm::with_theme(&theme)
                    .with_prompt("Change API key? (Leave empty to keep existing)")
                    .default(false)
                    .interact()?;
                
                if change_key {
                    Password::with_theme(&theme)
                        .with_prompt(format!("API Key for {} (leave empty to cancel)", provider_name))
                        .interact()?
                } else {
                    // Keep existing API key
                    if let Some(existing_endpoint) = existing {
                        existing_endpoint.api_key.clone()
                    } else {
                        String::new()
                    }
                }
            } else {
                Password::with_theme(&theme)
                    .with_prompt(format!("API Key for {}", provider_name))
                    .interact()?
            }
        } else {
            "none".to_string()
        };

        // If user left API key empty when editing, keep the existing one
        if provider_name != "Ollama" && api_key.is_empty() && existing.is_some() {
            if let Some(existing_endpoint) = existing {
                api_key = existing_endpoint.api_key.clone();
            }
        }

        let mut fetched_models = Vec::new();
        if provider_name != "Custom" {
            let fetch = Select::with_theme(&theme)
                .with_prompt("Fetch latest models from provider?")
                .items(&["Yes", "No"])
                .default(0)
                .interact()?;

            if fetch == 0 {
                println!("📡 Fetching models...");
                match fetch_models_from_provider(provider_name, &base_url, &api_key, "").await {
                    Ok(models) if !models.is_empty() => {
                        let m_idx = Select::with_theme(&theme)
                            .with_prompt("Select model")
                            .items(&models)
                            .default(0)
                            .interact()?;
                        model = models[m_idx].clone();
                        fetched_models = models;
                    }
                    _ => println!("⚠️ Could not fetch models, using default."),
                }
            }
        }

        if provider_name == "Custom" || fetched_models.is_empty() {
            model = Input::with_theme(&theme).with_prompt("Model name").with_initial_text(model).interact_text()?;
            if provider_name != "Ollama" && provider_name != "Google (Gemini)" {
                base_url = Input::with_theme(&theme).with_prompt("Base URL").with_initial_text(base_url).interact_text()?;
            }
        }

        let mut timeout_seconds = 60;
        let mut input_price_per_1m = 0.0;
        let mut output_price_per_1m = 0.0;
        let mut max_context_tokens = 32768;
        let mut condense_threshold = 0.8;

        if Confirm::with_theme(&theme)
            .with_prompt("Configure advanced settings (tokens, prices, timeout)?")
            .default(false)
            .interact()?
        {
            timeout_seconds = Input::with_theme(&theme)
                .with_prompt("Request timeout (seconds)")
                .default(timeout_seconds)
                .interact_text()?;

            input_price_per_1m = Input::with_theme(&theme)
                .with_prompt("Input price per 1M tokens ($)")
                .default(input_price_per_1m)
                .interact_text()?;

            output_price_per_1m = Input::with_theme(&theme)
                .with_prompt("Output price per 1M tokens ($)")
                .default(output_price_per_1m)
                .interact_text()?;

            max_context_tokens = Input::with_theme(&theme)
                .with_prompt("Max context tokens")
                .default(max_context_tokens)
                .interact_text()?;

            condense_threshold = Input::with_theme(&theme)
                .with_prompt("Condense threshold (0.0 - 1.0)")
                .default(condense_threshold)
                .interact_text()?;
        }

        let endpoint = endpoints::EndpointConfig {
            name: endpoint_name.to_string(),
            provider: provider_id,
            base_url,
            model,
            api_key,
            timeout_seconds,
            input_price_per_1m,
            output_price_per_1m,
            max_context_tokens,
            condense_threshold,
        };

        // Update or add the endpoint
        if let Some(e) = self.endpoints.iter_mut().find(|e| e.name == endpoint_name) {
            *e = endpoint;
        } else {
            self.endpoints.push(endpoint);
        }

        Ok(())
    }

    /// Edit only the provider for an endpoint
    pub async fn edit_endpoint_provider(&mut self, endpoint_name: &str) -> Result<()> {
        let theme = ColorfulTheme::default();
        let providers = vec!["OpenAI", "Google (Gemini)", "Ollama", "OpenRouter", "Custom"];
        
        let current_idx = if let Ok(e) = self.get_endpoint(Some(endpoint_name)) {
             match e.provider.as_str() {
                "google" => 1,
                "openrouter" => 3,
                _ => 0,
            }
        } else {
            0
        };

        let selection = Select::with_theme(&theme)
            .with_prompt("Select Provider")
            .items(&providers)
            .default(current_idx)
            .interact()?;
            
        let provider_name = providers[selection];
        let (provider_id, default_url) = match provider_name {
            "OpenAI" => ("openai".to_string(), "https://api.openai.com/v1".to_string()),
            "Google (Gemini)" => ("google".to_string(), "https://generativelanguage.googleapis.com".to_string()),
            "Ollama" => ("openai".to_string(), "http://localhost:11434/v1".to_string()),
            "OpenRouter" => ("openai".to_string(), "https://openrouter.ai/api/v1".to_string()),
            _ => ("openai".to_string(), String::new()),
        };

        if let Some(e) = self.endpoints.iter_mut().find(|e| e.name == endpoint_name) {
            e.provider = provider_id;
            // Optionally update base URL if it looks like a default one, or ask user?
            // To be safe/simple, we offer to update it if it's different.
            if e.base_url != default_url && !default_url.is_empty() {
                if Confirm::with_theme(&theme)
                    .with_prompt(format!("Update Base URL to default for {} ({})?", provider_name, default_url))
                    .default(true)
                    .interact()?
                {
                    e.base_url = default_url;
                }
            }
        }
        Ok(())
    }

    /// Edit only the Base URL for an endpoint
    pub fn edit_endpoint_base_url(&mut self, endpoint_name: &str) -> Result<()> {
        let theme = ColorfulTheme::default();
        let current_url = if let Ok(e) = self.get_endpoint(Some(endpoint_name)) {
            e.base_url.clone()
        } else {
            String::new()
        };

        let new_url: String = Input::with_theme(&theme)
            .with_prompt("API Base URL")
            .with_initial_text(&current_url)
            .interact_text()?;

        if let Some(e) = self.endpoints.iter_mut().find(|e| e.name == endpoint_name) {
            e.base_url = new_url;
        }
        Ok(())
    }

    /// Edit only the API Key for an endpoint
    pub fn edit_endpoint_api_key(&mut self, endpoint_name: &str) -> Result<()> {
        let theme = ColorfulTheme::default();
        let current_key = if let Ok(e) = self.get_endpoint(Some(endpoint_name)) {
            e.api_key.clone()
        } else {
            String::new()
        };
        
        // Show current status
        if current_key == "none" || current_key.is_empty() {
            println!("Current Key: Not Set");
        } else {
            println!("Current Key: ******** (Set)");
        }

        let new_key = Password::with_theme(&theme)
            .with_prompt("New API Key (leave empty to keep existing)")
            .allow_empty_password(true)
            .interact()?;

        if !new_key.is_empty() {
            if let Some(e) = self.endpoints.iter_mut().find(|e| e.name == endpoint_name) {
                e.api_key = new_key;
            }
        }
        Ok(())
    }

    /// Edit the Model for the active profile (Profile override)
    pub async fn edit_profile_model(&mut self, profile_name: &str) -> Result<()> {
        let theme = ColorfulTheme::default();
        
        // Need to find which endpoint is linked to this profile to fetch models
        let endpoint_name = if let Some(p) = self.profiles.iter().find(|p| p.name == profile_name) {
            p.endpoint.clone()
        } else {
            return Ok(());
        };

        let (provider, base_url, api_key) = if let Ok(e) = self.get_endpoint(Some(&endpoint_name)) {
            (e.provider.clone(), e.base_url.clone(), e.api_key.clone())
        } else {
            return Ok(());
        };

        // Options: Fetch list, Type manually, Use Connection Default
        let options = vec![
            "📡 Fetch from Provider",
            "⌨️  Type Manually",
            "🔙 Revert to Connection Default"
        ];
        
        let sel = Select::with_theme(&theme)
            .with_prompt("Select Model Method")
            .items(&options)
            .default(0)
            .interact()?;

        let new_model = match sel {
            0 => {
                println!("📡 Fetching models from {}...", base_url);
                match fetch_models_from_provider(&provider, &base_url, &api_key, "").await {
                    Ok(models) if !models.is_empty() => {
                        let m_idx = Select::with_theme(&theme)
                            .with_prompt("Select Model")
                            .items(&models)
                            .default(0)
                            .interact()?;
                        Some(models[m_idx].clone())
                    }
                    Ok(_) => {
                        println!("⚠️  No models found.");
                        None
                    }
                    Err(e) => {
                        println!("⚠️  Fetch failed: {}", e);
                        None
                    }
                }
            }
            1 => {
                let current_model = if let Some(p) = self.profiles.iter().find(|p| p.name == profile_name) {
                    p.model.clone().unwrap_or_default()
                } else { String::new() };
                
                let val: String = Input::with_theme(&theme)
                    .with_prompt("Enter Model ID")
                    .with_initial_text(&current_model)
                    .interact_text()?;
                Some(val)
            }
            2 => None, // Set to None (use default)
            _ => None,
        };

        // Apply change
        // Logic: if user selected 0/1 but cancelled/failed, we don't change anything.
        // If user selected 2, we set to None.
        if sel == 2 {
             if let Some(p) = self.profiles.iter_mut().find(|p| p.name == profile_name) {
                p.model = None;
                println!("✅ Reverted to connection default model.");
            }
        } else if let Some(model) = new_model {
            if let Some(p) = self.profiles.iter_mut().find(|p| p.name == profile_name) {
                p.model = Some(model);
                println!("✅ Updated profile model override.");
            }
        }

        Ok(())
    }

    /// Edit General settings (context limit, verbose mode, etc.)
    pub fn edit_general(&mut self) -> Result<()> {
        let theme = ColorfulTheme::default();
        
        loop {
            let options = vec![
                format!(
                    "Context Limit: {}",
                    self.context_limit
                        .map(|l| l.to_string())
                        .unwrap_or_else(|| "Model Default".to_string())
                ),
                format!("Verbose Mode: {}", if self.verbose_mode { "On" } else { "Off" }),
                format!("Auto-approve:  {}", if self.commands.allow_execution { "Enabled" } else { "Disabled" }),
                format!("Allowed Commands: {}", self.commands.allowed_commands.len()),
                format!("Blocked Commands: {}", self.commands.blocked_commands.len()),
                format!("Agent Version:  {}", self.agent.version),
                format!("Max Iterations: {} (steps per request)", self.agent.max_iterations),
                format!("Max Driver Loops: {} (session safety limit)", self.agent.max_driver_loops),
                format!("Context Profile: {}", self.context_profile),
                format!("Auto-Memory:   {}", if self.memory.auto_record { "Enabled" } else { "Disabled" }),
                format!("Auto-Categorize: {}", if self.memory.auto_categorize { "Enabled" } else { "Disabled" }),
                "⬅️  Back".to_string(),
            ];

            let selection = Select::with_theme(&theme)
                .with_prompt("General Settings")
                .items(&options)
                .default(0)
                .interact()?;

            match selection {
                0 => {
                    let current = self.context_limit.unwrap_or(0);
                    let val: String = Input::with_theme(&theme)
                        .with_prompt("Global context limit (0 to use model default)")
                        .with_initial_text(current.to_string())
                        .interact_text()?;

                    let n = val.parse::<usize>().unwrap_or(0);
                    if n == 0 {
                        self.context_limit = None;
                    } else {
                        self.context_limit = Some(n);
                    }
                }
                1 => {
                    self.verbose_mode = !self.verbose_mode;
                }
                2 => {
                    self.commands.allow_execution = !self.commands.allow_execution;
                }
                3 => {
                    edit_string_list(
                        &theme,
                        "Allowed Commands (exact command names)",
                        &mut self.commands.allowed_commands,
                    )?;
                }
                4 => {
                    edit_string_list(
                        &theme,
                        "Blocked Commands (exact command names)",
                        &mut self.commands.blocked_commands,
                    )?;
                }
                5 => {
                    let mut versions = vec![AgentVersion::V1];
                    if check_v2_binary_exists() {
                        versions.push(AgentVersion::V2);
                    }
                    
                    if versions.len() > 1 {
                        let items: Vec<String> = versions.iter().map(|v| v.to_string()).collect();
                        let current_idx = versions.iter().position(|v| v == &self.agent.version).unwrap_or(0);
                        
                        let idx = Select::with_theme(&theme)
                            .with_prompt("Select Agent Version")
                            .items(&items)
                            .default(current_idx)
                            .interact()?;
                        self.agent.version = versions[idx];
                    } else {
                        println!("ℹ️ Only V1 is available. V2 binary ('mylm-v2') not found.");
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                }
                6 => {
                    self.agent.max_iterations = Input::with_theme(&theme)
                        .with_prompt("Max steps (thoughts/actions) per single user request")
                        .default(self.agent.max_iterations)
                        .interact_text()?;
                }
                7 => {
                    self.agent.max_driver_loops = Input::with_theme(&theme)
                        .with_prompt("Safety limit: max total exchanges in one session")
                        .default(self.agent.max_driver_loops)
                        .interact_text()?;
                }
                8 => {
                    let profiles = vec![
                        ContextProfile::Minimal,
                        ContextProfile::Balanced,
                        ContextProfile::Verbose,
                    ];
                    let items: Vec<String> = profiles.iter().map(|p| p.to_string()).collect();
                    let idx = Select::with_theme(&theme)
                        .with_prompt("Select Context Profile")
                        .items(&items)
                        .default(1)
                        .interact()?;
                    self.context_profile = profiles[idx];
                }
                9 => {
                    self.memory.auto_record = !self.memory.auto_record;
                    self.memory.auto_context = self.memory.auto_record;
                    self.memory.auto_categorize = self.memory.auto_record;
                }
                10 => {
                    self.memory.auto_categorize = !self.memory.auto_categorize;
                }
                _ => break,
            }
        }

        Ok(())
    }

    /// Edit API keys for LLM or Search
    pub fn edit_api_key(&mut self, search: bool, endpoint_name: Option<&str>) -> Result<()> {
        let theme = ColorfulTheme::default();
        let prompt = if search { "Search API Key" } else { "LLM API Key" };
        let key = Password::with_theme(&theme)
            .with_prompt(prompt)
            .interact()?;

        if search {
            self.web_search.api_key = key;
        } else if let Some(name) = endpoint_name {
            if let Some(e) = self.endpoints.iter_mut().find(|e| e.name == name) {
                e.api_key = key;
            }
        } else if let Some(e) = self.endpoints.iter_mut().find(|e| e.name == "default") {
            e.api_key = key;
        }

        Ok(())
    }

    /// Edit Web Search configuration
    pub async fn edit_search(&mut self) -> Result<()> {
        let theme = ColorfulTheme::default();
        let providers = vec!["Kimi (Moonshot AI)", "SerpAPI (Google/Bing/etc.)", "Disabled"];
        let selection = Select::with_theme(&theme)
            .with_prompt("Select web search provider")
            .items(&providers)
            .default(0)
            .interact()?;

        match selection {
            0 => {
                self.web_search.enabled = true;
                self.web_search.provider = "kimi".to_string();
                self.web_search.api_key = Password::with_theme(&theme).with_prompt("Kimi API Key").interact()?;
                self.web_search.model = "kimi-k2-turbo-preview".to_string();
            }
            1 => {
                self.web_search.enabled = true;
                self.web_search.provider = "serpapi".to_string();
                self.web_search.api_key = Password::with_theme(&theme).with_prompt("SerpAPI Key").interact()?;
            }
            _ => {
                self.web_search.enabled = false;
            }
        }
        Ok(())
    }
}

fn edit_string_list(theme: &ColorfulTheme, title: &str, list: &mut Vec<String>) -> Result<()> {
    loop {
        let mut options: Vec<String> = Vec::new();
        options.push("➕ Add".to_string());
        if !list.is_empty() {
            options.push("➖ Remove".to_string());
            options.push("🧹 Clear".to_string());
        }
        options.push("⬅️  Back".to_string());

        let sel = Select::with_theme(theme)
            .with_prompt(title)
            .items(&options)
            .default(0)
            .interact()?;

        let choice = options.get(sel).map(|s| s.as_str()).unwrap_or("⬅️  Back");
        match choice {
            "➕ Add" => {
                let val: String = Input::with_theme(theme)
                    .with_prompt("Command name")
                    .interact_text()?;
                let v = val.trim();
                if !v.is_empty() {
                    if !list.iter().any(|x| x == v) {
                        list.push(v.to_string());
                    }
                }
            }
            "➖ Remove" => {
                if list.is_empty() {
                    continue;
                }
                let items = list.clone();
                let idx = Select::with_theme(theme)
                    .with_prompt("Remove which?")
                    .items(&items)
                    .default(0)
                    .interact()?;
                if idx < list.len() {
                    list.remove(idx);
                }
            }
            "🧹 Clear" => {
                if dialoguer::Confirm::with_theme(theme)
                    .with_prompt("Clear all entries?")
                    .default(false)
                    .interact()?
                {
                    list.clear();
                }
            }
            _ => break,
        }
    }
    Ok(())
}

/// Find the configuration file in standard locations
pub fn find_config_file() -> Option<PathBuf> {
    // Check current directory first
    if let Ok(cwd) = std::env::current_dir() {
        let path = cwd.join(CONFIG_FILE_NAME);
        if path.exists() {
            return Some(path);
        }
    }

    // Check config directory
    get_config_dir().map(|dir| dir.join(CONFIG_FILE_NAME))
}

/// Get the configuration directory
fn get_config_dir() -> Option<PathBuf> {
    // Try XDG config dir first
    if let Some(dir) = config_dir() {
        let path = dir.join(CONFIG_DIR_NAME);
        if path.exists() {
            return Some(path);
        }
    }

    // Fall back to home directory
    if let Some(home) = home_dir() {
        let path = home.join(".config").join(CONFIG_DIR_NAME);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Check if V2 plugin binary exists
pub fn check_v2_binary_exists() -> bool {
    let binary_name = if cfg!(windows) { "mylm-v2.exe" } else { "mylm-v2" };
    
    let paths = [
        PathBuf::from(binary_name),
        PathBuf::from("./").join(binary_name),
        PathBuf::from("plugins").join(binary_name),
    ];
    
    paths.iter().any(|p| p.exists())
}

/// Helper to fetch models from various providers
async fn fetch_models_from_provider(
    provider: &str,
    base_url: &str,
    api_key: &str,
    filter: &str,
) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    let mut models = Vec::new();

    match provider {
        "Google (Gemini)" => {
            let url = format!("{}/v1beta/models?key={}", base_url, api_key);
            let resp = client
                .get(url)
                .send()
                .await?
                .json::<serde_json::Value>()
                .await?;
            if let Some(model_list) = resp.get("models").and_then(|m| m.as_array()) {
                for m in model_list {
                    if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                        // Strip "models/" prefix
                        models.push(name.replace("models/", ""));
                    }
                }
            }
        }
        "OpenAI" | "OpenRouter" | "Ollama" => {
            let url = format!("{}/models", base_url);
            let mut req = client.get(url);
            if api_key != "none" && !api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", api_key));
            }
            let resp = req.send().await?.json::<serde_json::Value>().await?;
            if let Some(data) = resp.get("data").and_then(|d| d.as_array()) {
                for m in data {
                    if let Some(id) = m.get("id").and_then(|i| i.as_str()) {
                        models.push(id.to_string());
                    }
                }
            } else if let Some(model_list) = resp.get("models").and_then(|m| m.as_array()) {
                // Ollama format
                for m in model_list {
                    if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                        models.push(name.to_string());
                    }
                }
            }
        }
        _ => {}
    }

    // Apply search filter
    if !filter.is_empty() {
        let f = filter.to_lowercase();
        models.retain(|m| m.to_lowercase().contains(&f));
    }

    // Sort and filter for chat models if possible (simple heuristic)
    models.sort();
    Ok(models)
}

/// Create default configuration with Ollama as default endpoint
#[allow(dead_code)]
pub fn create_default_config() -> Config {
    Config {
        active_profile: "ollama".to_string(),
        profiles: vec![
            Profile {
                name: "ollama".to_string(),
                endpoint: "ollama".to_string(),
                prompt: "default".to_string(),
                model: None,
            },
            Profile {
                name: "openai".to_string(),
                endpoint: "openai".to_string(),
                prompt: "default".to_string(),
                model: None,
            },
        ],
        default_endpoint: "ollama".to_string(),
        endpoints: vec![
            endpoints::EndpointConfig {
                name: "ollama".to_string(),
                provider: "openai".to_string(),
                base_url: "http://localhost:11434/v1".to_string(),
                model: "llama3.2".to_string(),
                api_key: "none".to_string(),
                timeout_seconds: 120,
                input_price_per_1m: 0.0,
                output_price_per_1m: 0.0,
                max_context_tokens: 32768,
                condense_threshold: 0.8,
            },
            endpoints::EndpointConfig {
                name: "openai".to_string(),
                provider: "openai".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4o".to_string(),
                api_key: "".to_string(),
                timeout_seconds: 60,
                input_price_per_1m: 2.5,
                output_price_per_1m: 10.0,
                max_context_tokens: 128000,
                condense_threshold: 0.8,
            },
            endpoints::EndpointConfig {
                name: "lm-studio".to_string(),
                provider: "openai".to_string(),
                base_url: "http://localhost:1234/v1".to_string(),
                model: "qwen2.5-3b".to_string(),
                api_key: "none".to_string(),
                timeout_seconds: 120,
                input_price_per_1m: 0.0,
                output_price_per_1m: 0.0,
                max_context_tokens: 32768,
                condense_threshold: 0.8,
            },
        ],
        commands: CommandConfig {
            allow_execution: false,
            allowlist_paths: vec![],
            allowed_commands: vec![],
            blocked_commands: vec![],
        },
        web_search: WebSearchConfig::default(),
        memory: MemoryConfig::default(),
        agent: AgentConfig::default(),
        context_limit: None,
        verbose_mode: false,
        context_profile: ContextProfile::default(),
    }
}
