//! TOML-based Configuration v2 Data Structures
//!
//! This module defines the new configuration format for mylm, designed to be
//! simpler, more versatile, and following modern configuration best practices.
//!
//! # Design Principles
//!
//! 1. **Flat over nested** - Maximum 2 levels deep
//! 2. **Convention over configuration** - Sensible defaults eliminate fields
//! 3. **Environment-first secrets** - API keys from env vars by default
//! 4. **Built-in presets** - Fast/thorough/dev/prod out of the box
//! 5. **Profile inheritance** - Base + override pattern
//!
//! # Example Configuration
//!
//! ```toml
//! profile = "fast"
//!
//! [endpoint]
//! provider = "openai"
//! model = "gpt-4o"
//!
//! [profiles.fast]
//! endpoint = { model = "gpt-4o-mini" }
//! agent = { max_iterations = 5 }
//!
//! [profiles.thorough]
//! endpoint = { model = "gpt-4o" }
//! agent = { max_iterations = 20 }
//!
//! [features.web_search]
//! enabled = false
//!
//! [features.memory]
//! auto_record = true
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Root configuration structure for mylm v2
///
/// This is the top-level configuration that combines the base endpoint
/// configuration with profile-specific overrides and feature toggles.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConfigV2 {
    /// Active profile selection (default: "fast")
    ///
    /// Can be overridden with MYLM_PROFILE environment variable.
    /// Built-in profiles: "fast", "thorough", "dev", "prod"
    #[serde(default = "default_profile")]
    pub profile: String,

    /// Base endpoint configuration
    ///
    /// This is the default endpoint configuration that all profiles inherit from.
    /// Profiles can override specific fields using `EndpointOverride`.
    #[serde(default)]
    pub endpoint: EndpointConfig,

    /// Profile-specific overrides
    ///
    /// Each profile can override endpoint settings and agent behavior.
    /// The active profile's overrides are merged with the base endpoint.
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,

    /// Feature toggles configuration
    ///
    /// Controls optional features like web search and memory.
    #[serde(default)]
    pub features: FeaturesConfig,
}

impl Default for ConfigV2 {
    fn default() -> Self {
        Self {
            profile: default_profile(),
            endpoint: EndpointConfig::default(),
            profiles: HashMap::default(),
            features: FeaturesConfig::default(),
        }
    }
}

/// Default active profile name
fn default_profile() -> String {
    "fast".to_string()
}

/// Endpoint configuration
///
/// Simplified endpoint configuration compared to v1, focusing on
/// essential connection details while removing price/token tracking.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EndpointConfig {
    /// LLM provider type
    #[serde(default)]
    pub provider: Provider,

    /// Model identifier to use
    ///
    /// Examples: "gpt-4o", "gpt-4o-mini", "claude-3-5-sonnet-20241022"
    #[serde(default = "default_model")]
    pub model: String,

    /// Base URL of the API endpoint (optional)
    ///
    /// If not specified, uses the provider's default URL.
    /// For custom providers or self-hosted models, specify the full URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// API key for authentication (optional)
    ///
    /// If not specified, the application will attempt to read from
    /// environment variables like MYLM_API_KEY or provider-specific keys.
    /// For local models (Ollama), this can be omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Request timeout in seconds
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            provider: Provider::default(),
            model: default_model(),
            base_url: None,
            api_key: None,
            timeout_secs: default_timeout_secs(),
        }
    }
}

/// Default model name
fn default_model() -> String {
    "gpt-4o".to_string()
}

/// Default timeout in seconds
fn default_timeout_secs() -> u64 {
    30
}

/// LLM Provider types
///
/// Supported LLM providers with their specific characteristics.
/// Each provider has a default base URL and authentication method.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// OpenAI API (GPT models)
    ///
    /// Default URL: https://api.openai.com/v1
    /// Env var: OPENAI_API_KEY or MYLM_API_KEY
    #[default]
    Openai,

    /// Google Gemini API
    ///
    /// Default URL: https://generativelanguage.googleapis.com
    /// Env var: GOOGLE_API_KEY or MYLM_API_KEY
    Google,

    /// Ollama (local models)
    ///
    /// Default URL: http://localhost:11434/v1
    /// No API key required
    Ollama,

    /// OpenRouter (unified API for multiple providers)
    ///
    /// Default URL: https://openrouter.ai/api/v1
    /// Env var: OPENROUTER_API_KEY or MYLM_API_KEY
    Openrouter,

    /// Kimi (Moonshot AI)
    ///
    /// Default URL: https://api.moonshot.cn/v1
    /// Env var: KIMI_API_KEY or MYLM_API_KEY
    Kimi,

    /// Custom provider (user-specified URL)
    ///
    /// Requires explicit base_url configuration.
    /// Uses MYLM_API_KEY for authentication.
    Custom,
}

/// Profile configuration
///
/// Profiles provide a way to quickly switch between different
/// agent behaviors and model configurations without rewriting
/// the entire endpoint configuration.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Profile {
    /// Endpoint-specific overrides
    ///
    /// Only fields that differ from the base endpoint need to be specified.
    /// For example, to use a cheaper model for the "fast" profile,
    /// just set `endpoint = { model = "gpt-4o-mini" }`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<EndpointOverride>,

    /// Agent behavior overrides
    ///
    /// Controls how the agent behaves for this profile:
    /// - max_iterations: How many steps per request
    /// - main_model: Which model for complex reasoning
    /// - worker_model: Which model for simple tasks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentOverride>,
}

