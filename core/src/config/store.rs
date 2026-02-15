//! Configuration Store
//!
//! Independent configuration management for MyLM.
//! Handles loading/saving TOML config files.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Unified MyLM Configuration
/// 
/// This combines agent settings, LLM providers, and UI preferences
/// into a single configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Config file format version
    #[serde(default = "default_version")]
    pub version: String,

    /// Active profile name
    #[serde(default = "default_profile")]
    pub active_profile: String,

    /// Available profiles (profile_name -> ProfileConfig)
    #[serde(default)]
    pub profiles: std::collections::HashMap<String, ProfileConfig>,

    /// LLM Provider configurations (provider_name -> ProviderConfig)
    #[serde(default)]
    pub providers: std::collections::HashMap<String, ProviderConfig>,

    /// Application settings
    #[serde(default)]
    pub app: AppConfig,

    /// Feature toggles
    #[serde(default)]
    pub features: FeatureConfig,
}

impl Default for Config {
    fn default() -> Self {
        let mut config = Self {
            version: default_version(),
            active_profile: default_profile(),
            profiles: std::collections::HashMap::new(),
            providers: std::collections::HashMap::new(),
            app: AppConfig::default(),
            features: FeatureConfig::default(),
        };
        
        // Create default profile
        config.profiles.insert("default".to_string(), ProfileConfig::default());
        
        config
    }
}

impl Config {
    /// Load configuration from file
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
    
