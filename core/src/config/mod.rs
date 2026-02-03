//! Configuration management - ConfigV2
//!
//! TOML-based configuration with profile inheritance and environment overrides.

pub mod v2;
pub mod manager;

// Re-export manager types with aliases to avoid conflicts
pub use manager::{ConfigManager, CostPerToken, RateLimitError};
pub use manager::Config as ManagerConfig;
pub use manager::ConfigError as ManagerConfigError;

// Re-export v2 types
pub use v2::{
    AgentConfig, AgentOverride, ConfigError, ConfigV2, EndpointConfig,
    EndpointOverride, FeaturesConfig, MemoryConfig, PacoreConfig, Profile,
    PromptsConfig, Provider, ResolvedConfig, SearchProvider, WebSearchConfig,
    build_system_prompt, build_system_prompt_with_capabilities, generate_capabilities_prompt,
    get_identity_prompt, get_memory_protocol, get_prompts_dir, get_react_protocol,
    get_user_prompts_dir, install_default_prompts, load_prompt, load_prompt_from_path,
};

// Keep backward-compatible Config alias
pub use v2::ConfigV2 as Config;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Agent version/loop implementation
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentVersion {
    /// Classic ReAct loop (V1)
    V1,
    /// Multi-layered cognitive architecture (V2)
    #[default]
    V2,
}

impl std::fmt::Display for AgentVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentVersion::V1 => write!(f, "V1 (Classic ReAct)"),
            AgentVersion::V2 => write!(f, "V2 (Cognitive Submodule)"),
        }
    }
}