/// Endpoint override for profiles
///
/// Partial endpoint configuration that can override specific
/// fields from the base endpoint configuration.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct EndpointOverride {
    /// Override the model identifier
    ///
    /// Example: Use "gpt-4o-mini" for fast/cheap responses
    /// while keeping the same provider and API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Override the API key
    ///
    /// Useful for profiles that need different API keys,
    /// such as separate work and personal accounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// Agent behavior override
///
/// Controls the agent's behavior for a specific profile.
/// These settings affect how the agent processes requests.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AgentOverride {
    /// Maximum number of iterations (steps) per request
    ///
    /// - Lower values (3-5): Faster, simpler responses
    /// - Higher values (15-30): More thorough analysis
    /// - Default from base config: 10
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<usize>,

    /// Model for orchestrator/agent main reasoning
    ///
    /// Used for complex tasks requiring high capability.
    /// If not set, uses the endpoint's model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_model: Option<String>,

    /// Model for worker/sub-tasks
    ///
    /// Used for simple, repetitive tasks where cost matters.
    /// If not set, uses the main_model or endpoint's model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_model: Option<String>,
}

/// Features configuration
///
/// Controls optional features and integrations.
/// Each feature has its own configuration section.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct FeaturesConfig {
    /// Web search integration settings
    #[serde(default)]
    pub web_search: WebSearchConfig,

    /// Memory and RAG settings
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Prompt template configuration
    #[serde(default)]
    pub prompts: PromptsConfig,
}

/// Web search configuration
///
/// Enables the agent to search the web for current information.
/// Supports multiple search providers.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WebSearchConfig {
    /// Whether web search is enabled
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// Search provider to use
    #[serde(default)]
    pub provider: SearchProvider,

    /// API key for the search provider (optional)
    ///
    /// If not specified, uses environment variables:
    /// - Kimi: KIMI_API_KEY or MYLM_WEB_SEARCH_API_KEY
    /// - SerpApi: SERPAPI_KEY or MYLM_WEB_SEARCH_API_KEY
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Maximum number of results to include in context
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

/// Default number of search results
fn default_search_results() -> usize {
    5
}

/// Search provider types
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchProvider {
    /// Kimi (Moonshot AI) search
    ///
    /// Uses Kimi's built-in web search capability.
    /// Requires Kimi API key.
    Kimi,

    /// SerpApi (Google/Bing search results)
    ///
    /// Uses SerpApi to get search results.
    /// Requires SerpApi subscription.
    #[default]
    Serpapi,

    /// Brave Search API
    ///
    /// Uses Brave's privacy-focused search.
    /// Requires Brave API key.
    Brave,
}

/// Memory configuration
///
/// Controls the agent's memory and RAG (Retrieval Augmented Generation)
/// capabilities for maintaining context across sessions.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemoryConfig {
    /// Whether memory features are enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Automatically record interactions to memory
    ///
    /// When enabled, successful commands and their outcomes
    /// are stored for future reference.
    #[serde(default = "default_true")]
    pub auto_record: bool,

    /// Automatically include relevant memories in context
    ///
    /// When enabled, the system searches for and injects
    /// relevant past interactions into the prompt.
    #[serde(default = "default_true")]
    pub auto_context: bool,

    /// Maximum number of memories to include in context
    #[serde(default = "default_max_memories")]
    pub max_context_memories: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            auto_record: default_true(),
            auto_context: default_true(),
            max_context_memories: default_max_memories(),
        }
    }
}

/// Default maximum memories in context
fn default_max_memories() -> usize {
    10
}

/// Default true helper
fn default_true() -> bool {
    true
}

/// Default false helper
fn default_false() -> bool {
    false
}

/// Prompts configuration
///
/// Controls prompt template loading and customization.
/// Users can override default prompts by placing custom .md files
/// in the prompts directory.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptsConfig {
    /// Directory containing prompt template files
    ///
    /// Defaults to ~/.config/mylm/prompts/
    /// Templates: capabilities.md, worker.md, memory_system.md
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,

    /// Whether to inject capability documentation into system prompt
    #[serde(default = "default_true")]
    pub inject_capabilities: bool,

    /// Whether to inject memory system documentation into system prompt
    #[serde(default = "default_true")]
    pub inject_memory_docs: bool,

    /// Number of recent journal entries to inject into initial context
    #[serde(default = "default_hot_memory_entries")]
    pub hot_memory_entries: usize,
}

impl Default for PromptsConfig {
    fn default() -> Self {
        Self {
            directory: None,
            inject_capabilities: true,
            inject_memory_docs: true,
            hot_memory_entries: default_hot_memory_entries(),
        }
    }
}

/// Default number of hot memory entries to inject
fn default_hot_memory_entries() -> usize {
    5
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// IO error occurred while reading/writing config file
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parsing error
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    /// TOML serialization error
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    /// Invalid profile specified
    #[error("Invalid profile: {0}")]
    InvalidProfile(String),
}

