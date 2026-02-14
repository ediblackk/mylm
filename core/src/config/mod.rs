//! Configuration management
//!
//! Unified configuration system for MyLM.
//! All configuration types are exported from this module.

pub mod agent;
pub mod bridge;
pub mod llm;
pub mod prompt_schema;
pub mod store;
pub mod types;
pub mod manager;

// Re-export bridge functions
pub use bridge::{
    config_to_llm_config,
    config_to_kernel_config,
    config_to_runtime_config,
    default_llm_config,
    worker_llm_config,
    BridgeError,
};

// Re-export manager types
pub use manager::{ConfigManager, CostPerToken, RateLimitError};
pub use manager::Config as ManagerConfig;
pub use manager::ConfigError as ManagerConfigError;

// Re-export types
pub use types::{Provider, SearchProvider, ConfigError, AgentPermissions, WorkerShellConfig, EscalationMode, AgentVersion};

// Re-export agent config
pub use agent::AgentConfig;

// Re-export new store (primary config interface)
pub use store::{
    Config, ProfileConfig, ProviderConfig, ProviderType,
    AppConfig, FeatureConfig, PaCoReConfig, Theme,
};

// Re-export LLM config (legacy ConfigV2)
pub use llm::{ConfigV2, AgentConfig as LlmAgentConfig, EndpointConfig, Profile, ResolvedConfig, WebSearchConfig};
pub use llm::{EndpointOverride, FeaturesConfig, MemoryConfig, PacoreConfig, PromptsConfig, AgentOverride};

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

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

/// Build system prompt for TUI sessions (stub)
pub async fn build_system_prompt(
    _context: &crate::context::TerminalContext,
    _prompt_name: &str,
    _mode: Option<&str>,
    _prompts: Option<&PromptsConfig>,
    _tools: Option<&[&str]>,
    _agent_config: Option<&crate::config::llm::AgentConfig>,
) -> anyhow::Result<String> {
    Ok(r#"You are mylm, a helpful AI assistant for terminal tasks.

You can use tools to:
- Execute shell commands
- Read and write files
- Search code
- Check git status
- Delegate tasks to workers

Be concise and helpful."#.to_string())
}

/// Get prompts directory (stub)
pub fn get_prompts_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("mylm")
        .join("prompts")
}

/// Load prompt from file (stub)
pub fn load_prompt(name: &str) -> anyhow::Result<String> {
    let path = get_prompts_dir().join(format!("{}.md", name));
    std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Failed to load prompt {}: {}", name, e))
}