    /// Save configuration to file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        // Ensure parent directory exists
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }
    
    /// Load from default location or create default
    /// 
    /// Tries `config.toml` first (new format), then falls back to `mylm.toml` (legacy format)
    pub fn load_or_default() -> Self {
        // Try new format first: config.toml
        if let Some(path) = Self::default_path() {
            if path.exists() {
                if let Ok(config) = Self::load(&path) {
                    return config;
                }
            }
        }
        
        // Fall back to legacy format: mylm.toml
        if let Some(config_dir) = dirs::config_dir() {
            let legacy_path = config_dir.join("mylm").join("mylm.toml");
            if legacy_path.exists() {
                if let Ok(legacy_config) = crate::config::llm::ConfigV2::load() {
                    // Convert legacy config to new format
                    let config = Self::from_legacy(&legacy_config);
                    // Try to save in new format for future loads
                    let _ = config.save_default();
                    return config;
                }
            }
        }
        
        Self::default()
    }
    
    /// Convert from legacy ConfigV2 format
    fn from_legacy(legacy: &crate::config::llm::ConfigV2) -> Self {
        use crate::config::types::Provider;
        
        let mut config = Self::default();
        
        // Set active profile from legacy config
        config.active_profile = legacy.profile.clone();
        
        // Convert providers from legacy format
        for (name, provider) in &legacy.providers {
            let provider_config = ProviderConfig {
                provider_type: match provider.provider_type {
                    Provider::Openai => ProviderType::OpenAi,
                    Provider::Google => ProviderType::Google,
                    Provider::Ollama => ProviderType::Ollama,
                    Provider::Openrouter => ProviderType::OpenRouter,
                    Provider::Kimi => ProviderType::Kimi,
                    Provider::Custom => ProviderType::Custom,
                },
                base_url: provider.base_url.clone(),
                api_key: provider.api_key.clone(),
                default_model: legacy.endpoint.model.clone(),
                models: vec![legacy.endpoint.model.clone()],
                timeout_secs: provider.timeout_secs,
            };
            config.providers.insert(name.clone(), provider_config);
        }
        
        // Create default profile with legacy endpoint settings
        let profile_config = ProfileConfig {
            provider: legacy.active_provider.clone(),
            model: Some(legacy.endpoint.model.clone()),
            max_iterations: legacy.profiles.get(&legacy.profile)
                .and_then(|p| p.agent.as_ref())
                .and_then(|a| a.max_iterations)
                .unwrap_or(50),
            rate_limit_rpm: 60,
            context_window: legacy.endpoint.max_context_tokens.unwrap_or(8192),
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            input_price: legacy.endpoint.input_price,
            output_price: legacy.endpoint.output_price,
            tested_at: None,
            test_error: None,
            web_search: crate::config::WebSearchConfig::default(),
        };
        config.profiles.insert(legacy.profile.clone(), profile_config);
        
        // Convert features
        config.features.web_search = legacy.features.web_search.enabled;
        config.features.memory = legacy.features.memory.enabled;
        
        config
    }
    
    /// Save to default location
    pub fn save_default(&self) -> anyhow::Result<()> {
        if let Some(path) = Self::default_path() {
            self.save(path)?;
        }
        Ok(())
    }
    
    /// Get default config file path
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("mylm").join("config.toml"))
    }
    
    /// Get active profile (creates default if missing)
    pub fn active_profile(&self) -> &ProfileConfig {
        self.profiles.get(&self.active_profile)
            .unwrap_or_else(|| {
                // Return a static default if profile missing
                static DEFAULT: std::sync::OnceLock<ProfileConfig> = std::sync::OnceLock::new();
                DEFAULT.get_or_init(ProfileConfig::default)
            })
    }
    
    /// Get mutable active profile
    pub fn active_profile_mut(&mut self) -> &mut ProfileConfig {
        let profile_name = self.active_profile.clone();
        self.profiles.entry(profile_name)
            .or_insert_with(ProfileConfig::default)
    }
    
    /// Get provider by name
    pub fn get_provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }
    
    /// Add or update provider
    pub fn set_provider(&mut self, name: String, config: ProviderConfig) {
        self.providers.insert(name, config);
    }
    
    /// Remove provider
    pub fn remove_provider(&mut self, name: &str) -> bool {
        self.providers.remove(name).is_some()
    }
    
    /// List provider names
    pub fn provider_names(&self) -> Vec<&String> {
        self.providers.keys().collect()
    }
    
    /// Get the active provider for current profile
    pub fn active_provider(&self) -> Option<&ProviderConfig> {
        let profile = self.active_profile();
        self.providers.get(&profile.provider)
    }
    
    /// Check if configuration is initialized (has a valid provider)
    pub fn is_initialized(&self) -> bool {
        self.active_provider().is_some()
    }
    
    /// Mark a profile as tested successfully
    pub fn mark_profile_tested(&mut self, profile_name: &str) {
        if let Some(profile) = self.profiles.get_mut(profile_name) {
            profile.tested_at = Some(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs());
            profile.test_error = None;
        }
    }
    
    /// Mark a profile as needing re-test (clear tested_at)
    pub fn mark_profile_needs_test(&mut self, profile_name: &str) {
        if let Some(profile) = self.profiles.get_mut(profile_name) {
            profile.tested_at = None;
            profile.test_error = None;
        }
    }
    
    /// Mark a profile test as failed with error message
    pub fn mark_profile_test_failed(&mut self, profile_name: &str, error: String) {
        if let Some(profile) = self.profiles.get_mut(profile_name) {
            profile.tested_at = Some(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs());
            profile.test_error = Some(error);
        }
    }
    
    /// Get test status for a profile
    /// Returns: (tested: bool, has_error: bool, error_msg: Option<&str>)
    pub fn get_profile_test_status(&self, profile_name: &str) -> Option<(bool, bool, Option<&str>)> {
        self.profiles.get(profile_name).map(|p| {
            let tested = p.tested_at.is_some();
            let error = p.test_error.as_deref();
            (tested, error.is_some(), error)
        })
    }
    
    /// Get approval policy from configuration
    /// 
    /// Returns the approval policy that determines which tools require
    /// user approval before execution. Currently returns a default policy,
    /// but can be extended to read from profile configuration.
    pub fn approval_policy(&self) -> crate::agent::contract::config::ApprovalPolicy {
        // For now, return the default approval policy
        // In the future, this can be loaded from profile configuration
        crate::agent::contract::config::ApprovalPolicy::default()
    }
}

fn default_version() -> String {
    "2.0".to_string()
}

fn default_profile() -> String {
    "default".to_string()
}

/// Profile-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    /// Provider name to use
    #[serde(default = "default_provider")]
    pub provider: String,
    
    /// Model override (optional)
    pub model: Option<String>,
    
    /// Max iterations for agent
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    
    /// Rate limit (requests per minute)
    #[serde(default = "default_rate_limit")]
    pub rate_limit_rpm: u32,
    
    /// Context window size
    #[serde(default = "default_context_window")]
    pub context_window: usize,
    
    /// Temperature (0.0 - 2.0)
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    
    /// System prompt
    pub system_prompt: Option<String>,
    
    /// Condense threshold (tokens before condensing context)
    #[serde(default)]
    pub condense_threshold: Option<usize>,
    
    /// Input price per 1M tokens (for cost tracking)
    #[serde(default)]
    pub input_price: Option<f64>,
    
    /// Output price per 1M tokens (for cost tracking)
    #[serde(default)]
    pub output_price: Option<f64>,
    
    /// When this profile was last tested (Unix timestamp)
    /// None = never tested
    #[serde(default)]
    pub tested_at: Option<u64>,
    
    /// Error message from last test (None if test succeeded)
    #[serde(default)]
    pub test_error: Option<String>,
    
    /// Web search configuration for this profile
    #[serde(default)]
    pub web_search: crate::config::WebSearchConfig,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: None,
            max_iterations: default_max_iterations(),
            rate_limit_rpm: default_rate_limit(),
            context_window: default_context_window(),
            temperature: default_temperature(),
            system_prompt: None,
            condense_threshold: None,
            input_price: None,
            output_price: None,
            tested_at: None,
            test_error: None,
            web_search: crate::config::WebSearchConfig::default(),
        }
    }
}