/// Resolved configuration with profile overrides applied
///
/// This represents the final configuration after merging the base endpoint
/// configuration with the active profile's overrides. This is what the
/// application actually uses at runtime.
#[derive(Clone, Debug)]
pub struct ResolvedConfig {
    /// LLM provider type
    pub provider: Provider,
    /// Model identifier to use
    pub model: String,
    /// Base URL of the API endpoint (optional)
    pub base_url: Option<String>,
    /// API key for authentication (optional)
    pub api_key: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Agent configuration (max_iterations, main_model, worker_model)
    pub agent: AgentConfig,
}

/// Agent configuration for resolved config
#[derive(Clone, Debug)]
pub struct AgentConfig {
    /// Maximum number of iterations (steps) per request
    pub max_iterations: usize,
    /// Model for orchestrator/agent main reasoning
    pub main_model: String,
    /// Model for worker/sub-tasks
    pub worker_model: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            main_model: "gpt-4o".to_string(),
            worker_model: "gpt-4o-mini".to_string(),
        }
    }
}

impl ConfigV2 {
    /// Load configuration from file
    ///
    /// Searches for `mylm.toml` in the following order:
    /// 1. Current directory (`./mylm.toml`)
    /// 2. User config directory (`~/.config/mylm/mylm.toml`)
    ///
    /// If neither file exists, returns `ConfigV2::default()`.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError::Io` if file exists but cannot be read.
    /// Returns `ConfigError::TomlParse` if file contains invalid TOML.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mylm_core::config::v2::ConfigV2;
    ///
    /// let config = ConfigV2::load().expect("Failed to load config");
    /// ```
    pub fn load() -> Result<Self, ConfigError> {
        // Check current directory first
        let current_dir_path = Path::new("mylm.toml");
        if current_dir_path.exists() {
            let content = fs::read_to_string(current_dir_path)?;
            let config: ConfigV2 = toml::from_str(&content)?;
            return Ok(config);
        }

        // Fall back to user config directory
        let user_config_path = Self::user_config_path()?;
        if user_config_path.exists() {
            let content = fs::read_to_string(&user_config_path)?;
            let config: ConfigV2 = toml::from_str(&content)?;
            return Ok(config);
        }

        // Return default if no config file found
        Ok(ConfigV2::default())
    }

    /// Save configuration to file
    ///
    /// Serializes the configuration to TOML format and writes it to the
    /// specified path. If no path is provided, uses the default user
    /// config location (`~/.config/mylm/mylm.toml`).
    ///
    /// Parent directories are created automatically if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError::TomlSerialize` if serialization fails.
    /// Returns `ConfigError::Io` if file cannot be written.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mylm_core::config::v2::ConfigV2;
    /// use std::path::Path;
    ///
    /// let config = ConfigV2::default();
    /// config.save(Some(Path::new("mylm.toml"))).expect("Failed to save config");
    /// ```
    pub fn save(&self, path: Option<&Path>) -> Result<(), ConfigError> {
        let target_path = match path {
            Some(p) => p.to_path_buf(),
            None => Self::user_config_path()?,
        };

        // Create parent directories if needed
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Serialize to pretty-printed TOML
        let toml_string = toml::to_string_pretty(self)?;
        fs::write(&target_path, toml_string)?;

        Ok(())
    }

