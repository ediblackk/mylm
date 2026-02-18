//! Legacy Configuration (ConfigV2)
//!
//! This module contains the legacy ConfigV2 format for backward compatibility.
//! It is used only for migrating old configs to the new unified format.
//! 
//! DEPRECATED: Use `crate::config::unified::Config` instead.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::base::{Provider, SearchProvider, ConfigError};

/// Root configuration structure for mylm v2 (LEGACY)
///
/// DEPRECATED: Use `crate::config::unified::Config` instead.
/// This is kept only for migrating old configuration files.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConfigV2 {
    /// Active profile selection (default: "default")
    #[serde(default = "default_profile")]
    pub profile: String,

    /// Base endpoint configuration
    #[serde(default)]
    pub endpoint: EndpointConfig,

    /// Multiple provider configurations (provider_name -> config)
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,

    /// Currently active provider name
    #[serde(default = "default_provider_name")]
    pub active_provider: String,

    /// Profile-specific overrides
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,

    /// Feature toggles configuration
    #[serde(default)]
    pub features: FeaturesConfig,

    /// Tmux autostart setting
    #[serde(default = "default_true")]
    pub tmux_autostart: bool,
}

impl Default for ConfigV2 {
    fn default() -> Self {
        Self {
            profile: default_profile(),
            endpoint: EndpointConfig::default(),
            providers: HashMap::default(),
            active_provider: default_provider_name(),
            profiles: HashMap::default(),
            features: FeaturesConfig::default(),
            tmux_autostart: true,
        }
    }
}

impl ConfigV2 {
    /// Load configuration from file (legacy path)
    ///
    /// Searches for `mylm.toml` in the following order:
    /// 1. Current directory (`./mylm.toml`)
    /// 2. User config directory (`~/.config/mylm/mylm.toml`)
    pub fn load() -> Result<Self, ConfigError> {
        // Check current directory first
        let current_dir_path = Path::new("mylm.toml");
        if current_dir_path.exists() {
            let content = std::fs::read_to_string(current_dir_path)?;
            let config: ConfigV2 = toml::from_str(&content)?;
            return Ok(config);
        }

        // Fall back to user config directory
        let user_config_path = Self::user_config_path()?;
        if user_config_path.exists() {
            let content = std::fs::read_to_string(&user_config_path)?;
            let config: ConfigV2 = toml::from_str(&content)?;
            return Ok(config);
        }

        // Return default if no config file found
        Ok(ConfigV2::default())
    }

    /// Get the default user config path
    fn user_config_path() -> Result<PathBuf, ConfigError> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| ConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine home directory"
            )))?;
        Ok(home_dir.join(".config").join("mylm").join("mylm.toml"))
    }
}

/// Endpoint configuration (LEGACY)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EndpointConfig {
    /// LLM provider type
    #[serde(default)]
    pub provider: Provider,

    /// Model identifier to use
    #[serde(default = "default_model")]
    pub model: String,

    /// Base URL of the API endpoint (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// API key for authentication (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Request timeout in seconds
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    /// Maximum context tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<usize>,

    /// Input price per 1k tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_price: Option<f64>,

    /// Output price per 1k tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_price: Option<f64>,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            provider: Provider::default(),
            model: default_model(),
            base_url: None,
            api_key: None,
            timeout_secs: default_timeout_secs(),
            max_context_tokens: None,
            input_price: None,
            output_price: None,
        }
    }
}

/// Provider configuration for multi-provider support (LEGACY)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderConfig {
    /// Provider type
    pub provider_type: Provider,

    /// Base URL for the API
    pub base_url: String,

    /// API key (optional for local providers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Request timeout in seconds
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

/// Profile configuration (LEGACY)
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Profile {
    /// Endpoint-specific overrides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<EndpointOverride>,

    /// Agent behavior overrides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentOverride>,
}

/// Endpoint override for profiles (LEGACY)
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct EndpointOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// Agent behavior override (LEGACY)
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AgentOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iteration_rate_limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_price: Option<f64>,
}

/// Features configuration (LEGACY)
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct FeaturesConfig {
    #[serde(default)]
    pub web_search: WebSearchConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
}

/// Web search configuration (LEGACY)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WebSearchConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub provider: SearchProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_search_results")]
    pub max_results: usize,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: default_false(),
            provider: SearchProvider::default(),
            api_key: None,
            max_results: default_search_results(),
        }
    }
}

/// Memory configuration (LEGACY)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemoryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub auto_record: bool,
    #[serde(default = "default_true")]
    pub auto_context: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            auto_record: default_true(),
            auto_context: default_true(),
        }
    }
}

// Default helpers
fn default_profile() -> String {
    "default".to_string()
}

fn default_provider_name() -> String {
    "openai".to_string()
}

fn default_model() -> String {
    "default-model".to_string()
}

fn default_timeout_secs() -> u64 {
    30
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_search_results() -> usize {
    5
}