fn default_provider() -> String {
    "openai".to_string()
}

fn default_max_iterations() -> usize {
    50
}

fn default_rate_limit() -> u32 {
    60
}

fn default_context_window() -> usize {
    10
}

fn default_temperature() -> f32 {
    0.7
}

/// Provider (LLM) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider type
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    
    /// API base URL
    pub base_url: String,
    
    /// API key (stored in keyring in production)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    
    /// Default model
    pub default_model: String,
    
    /// Available models
    #[serde(default)]
    pub models: Vec<String>,
    
    /// Timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

impl ProviderConfig {
    /// Create OpenAI provider config
    pub fn openai(api_key: String) -> Self {
        Self {
            provider_type: ProviderType::OpenAi,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: Some(api_key),
            default_model: "gpt-4o-mini".to_string(),
            models: vec![
                "gpt-4o".to_string(),
                "gpt-4o-mini".to_string(),
                "gpt-3.5-turbo".to_string(),
            ],
            timeout_secs: default_timeout(),
        }
    }
    
    /// Create Ollama provider config
    pub fn ollama() -> Self {
        Self {
            provider_type: ProviderType::Ollama,
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: None,
            default_model: "llama3.2".to_string(),
            models: vec![
                "llama3.2".to_string(),
                "llama3.1".to_string(),
                "mistral".to_string(),
            ],
            timeout_secs: default_timeout(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    OpenAi,
    Google,
    Ollama,
    OpenRouter,
    Kimi,
    Custom,
}

fn default_timeout() -> u64 {
    120
}

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Enable tmux integration
    #[serde(default = "default_true")]
    pub tmux_enabled: bool,
    
    /// Default terminal shell
    #[serde(default = "default_shell")]
    pub shell: String,
    
    /// Editor command
    #[serde(default = "default_editor")]
    pub editor: String,
    
    /// Theme
    #[serde(default)]
    pub theme: Theme,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tmux_enabled: true,
            shell: default_shell(),
            editor: default_editor(),
            theme: Theme::default(),
        }
    }
}

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

fn default_editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string())
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Default,
    Dark,
    Light,
}

/// Feature toggles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureConfig {
    /// Enable web search
    #[serde(default)]
    pub web_search: bool,
    
    /// Enable memory/vector store
    #[serde(default = "default_true")]
    pub memory: bool,
    
    /// Enable worker delegation
    #[serde(default = "default_true")]
    pub workers: bool,
    
    /// Enable telemetry
    #[serde(default)]
    pub telemetry: bool,
    
    /// Auto-approve safe commands
    #[serde(default)]
    pub auto_approve_safe: bool,
    
    /// PaCoRe (Proactive Context Recall) settings
    #[serde(default)]
    pub pacore: PaCoReConfig,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            web_search: false,
            memory: true,
            workers: true,
            telemetry: false,
            auto_approve_safe: false,
            pacore: PaCoReConfig::default(),
        }
    }
}

/// PaCoRe configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaCoReConfig {
    /// Enable PaCoRe
    #[serde(default)]
    pub enabled: bool,
    
    /// Number of proactive rounds
    #[serde(default = "default_pacore_rounds")]
    pub rounds: usize,
}

impl Default for PaCoReConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rounds: default_pacore_rounds(),
        }
    }
}

fn default_pacore_rounds() -> usize {
    3
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert_eq!(config.version, "2.0");
        assert_eq!(config.active_profile, "default");
        assert!(config.profiles.contains_key("default"));
    }
    
    #[test]
    fn test_config_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");
        
        // Create and save config
        let mut config = Config::default();
        config.set_provider("test".to_string(), ProviderConfig::ollama());
        config.save(&config_path).unwrap();
        
        // Load and verify
        let loaded = Config::load(&config_path).unwrap();
        assert!(loaded.providers.contains_key("test"));
        assert_eq!(loaded.providers["test"].provider_type, ProviderType::Ollama);
    }
    
    #[test]
    fn test_toml_format() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        
        // Should be valid TOML
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.version, config.version);
    }
}