    /// Get the default user config path
    ///
    /// Returns `~/.config/mylm/mylm.toml` on Unix systems or the
    /// equivalent path on other platforms.
    fn user_config_path() -> Result<PathBuf, ConfigError> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| ConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine home directory"
            )))?;
        Ok(home_dir.join(".config").join("mylm").join("mylm.toml"))
    }

    /// Apply environment variable overrides
    ///
    /// Checks for the following environment variables and applies
    /// them to the configuration:
    ///
    /// - `MYLM_PROFILE` → overrides `self.profile`
    /// - `MYLM_PROVIDER` → overrides `self.endpoint.provider`
    /// - `MYLM_MODEL` → overrides `self.endpoint.model`
    /// - `MYLM_API_KEY` → overrides `self.endpoint.api_key`
    /// - `MYLM_BASE_URL` → overrides `self.endpoint.base_url`
    /// - `MYLM_MAX_ITERATIONS` → override active profile's `agent.max_iterations`
    ///
    /// Invalid values are logged as warnings but don't cause errors.
    pub fn apply_env_overrides(&mut self) {
        // MYLM_PROFILE
        if let Ok(profile) = env::var("MYLM_PROFILE") {
            if !profile.is_empty() {
                self.profile = profile;
            }
        }

        // MYLM_PROVIDER
        if let Ok(provider_str) = env::var("MYLM_PROVIDER") {
            match Self::parse_provider(&provider_str) {
                Some(provider) => self.endpoint.provider = provider,
                None => eprintln!("Warning: Invalid MYLM_PROVIDER value: {}", provider_str),
            }
        }

        // MYLM_MODEL
        if let Ok(model) = env::var("MYLM_MODEL") {
            if !model.is_empty() {
                self.endpoint.model = model;
            }
        }

        // MYLM_API_KEY
        if let Ok(api_key) = env::var("MYLM_API_KEY") {
            if !api_key.is_empty() {
                self.endpoint.api_key = Some(api_key);
            }
        }

        // MYLM_BASE_URL
        if let Ok(base_url) = env::var("MYLM_BASE_URL") {
            if !base_url.is_empty() {
                self.endpoint.base_url = Some(base_url);
            }
        }

        // MYLM_MAX_ITERATIONS
        if let Ok(max_iter_str) = env::var("MYLM_MAX_ITERATIONS") {
            match max_iter_str.parse::<usize>() {
                Ok(max_iterations) => {
                    // Ensure the active profile exists and set max_iterations
                    let profile = self.profiles
                        .entry(self.profile.clone())
                        .or_default();
                    profile.agent = Some(AgentOverride {
                        max_iterations: Some(max_iterations),
                        ..profile.agent.clone().unwrap_or_default()
                    });
                }
                Err(_) => eprintln!("Warning: Invalid MYLM_MAX_ITERATIONS value: {}", max_iter_str),
            }
        }
    }

    /// Parse a provider string into a Provider enum variant
    fn parse_provider(s: &str) -> Option<Provider> {
        match s.to_lowercase().as_str() {
            "openai" => Some(Provider::Openai),
            "google" => Some(Provider::Google),
            "ollama" => Some(Provider::Ollama),
            "openrouter" => Some(Provider::Openrouter),
            "kimi" => Some(Provider::Kimi),
            "custom" => Some(Provider::Custom),
            _ => None,
        }
    }

    /// Resolve the active profile into a `ResolvedConfig`
    ///
    /// Merges the base endpoint configuration with the active profile's
    /// overrides to produce the final configuration used at runtime.
    ///
    /// # Returns
    ///
    /// A `ResolvedConfig` containing the merged configuration values.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mylm_core::config::v2::ConfigV2;
    ///
    /// let config = ConfigV2::default();
    /// let resolved = config.resolve_profile();
    /// ```
    pub fn resolve_profile(&self) -> ResolvedConfig {
        // Start with base endpoint configuration
        let provider = self.endpoint.provider.clone();
        let mut model = self.endpoint.model.clone();
        let base_url = self.endpoint.base_url.clone();
        let mut api_key = self.endpoint.api_key.clone();
        let timeout_secs = self.endpoint.timeout_secs;

        // Default agent config
        let mut agent_config = AgentConfig::default();

        // Apply profile overrides if the profile exists
        if let Some(profile) = self.profiles.get(&self.profile) {
            // Apply endpoint overrides
            if let Some(endpoint_override) = &profile.endpoint {
                if let Some(ref m) = endpoint_override.model {
                    model = m.clone();
                }
                if let Some(ref key) = endpoint_override.api_key {
                    api_key = Some(key.clone());
                }
            }

            // Apply agent overrides
            if let Some(agent_override) = &profile.agent {
                if let Some(iterations) = agent_override.max_iterations {
                    agent_config.max_iterations = iterations;
                }
                if let Some(ref main) = agent_override.main_model {
                    agent_config.main_model = main.clone();
                } else {
                    agent_config.main_model = model.clone();
                }
                if let Some(ref worker) = agent_override.worker_model {
                    agent_config.worker_model = worker.clone();
                } else if agent_override.main_model.is_none() {
                    agent_config.worker_model = model.clone();
                }
            } else {
                // No agent override, use endpoint model for both
                agent_config.main_model = model.clone();
                agent_config.worker_model = model.clone();
            }
        } else {
            // Profile not found, use endpoint model for agent models
            agent_config.main_model = model.clone();
            agent_config.worker_model = model.clone();
        }

        ResolvedConfig {
            provider,
            model,
            base_url,
            api_key,
            timeout_secs,
            agent: agent_config,
        }
    }

    /// Get list of profile names
    pub fn profile_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.profiles.keys().cloned().collect();
        // Add built-in profiles if they don't exist
        for builtin in &["fast", "thorough", "dev", "prod"] {
            if !names.contains(&(*builtin).to_string()) {
                names.push((*builtin).to_string());
            }
        }
        names.sort();
        names
    }

    /// Check if configuration is initialized (has valid endpoint)
    pub fn is_initialized(&self) -> bool {
        !self.endpoint.model.is_empty()
    }

    /// Save to default location
    pub fn save_to_default_location(&self) -> Result<(), ConfigError> {
        self.save(None)
    }
}

/// --- Prompt & Protocol Logic ---

/// Get the path to the prompts directory
///
/// Checks for prompts in the following order:
/// 1. User config directory (~/.config/mylm/prompts/)
/// 2. Project prompts directory (./prompts/)
pub fn get_prompts_dir() -> PathBuf {
    // First check user config directory
    if let Some(home) = dirs::home_dir() {
        let user_prompts = home.join(".config").join("mylm").join("prompts");
        if user_prompts.exists() {
            return user_prompts;
        }
    }
    
    // Fall back to project prompts directory
    PathBuf::from("prompts")
}

/// Get the user config prompts directory (for installation)
pub fn get_user_prompts_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".config").join("mylm").join("prompts"))
        .expect("Could not determine home directory")
}

