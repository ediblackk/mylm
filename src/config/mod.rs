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

/// Agent configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentConfig {
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
            }],
            default_endpoint: default_endpoint(),
            endpoints: Vec::new(),
            commands: CommandConfig::default(),
            web_search: WebSearchConfig::default(),
            memory: MemoryConfig::default(),
            agent: AgentConfig::default(),
            context_limit: None,
            verbose_mode: default_verbose_mode(),
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
    /// Edit the LLM endpoint configuration
    pub async fn edit_endpoint_details(&mut self, endpoint_name: &str) -> Result<()> {
        let theme = ColorfulTheme::default();
        let providers = vec!["OpenAI", "Google (Gemini)", "Ollama", "OpenRouter", "Custom"];
        let selection = Select::with_theme(&theme)
            .with_prompt("Select your LLM provider")
            .items(&providers)
            .default(0)
            .interact()?;

        let provider_name = providers[selection];
        let (provider_id, mut base_url, mut model) = match provider_name {
            "OpenAI" => ("openai".to_string(), "https://api.openai.com/v1".to_string(), "gpt-4o".to_string()),
            "Google (Gemini)" => ("google".to_string(), "https://generativelanguage.googleapis.com".to_string(), "gemini-3-flash-preview".to_string()),
            "Ollama" => ("openai".to_string(), "http://localhost:11434/v1".to_string(), "llama3.2".to_string()),
            "OpenRouter" => ("openai".to_string(), "https://openrouter.ai/api/v1".to_string(), "google/gemini-2.0-flash-001".to_string()),
            _ => ("openai".to_string(), String::new(), String::new()),
        };

        let api_key = if provider_name != "Ollama" {
            Password::with_theme(&theme)
                .with_prompt(format!("API Key for {}", provider_name))
                .interact()?
        } else {
            "none".to_string()
        };

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
        let mut input_price_per_1k = 0.0;
        let mut output_price_per_1k = 0.0;
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

            input_price_per_1k = Input::with_theme(&theme)
                .with_prompt("Input price per 1k tokens ($)")
                .default(input_price_per_1k)
                .interact_text()?;

            output_price_per_1k = Input::with_theme(&theme)
                .with_prompt("Output price per 1k tokens ($)")
                .default(output_price_per_1k)
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
            input_price_per_1k,
            output_price_per_1k,
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
                format!("Max Iterations: {} (steps per request)", self.agent.max_iterations),
                format!("Max Driver Loops: {} (session safety limit)", self.agent.max_driver_loops),
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
                    self.agent.max_iterations = Input::with_theme(&theme)
                        .with_prompt("Max steps (thoughts/actions) per single user request")
                        .default(self.agent.max_iterations)
                        .interact_text()?;
                }
                4 => {
                    self.agent.max_driver_loops = Input::with_theme(&theme)
                        .with_prompt("Safety limit: max total exchanges in one session")
                        .default(self.agent.max_driver_loops)
                        .interact_text()?;
                }
                5 => {
                    self.memory.auto_record = !self.memory.auto_record;
                    self.memory.auto_context = self.memory.auto_record;
                    self.memory.auto_categorize = self.memory.auto_record;
                }
                6 => {
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
            },
            Profile {
                name: "openai".to_string(),
                endpoint: "openai".to_string(),
                prompt: "default".to_string(),
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
                input_price_per_1k: 0.0,
                output_price_per_1k: 0.0,
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
                input_price_per_1k: 0.0025,
                output_price_per_1k: 0.01,
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
                input_price_per_1k: 0.0,
                output_price_per_1k: 0.0,
                max_context_tokens: 32768,
                condense_threshold: 0.8,
            },
        ],
        commands: CommandConfig {
            allow_execution: false,
            allowlist_paths: vec![],
        },
        web_search: WebSearchConfig::default(),
        memory: MemoryConfig::default(),
        agent: AgentConfig::default(),
        context_limit: None,
        verbose_mode: false,
    }
}