/// Context budgeting profile
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContextProfile {
    Minimal,
    #[default]
    Balanced,
    Verbose,
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

/// Command execution configuration
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CommandConfig {
    #[serde(default)]
    pub allow_execution: bool,
    #[serde(default)]
    pub allowlist_paths: Vec<PathBuf>,
    #[serde(default)]
    pub allowed_commands: Vec<String>,
    #[serde(default)]
    pub blocked_commands: Vec<String>,
}

/// Legacy WebSearchConfig for backward compatibility (for existing user configs)
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LegacyWebSearchConfig {
    pub enabled: bool,
    pub provider: String,
    pub api_key: String,
    pub model: String,
}

impl From<WebSearchConfig> for LegacyWebSearchConfig {
    fn from(config: WebSearchConfig) -> Self {
        Self {
            enabled: config.enabled,
            provider: match config.provider {
                SearchProvider::Kimi => "kimi".to_string(),
                SearchProvider::Serpapi => "serpapi".to_string(),
                SearchProvider::Brave => "brave".to_string(),
            },
            api_key: config.api_key.unwrap_or_default(),
            model: "kimi-k2-turbo-preview".to_string(),
        }
    }
}

impl From<LegacyWebSearchConfig> for WebSearchConfig {
    fn from(config: LegacyWebSearchConfig) -> Self {
        let provider = match config.provider.as_str() {
            "kimi" => SearchProvider::Kimi,
            "brave" => SearchProvider::Brave,
            _ => SearchProvider::Serpapi,
        };
        Self {
            enabled: config.enabled,
            provider,
            api_key: if config.api_key.is_empty() { None } else { Some(config.api_key) },
            max_results: 5,
        }
    }
}

/// Legacy MemoryConfig adapter for compatibility
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LegacyMemoryConfig {
    pub auto_record: bool,
    pub auto_context: bool,
    pub auto_categorize: bool,
}

impl Default for LegacyMemoryConfig {
    fn default() -> Self {
        Self {
            auto_record: true,
            auto_context: true,
            auto_categorize: true,
        }
    }
}

impl From<MemoryConfig> for LegacyMemoryConfig {
    fn from(config: MemoryConfig) -> Self {
        Self {
            auto_record: config.auto_record,
            auto_context: config.auto_context,
            auto_categorize: true,
        }
    }
}

impl From<LegacyMemoryConfig> for MemoryConfig {
    fn from(config: LegacyMemoryConfig) -> Self {
        Self {
            enabled: true,
            auto_record: config.auto_record,
            auto_context: config.auto_context,
            max_context_memories: 10,
        }
    }
}

/// UI-facing profile info (for display purposes)
#[derive(Debug, Clone)]
pub struct ProfileInfo {
    pub name: String,
    pub model_override: Option<String>,
    pub max_iterations: Option<usize>,
    pub iteration_rate_limit: Option<u64>,
}

/// UI-facing endpoint info (for display purposes)
#[derive(Debug, Clone)]
pub struct EndpointInfo {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key_set: bool,
    pub timeout_seconds: u64,
}

/// Find the configuration file in standard locations
pub fn find_config_file() -> Option<PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        let path = cwd.join("mylm.toml");
        if path.exists() {
            return Some(path);
        }
    }

    if let Some(dir) = get_config_dir() {
        let path = dir.join("mylm.toml");
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Get the configuration directory path
pub fn get_config_dir() -> Option<PathBuf> {
    use dirs::config_dir;
    use home::home_dir;
    
    if let Some(dir) = config_dir() {
        return Some(dir.join("mylm"));
    }

    if let Some(home) = home_dir() {
        return Some(home.join(".config").join("mylm"));
    }

    None
}

/// Create default configuration
pub fn create_default_config() -> ConfigV2 {
    ConfigV2::default()
}

/// Extension trait for UI-friendly methods
pub trait ConfigUiExt {
    /// Get list of profile names
    fn profile_names(&self) -> Vec<String>;
    
    /// Get profile info for UI display
    fn get_profile_info(&self, name: &str) -> Option<ProfileInfo>;
    
    /// Get the active profile info
    fn get_active_profile_info(&self) -> Option<ProfileInfo>;
    
    /// Set active profile
    fn set_active_profile(&mut self, name: &str) -> anyhow::Result<()>;
    
    /// Create a new profile
    fn create_profile(&mut self, name: &str) -> anyhow::Result<()>;
    
    /// Delete a profile
    fn delete_profile(&mut self, name: &str) -> anyhow::Result<()>;
    
    /// Get base endpoint info
    fn get_endpoint_info(&self) -> EndpointInfo;
    
    /// Get effective endpoint info (with profile overrides applied)
    fn get_effective_endpoint_info(&self) -> EndpointInfo;
    
    /// Update base endpoint
    fn update_endpoint(&mut self, provider: Provider, model: String, base_url: Option<String>, api_key: Option<String>) -> anyhow::Result<()>;
    
    /// Set profile model override
    fn set_profile_model_override(&mut self, profile_name: &str, model: Option<String>) -> anyhow::Result<()>;
    
    /// Set profile max_iterations override
    fn set_profile_max_iterations(&mut self, profile_name: &str, iterations: Option<usize>) -> anyhow::Result<()>;
    
    /// Set profile iteration_rate_limit override
    fn set_profile_iteration_rate_limit(&mut self, profile_name: &str, rate_limit: Option<u64>) -> anyhow::Result<()>;
    
    /// Check if configuration is initialized (has valid endpoint)
    fn is_initialized(&self) -> bool;
    
    /// Save to default location
    fn save_to_default_location(&self) -> anyhow::Result<()>;
}

impl ConfigUiExt for ConfigV2 {
    fn profile_names(&self) -> Vec<String> {
        self.profiles.keys().cloned().collect()
    }
    
    fn get_profile_info(&self, name: &str) -> Option<ProfileInfo> {
        self.profiles.get(name).map(|p| ProfileInfo {
            name: name.to_string(),
            model_override: p.endpoint.as_ref().and_then(|e| e.model.clone()),
            max_iterations: p.agent.as_ref().and_then(|a| a.max_iterations),
            iteration_rate_limit: p.agent.as_ref().and_then(|a| a.iteration_rate_limit),
        })
    }
    
    fn get_active_profile_info(&self) -> Option<ProfileInfo> {
        self.get_profile_info(&self.profile)
    }
    
    fn set_active_profile(&mut self, name: &str) -> anyhow::Result<()> {
        if !self.profiles.contains_key(name) {
            anyhow::bail!("Profile '{}' does not exist", name);
        }
        self.profile = name.to_string();
        Ok(())
    }
    
    fn create_profile(&mut self, name: &str) -> anyhow::Result<()> {
        if self.profiles.contains_key(name) {
            anyhow::bail!("Profile '{}' already exists", name);
        }
        self.profiles.insert(name.to_string(), Profile::default());
        Ok(())
    }
    
    fn delete_profile(&mut self, name: &str) -> anyhow::Result<()> {
        if name == self.profile {
            anyhow::bail!("Cannot delete the active profile");
        }
        if self.profiles.remove(name).is_none() {
            anyhow::bail!("Profile '{}' does not exist", name);
        }
        Ok(())
    }
    
    fn get_endpoint_info(&self) -> EndpointInfo {
        EndpointInfo {
            provider: format!("{:?}", self.endpoint.provider).to_lowercase(),
            base_url: self.endpoint.base_url.clone().unwrap_or_else(|| self.endpoint.provider.default_url()),
            model: self.endpoint.model.clone(),
            api_key_set: self.endpoint.api_key.is_some() && !self.endpoint.api_key.as_ref().unwrap().is_empty(),
            timeout_seconds: self.endpoint.timeout_secs,
        }
    }
    
    fn get_effective_endpoint_info(&self) -> EndpointInfo {
        let resolved = self.resolve_profile();
        EndpointInfo {
            provider: format!("{:?}", resolved.provider).to_lowercase(),
            base_url: resolved.base_url.unwrap_or_else(|| resolved.provider.default_url()),
            model: resolved.model,
            api_key_set: resolved.api_key.is_some() && !resolved.api_key.as_ref().unwrap().is_empty(),
            timeout_seconds: resolved.timeout_secs,
        }
    }
    
    fn update_endpoint(&mut self, provider: Provider, model: String, base_url: Option<String>, api_key: Option<String>) -> anyhow::Result<()> {
        self.endpoint.provider = provider;
        self.endpoint.model = model;
        self.endpoint.base_url = base_url;
        self.endpoint.api_key = api_key;
        Ok(())
    }
    
    fn set_profile_model_override(&mut self, profile_name: &str, model: Option<String>) -> anyhow::Result<()> {
        let profile = self.profiles.get_mut(profile_name)
            .ok_or_else(|| anyhow::anyhow!("Profile '{}' does not exist", profile_name))?;
        
        if let Some(ref m) = model {
            profile.endpoint = Some(EndpointOverride {
                model: Some(m.clone()),
                api_key: profile.endpoint.as_ref().and_then(|e| e.api_key.clone()),
            });
        } else {
            // Remove model override but keep api_key if present
            let api_key = profile.endpoint.as_ref().and_then(|e| e.api_key.clone());
            if api_key.is_some() {
                profile.endpoint = Some(EndpointOverride { model: None, api_key });
            } else {
                profile.endpoint = None;
            }
        }
        Ok(())
    }
    
    fn set_profile_max_iterations(&mut self, profile_name: &str, iterations: Option<usize>) -> anyhow::Result<()> {
        let profile = self.profiles.get_mut(profile_name)
            .ok_or_else(|| anyhow::anyhow!("Profile '{}' does not exist", profile_name))?;
        
        if let Some(iters) = iterations {
            profile.agent = Some(AgentOverride {
                max_iterations: Some(iters),
                iteration_rate_limit: profile.agent.as_ref().and_then(|a| a.iteration_rate_limit),
                main_model: profile.agent.as_ref().and_then(|a| a.main_model.clone()),
                worker_model: profile.agent.as_ref().and_then(|a| a.worker_model.clone()),
            });
        } else {
            // Remove max_iterations override but keep other agent settings
            let iteration_rate_limit = profile.agent.as_ref().and_then(|a| a.iteration_rate_limit);
            let main_model = profile.agent.as_ref().and_then(|a| a.main_model.clone());
            let worker_model = profile.agent.as_ref().and_then(|a| a.worker_model.clone());
            if iteration_rate_limit.is_some() || main_model.is_some() || worker_model.is_some() {
                profile.agent = Some(AgentOverride {
                    max_iterations: None,
                    iteration_rate_limit,
                    main_model,
                    worker_model,
                });
            } else {
                profile.agent = None;
            }
        }
        Ok(())
    }
    
    fn set_profile_iteration_rate_limit(&mut self, profile_name: &str, rate_limit: Option<u64>) -> anyhow::Result<()> {
        let profile = self.profiles.get_mut(profile_name)
            .ok_or_else(|| anyhow::anyhow!("Profile '{}' does not exist", profile_name))?;
        
        if let Some(ms) = rate_limit {
            profile.agent = Some(AgentOverride {
                max_iterations: profile.agent.as_ref().and_then(|a| a.max_iterations),
                iteration_rate_limit: Some(ms),
                main_model: profile.agent.as_ref().and_then(|a| a.main_model.clone()),
                worker_model: profile.agent.as_ref().and_then(|a| a.worker_model.clone()),
            });
        } else {
            // Remove rate limit override but keep other agent settings
            let max_iterations = profile.agent.as_ref().and_then(|a| a.max_iterations);
            let main_model = profile.agent.as_ref().and_then(|a| a.main_model.clone());
            let worker_model = profile.agent.as_ref().and_then(|a| a.worker_model.clone());
            if max_iterations.is_some() || main_model.is_some() || worker_model.is_some() {
                profile.agent = Some(AgentOverride {
                    max_iterations,
                    iteration_rate_limit: None,
                    main_model,
                    worker_model,
                });
            } else {
                profile.agent = None;
            }
        }
        Ok(())
    }
    
    fn is_initialized(&self) -> bool {
        // Consider initialized if we have a valid provider and model
        !self.endpoint.model.is_empty()
    }
    
    fn save_to_default_location(&self) -> anyhow::Result<()> {
        self.save(None)?;
        Ok(())
    }
}

impl std::str::FromStr for Provider {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Provider::Openai),
            "google" => Ok(Provider::Google),
            "ollama" => Ok(Provider::Ollama),
            "openrouter" => Ok(Provider::Openrouter),
            "kimi" => Ok(Provider::Kimi),
            "custom" => Ok(Provider::Custom),
            _ => Err(()),
        }
    }
}

impl Provider {
    /// Get default URL for this provider
    pub fn default_url(&self) -> String {
        match self {
            Provider::Openai => "https://api.openai.com/v1".to_string(),
            Provider::Google => "https://generativelanguage.googleapis.com".to_string(),
            Provider::Ollama => "http://localhost:11434/v1".to_string(),
            Provider::Openrouter => "https://openrouter.ai/api/v1".to_string(),
            Provider::Kimi => "https://api.moonshot.cn/v1".to_string(),
            Provider::Custom => "".to_string(),
        }
    }
}