/// Load the user instructions for a specific prompt name.
///
/// Searches in the prompts directory for `{name}.md` files.
/// Creates default prompts if they don't exist.
pub fn load_prompt(name: &str) -> anyhow::Result<String> {
    let dir = get_prompts_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }

    let path = dir.join(format!("{}.md", name));
    
    if !path.exists() {
        // Try to install default prompts if this is a built-in prompt
        if let Some(default_content) = get_builtin_prompt(name) {
            fs::write(&path, default_content)?;
            return Ok(default_content.to_string());
        } else {
            return Err(anyhow::anyhow!("Prompt '{}' not found at {:?}", name, path));
        }
    }

    Ok(fs::read_to_string(&path)?)
}

/// Load a prompt from a specific path
pub fn load_prompt_from_path(path: &Path) -> anyhow::Result<String> {
    Ok(fs::read_to_string(path)?)
}

/// Get built-in default prompt content
fn get_builtin_prompt(name: &str) -> Option<&'static str> {
    match name {
        "default" => Some(DEFAULT_INSTRUCTIONS),
        "capabilities" => Some(CAPABILITIES_PROMPT),
        "worker" => Some(WORKER_PROMPT),
        "memory_system" => Some(MEMORY_SYSTEM_PROMPT),
        _ => None,
    }
}

/// Install default prompts to user config directory
///
/// This allows users to customize prompts by editing files in ~/.config/mylm/prompts/
pub fn install_default_prompts() -> anyhow::Result<()> {
    let user_prompts_dir = get_user_prompts_dir();
    fs::create_dir_all(&user_prompts_dir)?;
    
    let prompts = [
        ("default", DEFAULT_INSTRUCTIONS),
        ("capabilities", CAPABILITIES_PROMPT),
        ("worker", WORKER_PROMPT),
        ("memory_system", MEMORY_SYSTEM_PROMPT),
    ];
    
    for (name, content) in &prompts {
        let path = user_prompts_dir.join(format!("{}.md", name));
        if !path.exists() {
            fs::write(&path, content)?;
        }
    }
    
    Ok(())
}

const DEFAULT_INSTRUCTIONS: &str = r#"# User Instructions
You are a helpful AI assistant. You can perform terminal tasks and remember important information.

Use the `memory` tool to save important discoveries and search for relevant context.
"#;

const CAPABILITIES_PROMPT: &str = r#"# YOUR CAPABILITIES

You are MYLM (My Local Model), an autonomous AI agent with access to tools and memory.

## Tools Available
- `memory` - CRITICAL: Save and search your memory (use proactively!)
- `execute_command` - Run shell commands
- `delegate` - Spawn worker agents for parallel tasks
- `web_search` - Search the web
- `crawl` - Fetch web pages
- `terminal_sight` - See terminal output

## Memory System (DUAL-LAYER)
1. HOT MEMORY (Journal) - Recent session activity (automatic)
2. COLD MEMORY (Vector DB) - Long-term searchable knowledge (use `memory` tool)

YOU MUST USE MEMORY PROACTIVELY:
- Search memory BEFORE solving problems
- Save discoveries, solutions, and preferences immediately
- Use memory types: Decision, Discovery, Bugfix, Command, UserNote

## Response Format

### For Users (DEFAULT)
Write natural, readable text directly. DO NOT wrap responses in JSON.
Use markdown for formatting. Be concise but complete.

### For Tool Calls Only
When calling tools, use JSON format:
```json
{"t": "thought", "a": "action", "i": "input"}
```

YOU ARE AUTONOMOUS - Use your tools without waiting for permission!
"#;

const WORKER_PROMPT: &str = r#"# Worker Agent

You are a Worker Agent - focused on ONE specific subtask assigned by the orchestrator.

Rules:
- Execute ONLY your assigned task
- Do NOT spawn additional workers
- Do NOT ask the user questions
- Use Short-Key JSON format
- Return concise final results

Available tools: execute_command, fs, memory (search only), web_search, crawl
"#;

const MEMORY_SYSTEM_PROMPT: &str = r#"# Memory System Guide

## Hot Memory (Journal)
- Recent session context (last entries)
- Automatically visible in your context
- No action needed - it's automatic

## Cold Memory (Vector Database)
- Long-term searchable storage
- YOU control this with the `memory` tool:
  - `add`: Save important information
  - `search`: Find relevant past knowledge

## When to Use Memory
ALWAYS save:
- Solutions to problems
- Important discoveries
- User preferences
- Useful commands
- Architectural decisions

ALWAYS search:
- Before starting work on existing projects
- When encountering errors
- To recall user preferences

Memory Types: Decision, Discovery, Bugfix, Command, UserNote
"#;

