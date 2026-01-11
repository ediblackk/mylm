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

    /// Maximum context tokens to keep in history
    #[serde(default = "default_context_limit")]
    pub context_limit: usize,

    /// Whether to show intermediate steps (thoughts/actions)
    #[serde(default = "default_verbose_mode")]
    pub verbose_mode: bool,
}

fn default_context_limit() -> usize {
    100000
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
            context_limit: default_context_limit(),
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
    pub async fn setup() -> Result<Self> {
        let theme = ColorfulTheme::default();
        
        // Check if config already exists
        if let Some(config_path) = find_config_file() {
            if config_path.exists() {
                let reconfigure = Confirm::with_theme(&theme)
                    .with_prompt(format!("Configuration already exists at {:?}. Reconfigure?", config_path))
                    .default(false)
                    .interact()?;
                
                if !reconfigure {
                    println!("✅ Keeping existing configuration.");
                    return Self::load_from_file(config_path);
                }
            }
        }

        println!("🤖 Welcome to mylm setup wizard!");

        // 1. Choose Provider
        let providers = vec!["OpenAI", "Google (Gemini)", "Ollama", "OpenRouter", "Custom"];
        let selection = Select::with_theme(&theme)
            .with_prompt("Select your LLM provider")
            .items(&providers)
            .default(0)
            .interact()?;

        let provider_name = providers[selection];
        
        // Default values based on provider
        let (mut provider_id, mut base_url, mut model) = match provider_name {
            "OpenAI" => ("openai".to_string(), "https://api.openai.com/v1".to_string(), "gpt-4o".to_string()),
            "Google (Gemini)" => ("google".to_string(), "https://generativelanguage.googleapis.com".to_string(), "gemini-3-flash-preview".to_string()),
            "Ollama" => ("openai".to_string(), "http://localhost:11434/v1".to_string(), "llama3.2".to_string()),
            "OpenRouter" => ("openai".to_string(), "https://openrouter.ai/api/v1".to_string(), "google/gemini-2.0-flash-001".to_string()),
            _ => ("openai".to_string(), String::new(), String::new()),
        };

        // 2. API Key (needed before fetching models)
        let api_key = if provider_name != "Ollama" {
            Password::with_theme(&theme)
                .with_prompt(format!("API Key for {}", provider_name))
                .interact()?
        } else {
            "none".to_string()
        };

        // 3. Fetch models if possible
        let mut fetched_models = Vec::new();
        if provider_name != "Custom" {
            let fetch_options = vec!["Yes", "No (use default or enter manually)"];
            let fetch = Select::with_theme(&theme)
                .with_prompt("Fetch latest models from provider?")
                .items(&fetch_options)
                .default(0)
                .interact()?;

            if fetch == 0 {
                let filter: String = Input::with_theme(&theme)
                    .with_prompt("Filter models by keyword (optional)")
                    .allow_empty(true)
                    .interact_text()?;

                println!("📡 Fetching models...");
                match fetch_models_from_provider(provider_name, &base_url, &api_key, &filter).await {
                    Ok(models) => {
                        if !models.is_empty() {
                            let model_selection = Select::with_theme(&theme)
                                .with_prompt(format!("Select model (showing {} matching)", models.len()))
                                .items(&models)
                                .default(0)
                                .interact()?;
                            model = models[model_selection].clone();
                            fetched_models = models;
                        } else {
                            println!("⚠️ No models found. Using default.");
                        }
                    }
                    Err(e) => {
                        println!("❌ Failed to fetch models: {}. Using default.", e);
                    }
                }
            }
        }

        // 4. Custom inputs if needed or if user wants to override
        if provider_name == "Custom" {
            provider_id = "openai".to_string(); // Default custom to openai-compatible
            base_url = Input::<String>::with_theme(&theme)
                .with_prompt("Base URL")
                .interact_text()?;
            model = Input::<String>::with_theme(&theme)
                .with_prompt("Model name")
                .interact_text()?;
        } else if fetched_models.is_empty() {
            // Allow overriding defaults if we didn't fetch
            model = Input::<String>::with_theme(&theme)
                .with_prompt("Model name")
                .with_initial_text(model)
                .interact_text()?;

            if provider_name != "Ollama" && provider_name != "Google (Gemini)" {
                 base_url = Input::<String>::with_theme(&theme)
                    .with_prompt("Base URL")
                    .with_initial_text(base_url)
                    .interact_text()?;
            }
        }

        // 4. Create endpoint
        let endpoint = endpoints::EndpointConfig {
            name: "default".to_string(),
            provider: provider_id.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            api_key,
            timeout_seconds: 60,
            input_price_per_1k: 0.0,
            output_price_per_1k: 0.0,
            max_context_tokens: 32768,
            condense_threshold: 0.8,
        };

        // 5. Web Search Setup
        let enable_search = Confirm::with_theme(&theme)
            .with_prompt("Enable web search capabilities?")
            .default(true)
            .interact()?;

        let mut web_search = WebSearchConfig::default();
        if enable_search {
            web_search.enabled = true;
            let search_providers = vec!["Kimi (Moonshot AI)", "SerpAPI (Google/Bing/etc.)"];
            let search_selection = Select::with_theme(&theme)
                .with_prompt("Select web search provider")
                .items(&search_providers)
                .default(0)
                .interact()?;

            match search_selection {
                0 => {
                    web_search.provider = "kimi".to_string();
                    web_search.api_key = Password::with_theme(&theme)
                        .with_prompt("Kimi API Key")
                        .interact()?;
                    web_search.model = "kimi-k2-turbo-preview".to_string();
                }
                1 => {
                    web_search.provider = "serpapi".to_string();
                    web_search.api_key = Password::with_theme(&theme)
                        .with_prompt("SerpAPI Key")
                        .interact()?;
                }
                _ => {}
            }
        }

        // 6. Build final config
        let config = Config {
            active_profile: "default".to_string(),
            profiles: vec![Profile {
                name: "default".to_string(),
                endpoint: "default".to_string(),
                prompt: "default".to_string(),
            }],
            default_endpoint: "default".to_string(),
            endpoints: vec![endpoint],
            commands: CommandConfig {
                allow_execution: false,
                allowlist_paths: vec![],
            },
            web_search,
            context_limit: default_context_limit(),
            verbose_mode: default_verbose_mode(),
        };

        // 6. Save config
        let config_dir = get_config_dir()
            .or_else(|| {
                // Create if not exists
                home_dir().map(|h| h.join(".config").join(CONFIG_DIR_NAME))
            })
            .context("Could not determine config directory")?;

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)?;
        }

        let config_path = config_dir.join(CONFIG_FILE_NAME);
        config.save(&config_path)?;

        println!("\n✅ Configuration saved to {:?}", config_path);
        println!("Try it out with: ai 'hello world'");

        Ok(config)
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
        context_limit: 100000,
        verbose_mode: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.default_endpoint, "default");
        assert!(config.endpoints.is_empty());
    }

    #[test]
    fn test_load_from_file() {
        let yaml_content = r#"
default_endpoint: ollama
endpoints:
  - name: ollama
    provider: openai
    base_url: http://localhost:11434/v1
    model: llama3.2
    api_key: none
    timeout_seconds: 120
"#;
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("mylm.yaml");
        fs::write(&config_path, yaml_content).unwrap();

        let config = Config::load_from_file(&config_path).unwrap();
        assert_eq!(config.default_endpoint, "ollama");
        assert_eq!(config.endpoints.len(), 1);
        assert_eq!(config.endpoints[0].name, "ollama");
    }

    #[test]
    fn test_get_endpoint() {
        let config = create_default_config();
        let endpoint = config.get_endpoint(Some("ollama")).unwrap();
        assert_eq!(endpoint.name, "ollama");
    }

    #[test]
    fn test_get_default_endpoint() {
        let config = create_default_config();
        let endpoint = config.get_default_endpoint().unwrap();
        assert_eq!(endpoint.name, "ollama");
    }
}
