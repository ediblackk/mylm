use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::types::{Provider, SearchProvider, ConfigError, AgentPermissions};

/// Root configuration structure for mylm v2
///
/// This is the top-level configuration that combines the base endpoint
/// configuration with profile-specific overrides and feature toggles.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConfigV2 {
    /// Active profile selection (default: "default")
    #[serde(default = "default_profile")]
    pub profile: String,

    /// Base endpoint configuration (legacy - kept for compatibility)
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

/// Default active profile name
fn default_profile() -> String {
    "default".to_string()
}

/// Default provider name
fn default_provider_name() -> String {
    "openai".to_string()
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

    /// Maximum context tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<usize>,

    /// Input price per 1k tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_price: Option<f64>,

    /// Output price per 1k tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_price: Option<f64>,

    /// Tokens to trigger summary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condensation_threshold: Option<usize>,
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
            condensation_threshold: None,
        }
    }
}

/// Default model name
fn default_model() -> String {
    "default-model".to_string()
}

/// Default timeout in seconds
fn default_timeout_secs() -> u64 {
    30
}

/// Provider configuration for multi-provider support
///
/// Stores configuration for a specific LLM provider.
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

impl ProviderConfig {
    /// Create a new provider config with default URL for the provider type
    pub fn new(provider_type: Provider, api_key: Option<String>) -> Self {
        let base_url = provider_type.default_url();
        Self {
            provider_type,
            base_url,
            api_key,
            timeout_secs: default_timeout_secs(),
        }
    }
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

    /// Delay in milliseconds between iterations
    ///
    /// - 0: No delay (default)
    /// - Higher values: Add pause between agentic actions
    ///   Useful for rate limiting or to observe agent behavior
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iteration_rate_limit: Option<u64>,

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

    /// Maximum context tokens for this profile's agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<usize>,

    /// Input price per 1M tokens for this profile's agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_price: Option<f64>,

    /// Output price per 1M tokens for this profile's agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_price: Option<f64>,

    /// Tokens to trigger summary for this profile's agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condensation_threshold: Option<usize>,

    /// Permission controls for this agent profile
    pub permissions: Option<AgentPermissions>,

    /// Rate limit for main agent (requests per minute)
    ///
    /// - 0: No limit (default)
    /// - Lower values: Slower requests, prevents rate limiting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_rpm: Option<u32>,

    /// Rate limit for workers (shared pool, requests per minute)
    ///
    /// - 0: Uses default (30)
    /// - Lower values: Slower worker execution, prevents spamming provider
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workers_rpm: Option<u32>,

    /// Maximum number of concurrent workers (background jobs)
    ///
    /// - Lower values (5-10): Conservative, less provider load
    /// - Higher values (50-100): Aggressive parallel execution
    /// - Default: 20
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_limit: Option<usize>,

    /// Rate limit tier for the provider
    ///
    /// Predefined configurations based on provider capabilities:
    /// - conservative: Basic/free tier (10 workers, 60 RPM)
    /// - standard: Standard tier (20 workers, 120 RPM)
    /// - high: Premium tier (50 workers, 300 RPM)
    /// - enterprise: Unlimited tier (100 workers, 600 RPM)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_tier: Option<String>,
    
    /// Maximum actions before a worker job is considered stalled
    ///
    /// Prevents workers from looping indefinitely without returning a final answer.
    /// - Lower values (5-10): Stricter, catches runaway workers faster
    /// - Higher values (20-30): Allows complex multi-step workflows
    /// - Default: 15
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_actions_before_stall: Option<usize>,
    
    /// Maximum consecutive messages without tool use
    ///
    /// After this many conversational messages, the worker is reminded to use tools.
    /// - Lower values (2-3): More aggressive tool pushing
    /// - Higher values (5-10): Allows more back-and-forth
    /// - Default: 3
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_consecutive_messages: Option<u32>,
    
    /// Maximum recovery attempts after errors
    ///
    /// Number of times to retry after encountering errors before giving up.
    /// - Lower values (1-2): Fail fast
    /// - Higher values (5-10): More resilient to transient errors
    /// - Default: 3
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_recovery_attempts: Option<u32>,
    
    /// Maximum consecutive tool failures before worker is stalled
    ///
    /// Prevents workers from retrying failed tools indefinitely.
    /// - Lower values (2-3): Fail fast on tool errors
    /// - Higher values (5-10): More resilient to transient tool failures
    /// - Default: 5
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_failures: Option<usize>,
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

    /// Parallel Consistency Reasoning (PaCoRe) settings
    #[serde(default)]
    pub pacore: PacoreConfig,

    /// Agent version selection
    ///
    /// Choose between V1 (Classic ReAct) and V2 (Cognitive Submodule) agent architectures.
    /// V2 is the default and recommended for most use cases.
    #[serde(default)]
    pub agent_version: crate::config::AgentVersion,
}

/// Parallel Consistency Reasoning (PaCoRe) configuration
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PacoreConfig {
    /// Whether PaCoRe reasoning is enabled
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// Reasoning rounds (e.g., "4,1" means 4 parallel samples then 1 synthesis)
    #[serde(default = "default_pacore_rounds")]
    pub rounds: String,

    /// Optional specific model for reasoning (defaults to profile main model)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl Default for PacoreConfig {
    fn default() -> Self {
        Self {
            enabled: default_false(),
            rounds: default_pacore_rounds(),
            model: None,
        }
    }
}