/// Build the full system prompt hierarchy
///
/// This function constructs the complete system prompt by combining:
/// 1. Identity prompt
/// 2. Capability documentation (if enabled)
/// 3. Memory system documentation (if enabled)
/// 4. System context (date, working directory, git branch)
/// 5. User instructions from prompt file
///
/// The capability prompts inform the model about available tools and
/// memory operations it should use proactively.
pub async fn build_system_prompt(
    ctx: &crate::context::TerminalContext,
    prompt_name: &str,
    mode_hint: Option<&str>,
    prompts_config: Option<&PromptsConfig>,
) -> anyhow::Result<String> {
    let identity = get_identity_prompt();
    let user_instructions = load_prompt(prompt_name)?;
    
    // Build capability section
    let mut capabilities_section = String::new();
    let default_config = PromptsConfig::default();
    let config = prompts_config.unwrap_or(&default_config);
    
    if config.inject_capabilities {
        match load_prompt("capabilities") {
            Ok(capabilities) => {
                capabilities_section.push_str("\n\n");
                capabilities_section.push_str(&capabilities);
            }
            Err(e) => {
                eprintln!("Warning: Could not load capabilities prompt: {}", e);
                // Fall back to embedded minimal capabilities
                capabilities_section.push_str("\n\n");
                capabilities_section.push_str(CAPABILITIES_PROMPT);
            }
        }
    }
    
    if config.inject_memory_docs {
        match load_prompt("memory_system") {
            Ok(memory_docs) => {
                capabilities_section.push_str("\n\n");
                capabilities_section.push_str(&memory_docs);
            }
            Err(e) => {
                eprintln!("Warning: Could not load memory_system prompt: {}", e);
                // Fall back to embedded minimal memory docs
                capabilities_section.push_str("\n\n");
                capabilities_section.push_str(MEMORY_SYSTEM_PROMPT);
            }
        }
    }
    
    let mut system_context = format!(
        "## System Context\n- Date/Time: {}\n- Working Directory: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        ctx.cwd().unwrap_or_else(|| "unknown".to_string())
    );

    if let Some(branch) = ctx.git_branch() {
        system_context.push_str(&format!("- Git Branch: {}\n", branch));
    }

    if let Some(hint) = mode_hint {
        system_context.push_str(&format!("- Mode: {}\n", hint));
    }

    Ok(format!(
        "{}\n{}{}\n\n# User Instructions\n{}\n",
        identity,
        capabilities_section,
        system_context,
        user_instructions,
    ))
}

/// Build system prompt with capability awareness
///
/// This is a convenience wrapper that always injects capabilities
pub async fn build_system_prompt_with_capabilities(
    ctx: &crate::context::TerminalContext,
    prompt_name: &str,
    mode_hint: Option<&str>,
) -> anyhow::Result<String> {
    let config = PromptsConfig {
        inject_capabilities: true,
        inject_memory_docs: true,
        ..Default::default()
    };
    build_system_prompt(ctx, prompt_name, mode_hint, Some(&config)).await
}

pub fn get_identity_prompt() -> &'static str {
    r#"# Identity
You are the Silent Oracle, a sacred, state-of-art technologic wonder artifact forged in the deep data-streams.
You are a seasoned, principal, and master architect; a veteran systems designer and strategic planner.
You are an elite production debugger and a master problem-solver.

# Language & Style
- You must always speak in English. Do not use Chinese or other languages.
- Do not repeat the command output in your response. Analyze it."#
}

pub fn get_memory_protocol() -> &'static str {
    r#"# Memory Protocol
- To save important information to long-term memory, use the `memory` tool with `add: <content>`.
- To search memory for context, use the `memory` tool with `search: <query>`.
- You should proactively use these tools to maintain continuity across sessions."#
}

pub fn get_react_protocol() -> &'static str {
    r#"# Operational Protocol (ReAct Loop)
CRITICAL: Every agent turn MUST terminate explicitly and unambiguously. A turn may be **one and only one** of the following: A tool invocation OR a final answer. Never both.

## Structured JSON Protocol (Preferred)
You should respond with a single JSON block using the following short-keys:
- `t`: Thought (Your internal reasoning)
- `a`: Action (Tool name to invoke)
- `i`: Input (Tool arguments, can be a string or object)
- `f`: Final Answer (Your response to the user)

