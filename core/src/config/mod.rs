//! Configuration management
//!
//! Unified configuration system for MyLM.
//! 
//! # Module Structure
//! 
//! - `base` - Core types: Provider, SearchProvider, ConfigError
//! - `unified` - Main Config with profiles, providers, app settings
//! - `app` - AppConfig, FeatureConfig, Theme, PaCoReConfig
//! - `profile` - ProfileConfig, ResolvedProfile, WebSearchConfig
//! - `provider` - ProviderConfig, ProviderType
//! - `manager` - ConfigManager with hot-reload and rate limiting
//! - `bridge` - Bridge functions to convert Config to LLM/Agent configs
//! - `prompt` - Prompt schema definitions
//! - `agent` - Agent-specific configuration
//! - `legacy` - DEPRECATED: ConfigV2 for backward compatibility

// Core types
pub mod base;

// Main configuration
pub mod unified;
pub mod app;
pub mod profile;
pub mod provider;

// Management and utilities
pub mod manager;
pub mod bridge;
pub mod prompt;
pub mod prompt_schema;
pub mod agent;

// Legacy (for migration only)
pub mod legacy;

// Re-exports from base
pub use base::{
    Provider, SearchProvider, ConfigError,
    AgentPermissions, WorkerShellConfig, EscalationMode, AgentVersion,
    ContextProfile,
};

// Re-exports from unified (main config)
pub use unified::{
    Config,
    ProfileConfig, ResolvedProfile, WebSearchConfig,
    ProviderConfig, ProviderType,
    AppConfig, FeatureConfig, MemorySettings, PaCoReConfig, Theme,
};

// Re-exports from manager
pub use manager::{ConfigManager, CostPerToken, RateLimitError};

// Re-exports from bridge
pub use bridge::{
    config_to_llm_config,
    config_to_kernel_config,
    config_to_runtime_config,
    default_llm_config,
    worker_llm_config,
    BridgeError,
};

// Re-exports from agent
pub use agent::{AgentConfig, MemoryConfig, UserProfile, SessionSummary};

// Re-exports from prompt_schema (prompt types)
pub use prompt_schema::{PromptConfig, IdentitySection, Section, Protocols, JsonKeys, ReactProtocol};

use std::path::PathBuf;

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
pub fn create_default_config() -> Config {
    Config::default()
}

/// Get prompts directory
pub fn get_prompts_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("mylm")
        .join("prompts")
}

/// Load prompt from file
pub fn load_prompt(name: &str) -> anyhow::Result<String> {
    let path = get_prompts_dir().join(format!("{}.md", name));
    std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Failed to load prompt {}: {}", name, e))
}

/// Build system prompt for TUI sessions (stub)
pub async fn build_system_prompt(
    _context: &crate::environment::EnvironmentContext,
    _prompt_name: &str,
    _mode: Option<&str>,
    _prompts: Option<&crate::config::prompt_schema::PromptConfig>,
    _tools: Option<&[&str]>,
    _agent_config: Option<&crate::config::agent::AgentConfig>,
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