fn default_pacore_rounds() -> String {
    "4,1".to_string()
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

    /// System prompt config name to use (default: "default")
    ///
    /// Specifies which prompt configuration to load for the main system prompt.
    /// The config will be loaded from ~/.config/mylm/prompts/config/{name}.json
    /// or fall back to embedded defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,

    /// Worker prompt config name to use (default: "worker")
    ///
    /// Specifies which prompt configuration to load for worker/agent tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_prompt: Option<String>,

    /// Memory prompt config name to use (default: "memory")
    ///
    /// Specifies which prompt configuration to load for memory operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_prompt: Option<String>,
}

impl Default for PromptsConfig {
    fn default() -> Self {
        Self {
            directory: None,
            inject_capabilities: true,
            inject_memory_docs: true,
            hot_memory_entries: default_hot_memory_entries(),
            system_prompt: None,
            worker_prompt: None,
            memory_prompt: None,
        }
    }
}

/// Default number of hot memory entries to inject
fn default_hot_memory_entries() -> usize {
    5
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
    /// Maximum context tokens
    pub max_context_tokens: usize,
    /// Agent configuration (max_iterations, main_model, worker_model)
    pub agent: AgentConfig,
    /// PaCoRe configuration
    pub pacore: PacoreConfig,
}

/// Agent configuration for resolved config
#[derive(Clone, Debug)]
pub struct AgentConfig {
    /// Maximum number of iterations (steps) per request
    pub max_iterations: usize,
    /// Delay in milliseconds between iterations
    pub iteration_rate_limit: u64,
    /// Model for orchestrator/agent main reasoning
    pub main_model: String,
    /// Model for worker/sub-tasks
    pub worker_model: String,

    /// Maximum context tokens for this agent
    pub max_context_tokens: usize,

    /// Permission controls for this agent
    pub permissions: Option<AgentPermissions>,

    /// Rate limit for main agent (requests per minute)
    pub main_rpm: u32,
    /// Rate limit for workers (shared pool, requests per minute)
    pub workers_rpm: u32,
    /// Maximum number of concurrent workers (background jobs)
    pub worker_limit: usize,
    /// Rate limit tier for the provider
    pub rate_limit_tier: String,
    
    /// Maximum actions before a worker job is considered stalled
    /// Prevents workers from looping indefinitely without returning a final answer
    pub max_actions_before_stall: usize,
    /// Maximum consecutive messages without tool use before pushing for action
    pub max_consecutive_messages: u32,
    /// Maximum recovery attempts after errors before giving up
    pub max_recovery_attempts: u32,
    /// Maximum consecutive tool failures before worker is stalled
    pub max_tool_failures: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            iteration_rate_limit: 0,
            main_model: "default-model".to_string(),
            worker_model: "default-worker-model".to_string(),
            max_context_tokens: 128000,
            permissions: None,
            main_rpm: 0,           // 0 = no limit
            workers_rpm: 300,      // Default: 5 req/sec shared for workers (increased from 30)
            worker_limit: 20,      // Default: 20 concurrent workers
            rate_limit_tier: "standard".to_string(), // Default: standard tier
            max_actions_before_stall: 15,  // Default: 15 actions before stall detection
            max_consecutive_messages: 3,   // Default: 3 messages before action reminder
            max_recovery_attempts: 3,      // Default: 3 recovery attempts after errors
            max_tool_failures: 5,          // Default: 5 tool failures before stall
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
        let mut max_context_tokens = self.endpoint.max_context_tokens.unwrap_or(128000);

        // Default agent config
        let mut agent_config = AgentConfig::default();
        agent_config.max_context_tokens = max_context_tokens;

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
                if let Some(tokens) = agent_override.max_context_tokens {
                    max_context_tokens = tokens;
                    agent_config.max_context_tokens = tokens;
                }
                if let Some(iterations) = agent_override.max_iterations {
                    agent_config.max_iterations = iterations;
                }
                if let Some(rate_limit) = agent_override.iteration_rate_limit {
                    agent_config.iteration_rate_limit = rate_limit;
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
                if let Some(main_rpm) = agent_override.main_rpm {
                    agent_config.main_rpm = main_rpm;
                }
                if let Some(workers_rpm) = agent_override.workers_rpm {
                    agent_config.workers_rpm = workers_rpm;
                }
                if let Some(worker_limit) = agent_override.worker_limit {
                    agent_config.worker_limit = worker_limit;
                }
                if let Some(ref tier) = agent_override.rate_limit_tier {
                    agent_config.rate_limit_tier = tier.clone();
                }
                if let Some(max_actions) = agent_override.max_actions_before_stall {
                    agent_config.max_actions_before_stall = max_actions;
                }
                if let Some(max_messages) = agent_override.max_consecutive_messages {
                    agent_config.max_consecutive_messages = max_messages;
                }
                if let Some(max_recovery) = agent_override.max_recovery_attempts {
                    agent_config.max_recovery_attempts = max_recovery;
                }
                if let Some(max_failures) = agent_override.max_tool_failures {
                    agent_config.max_tool_failures = max_failures;
                }
                // Copy permissions from agent override
                if agent_override.permissions.is_some() {
                    agent_config.permissions = agent_override.permissions.clone();
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
            max_context_tokens,
            agent: agent_config,
            pacore: self.features.pacore.clone(),
        }
    }

    /// Get list of profile names (only existing profiles, no fake built-ins)
    pub fn profile_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.profiles.keys().cloned().collect();
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