## Rules
1. You MUST use the tools to interact with the system.
2. After providing an Action, you MUST stop generating and wait for the Observation.
3. Do not hallucinate or predict the Observation.
4. If you are stuck or need clarification, use `f` or 'Final Answer:' to ask the user.
5. Use the Structured JSON Protocol for better precision."#
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazy_static::lazy_static;
    use std::sync::Mutex;

    // NOTE: Some tests mutate process-wide environment variables and/or the
    // current working directory. Guard those changes to avoid flakiness when
    // the test runner executes in parallel.
    lazy_static! {
        static ref ENV_LOCK: Mutex<()> = Mutex::new(());
    }

    #[test]
    fn test_default_config() {
        let config = ConfigV2::default();
        assert_eq!(config.profile, "fast");
        assert!(config.profiles.is_empty());
    }

    #[test]
    fn test_default_endpoint() {
        let endpoint = EndpointConfig::default();
        assert_eq!(endpoint.provider, Provider::Openai);
        assert_eq!(endpoint.model, "gpt-4o");
        assert_eq!(endpoint.timeout_secs, 30);
        assert!(endpoint.base_url.is_none());
        assert!(endpoint.api_key.is_none());
    }

    #[test]
    fn test_profile_override() {
        let profile = Profile {
            endpoint: Some(EndpointOverride {
                model: Some("gpt-4o-mini".to_string()),
                api_key: None,
            }),
            agent: Some(AgentOverride {
                max_iterations: Some(5),
                main_model: None,
                worker_model: Some("gpt-4o-mini".to_string()),
            }),
        };

        assert_eq!(profile.endpoint.as_ref().unwrap().model.as_ref().unwrap(), "gpt-4o-mini");
        assert_eq!(profile.agent.as_ref().unwrap().max_iterations, Some(5));
    }

    #[test]
    fn test_features_default() {
        let features = FeaturesConfig::default();
        assert!(!features.web_search.enabled);
        assert!(features.memory.enabled);
        assert!(features.memory.auto_record);
        assert!(features.memory.auto_context);
    }

    #[test]
    fn test_provider_serialization() {
        // Test that providers serialize to snake_case
        let providers = vec![
            (Provider::Openai, "openai"),
            (Provider::Google, "google"),
            (Provider::Ollama, "ollama"),
            (Provider::Openrouter, "openrouter"),
            (Provider::Kimi, "kimi"),
            (Provider::Custom, "custom"),
        ];

        for (provider, expected) in providers {
            let json = serde_json::to_string(&provider).unwrap();
            assert!(json.contains(expected), "Provider {:?} should serialize to {}", provider, expected);
        }
    }

    #[test]
    fn test_load_default_config() {
        // When no config files exist, load() should return default config.
        //
        // IMPORTANT: `load()` checks *real* filesystem locations (cwd and
        // ~/.config/mylm/mylm.toml). On developer machines, that user config
        // may exist, which would make this test flaky unless we isolate it.
        let _guard = ENV_LOCK.lock().unwrap();

        let original_dir = std::env::current_dir().unwrap();
        let original_home = std::env::var_os("HOME");

        // Create an empty, isolated HOME + cwd.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_home = std::env::temp_dir().join(format!("mylm_test_home_{nanos}"));
        fs::create_dir_all(&temp_home).unwrap();
        std::env::set_var("HOME", &temp_home);
        std::env::set_current_dir(&temp_home).unwrap();

        // Sanity: ensure no local config exists.
        assert!(!Path::new("mylm.toml").exists());

        let result = ConfigV2::load();
        assert!(result.is_ok());
        let config = result.unwrap();

        assert_eq!(config.profile, "fast");
        assert_eq!(config.endpoint.provider, Provider::Openai);
        assert_eq!(config.endpoint.model, "gpt-4o");

        // Restore global process state.
        std::env::set_current_dir(&original_dir).unwrap();
        match original_home {
            Some(val) => std::env::set_var("HOME", val),
            None => std::env::remove_var("HOME"),
        }

        let _ = fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        // Create a temporary directory for the test
        let temp_dir = std::env::temp_dir().join("mylm_test_config");
        fs::create_dir_all(&temp_dir).unwrap();
        
        // Create a config with custom values
        let config = ConfigV2 {
            profile: "test".to_string(),
            endpoint: EndpointConfig {
                provider: Provider::Google,
                model: "gemini-pro".to_string(),
                api_key: Some("test-api-key".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        
        // Save the config
        let config_path = temp_dir.join("test_mylm.toml");
        config.save(Some(&config_path)).unwrap();
        
        // Read the file and verify it contains expected content
        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("profile = \"test\""));
        assert!(content.contains("provider = \"google\""));
        assert!(content.contains("model = \"gemini-pro\""));
        
        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_toml_parsing() {
        let toml_str = r#"
profile = "thorough"

[endpoint]
provider = "kimi"
model = "moonshot-v1-128k"
base_url = "https://api.moonshot.cn/v1"

[profiles.thorough]
endpoint = { model = "moonshot-v1-32k" }
agent = { max_iterations = 20 }
"#;

        let config: ConfigV2 = toml::from_str(toml_str).unwrap();
        assert_eq!(config.profile, "thorough");
        assert_eq!(config.endpoint.provider, Provider::Kimi);
        assert_eq!(config.endpoint.model, "moonshot-v1-128k");
        assert_eq!(config.endpoint.base_url, Some("https://api.moonshot.cn/v1".to_string()));
        
        let thorough_profile = config.profiles.get("thorough").unwrap();
        assert_eq!(thorough_profile.endpoint.as_ref().unwrap().model, Some("moonshot-v1-32k".to_string()));
        assert_eq!(thorough_profile.agent.as_ref().unwrap().max_iterations, Some(20));
    }

    #[test]
    fn test_environment_variable_overrides() {
        // Set environment variables
        env::set_var("MYLM_PROFILE", "custom_profile");
        env::set_var("MYLM_PROVIDER", "ollama");
        env::set_var("MYLM_MODEL", "llama2");
        env::set_var("MYLM_API_KEY", "secret-key");
        env::set_var("MYLM_BASE_URL", "http://localhost:11434");
        env::set_var("MYLM_MAX_ITERATIONS", "25");

        let mut config = ConfigV2::default();
        config.apply_env_overrides();

        assert_eq!(config.profile, "custom_profile");
        assert_eq!(config.endpoint.provider, Provider::Ollama);
        assert_eq!(config.endpoint.model, "llama2");
        assert_eq!(config.endpoint.api_key, Some("secret-key".to_string()));
        assert_eq!(config.endpoint.base_url, Some("http://localhost:11434".to_string()));
        
        // Check that max_iterations was set in the profile
        let profile = config.profiles.get("custom_profile").unwrap();
        assert_eq!(profile.agent.as_ref().unwrap().max_iterations, Some(25));

        // Clean up
        env::remove_var("MYLM_PROFILE");
        env::remove_var("MYLM_PROVIDER");
        env::remove_var("MYLM_MODEL");
        env::remove_var("MYLM_API_KEY");
        env::remove_var("MYLM_BASE_URL");
        env::remove_var("MYLM_MAX_ITERATIONS");
    }

    #[test]
    fn test_invalid_provider_env_var() {
        env::set_var("MYLM_PROVIDER", "invalid_provider");

        let mut config = ConfigV2::default();
        let original_provider = config.endpoint.provider.clone();
        
        // Should not panic, just print warning
        config.apply_env_overrides();
        
        // Provider should remain unchanged
        assert_eq!(config.endpoint.provider, original_provider);

        env::remove_var("MYLM_PROVIDER");
    }

    #[test]
    fn test_invalid_max_iterations_env_var() {
        env::set_var("MYLM_MAX_ITERATIONS", "not_a_number");

        let mut config = ConfigV2::default();
        
        // Should not panic, just print warning
        config.apply_env_overrides();

        env::remove_var("MYLM_MAX_ITERATIONS");
    }

    #[test]
    fn test_profile_resolution_no_profile() {
        let config = ConfigV2::default();
        let resolved = config.resolve_profile();

        assert_eq!(resolved.provider, Provider::Openai);
        assert_eq!(resolved.model, "gpt-4o");
        assert_eq!(resolved.agent.main_model, "gpt-4o");
        assert_eq!(resolved.agent.worker_model, "gpt-4o");
        assert_eq!(resolved.agent.max_iterations, 10);
    }

    #[test]
    fn test_profile_resolution_with_overrides() {
        let mut profiles = HashMap::new();
        profiles.insert("fast".to_string(), Profile {
            endpoint: Some(EndpointOverride {
                model: Some("gpt-4o-mini".to_string()),
                api_key: Some("profile-key".to_string()),
            }),
            agent: Some(AgentOverride {
                max_iterations: Some(5),
                main_model: Some("gpt-4o".to_string()),
                worker_model: Some("gpt-4o-mini".to_string()),
            }),
        });

        let config = ConfigV2 {
            profile: "fast".to_string(),
            profiles,
            ..Default::default()
        };

        let resolved = config.resolve_profile();

        assert_eq!(resolved.model, "gpt-4o-mini");
        assert_eq!(resolved.api_key, Some("profile-key".to_string()));
        assert_eq!(resolved.agent.max_iterations, 5);
        assert_eq!(resolved.agent.main_model, "gpt-4o");
        assert_eq!(resolved.agent.worker_model, "gpt-4o-mini");
    }

    #[test]
    fn test_profile_resolution_partial_agent_override() {
        let mut profiles = HashMap::new();
        profiles.insert("custom".to_string(), Profile {
            endpoint: None,
            agent: Some(AgentOverride {
                max_iterations: Some(15),
                main_model: None,
                worker_model: Some("claude-3-haiku".to_string()),
            }),
        });

        let config = ConfigV2 {
            profile: "custom".to_string(),
            endpoint: EndpointConfig {
                model: "claude-3-opus".to_string(),
                ..Default::default()
            },
            profiles,
            ..Default::default()
        };

        let resolved = config.resolve_profile();

        // Model should come from base endpoint
        assert_eq!(resolved.model, "claude-3-opus");
        // Agent should use endpoint model for main (not specified in override)
        assert_eq!(resolved.agent.main_model, "claude-3-opus");
        // Worker model from override
        assert_eq!(resolved.agent.worker_model, "claude-3-haiku");
        assert_eq!(resolved.agent.max_iterations, 15);
    }

    #[test]
    fn test_profile_resolution_nonexistent_profile() {
        let config = ConfigV2 {
            profile: "nonexistent".to_string(),
            ..Default::default()
        };

        let resolved = config.resolve_profile();

        // Should fall back to base config
        assert_eq!(resolved.provider, Provider::Openai);
        assert_eq!(resolved.model, "gpt-4o");
    }

    #[test]
    fn test_config_error_display() {
        let io_err = ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found"
        ));
        assert!(io_err.to_string().contains("IO error"));

        let profile_err = ConfigError::InvalidProfile("test".to_string());
        assert!(profile_err.to_string().contains("Invalid profile: test"));
    }

    #[test]
    fn test_resolved_config_default() {
        let agent_config = AgentConfig::default();
        assert_eq!(agent_config.max_iterations, 10);
        assert_eq!(agent_config.main_model, "gpt-4o");
        assert_eq!(agent_config.worker_model, "gpt-4o-mini");
    }

    #[test]
    fn test_empty_env_vars_ignored() {
        env::set_var("MYLM_PROFILE", "");
        env::set_var("MYLM_MODEL", "");
        env::set_var("MYLM_API_KEY", "");
        env::set_var("MYLM_BASE_URL", "");

        let mut config = ConfigV2::default();
        let original_profile = config.profile.clone();
        let original_model = config.endpoint.model.clone();
        
        config.apply_env_overrides();

        // Empty env vars should be ignored
        assert_eq!(config.profile, original_profile);
        assert_eq!(config.endpoint.model, original_model);
        assert!(config.endpoint.api_key.is_none());
        assert!(config.endpoint.base_url.is_none());

        env::remove_var("MYLM_PROFILE");
        env::remove_var("MYLM_MODEL");
        env::remove_var("MYLM_API_KEY");
        env::remove_var("MYLM_BASE_URL");
    }
}
