//! Agent Configuration
//!
//! Configuration for agent behavior, tools, and capabilities.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Session configuration
    pub session: crate::agent::session::SessionConfig,
    
    /// Tool configuration
    pub tools: ToolConfig,
    
    /// LLM configuration
    pub llm: LlmConfig,
    
    /// Retry configuration
    pub retry: RetryConfig,
    
    /// Memory configuration
    pub memory: MemoryConfig,
    
    /// Worker configuration
    pub workers: WorkerConfig,
    
    /// Telemetry configuration
    pub telemetry: TelemetryConfig,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            session: crate::agent::session::SessionConfig::default(),
            tools: ToolConfig::default(),
            llm: LlmConfig::default(),
            retry: RetryConfig::default(),
            memory: MemoryConfig::default(),
            workers: WorkerConfig::default(),
            telemetry: TelemetryConfig::default(),
        }
    }
}

impl AgentConfig {
    /// Load from TOML file
    pub fn from_file(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }
    
    /// Save to TOML file
    pub fn to_file(&self, path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
    
    /// Load from default location or create default
    pub fn load() -> Self {
        if let Some(config_dir) = dirs::config_dir() {
            let path = config_dir.join("mylm").join("agent.toml");
            if path.exists() {
                if let Ok(config) = Self::from_file(&path) {
                    return config;
                }
            }
        }
        Self::default()
    }
    
    /// Merge with another config (other takes precedence)
    pub fn merge(&mut self, other: AgentConfig) {
        self.session = other.session;
        self.tools.merge(other.tools);
        self.llm.merge(other.llm);
        self.retry.merge(other.retry);
        self.memory.merge(other.memory);
        self.workers.merge(other.workers);
        self.telemetry.merge(other.telemetry);
    }
}

/// Tool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    /// Blocked commands (for shell tool)
    pub blocked_commands: Vec<String>,
    /// Allowed paths (for file tools)
    pub allowed_paths: Vec<PathBuf>,
    /// Require approval for dangerous tools
    pub require_approval: bool,
    /// Tool timeout in seconds
    pub timeout_secs: u64,
    /// Enable tool result caching
    pub enable_caching: bool,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            blocked_commands: vec![
                "rm -rf /".to_string(),
                "sudo".to_string(),
                "chmod 777".to_string(),
                "> /dev/null".to_string(),
            ],
            allowed_paths: vec![],
            require_approval: true,
            timeout_secs: 30,
            enable_caching: false,
        }
    }
}

impl ToolConfig {
    fn merge(&mut self, other: Self) {
        if !other.blocked_commands.is_empty() {
            self.blocked_commands = other.blocked_commands;
        }
        if !other.allowed_paths.is_empty() {
            self.allowed_paths = other.allowed_paths;
        }
        self.require_approval = other.require_approval;
        self.timeout_secs = other.timeout_secs;
        self.enable_caching = other.enable_caching;
    }
}

/// LLM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Model to use
    pub model: String,
    /// Temperature (0.0 - 2.0)
    pub temperature: f32,
    /// Maximum tokens
    pub max_tokens: u32,
    /// System prompt
    pub system_prompt: Option<String>,
    /// Enable streaming
    pub streaming: bool,
    /// Response format ("json", "xml", "text")
    pub response_format: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model: "gpt-3.5-turbo".to_string(),
            temperature: 0.7,
            max_tokens: 4096,
            system_prompt: None,
            streaming: false,
            response_format: "json".to_string(),
        }
    }
}

impl LlmConfig {
    fn merge(&mut self, other: Self) {
        self.model = other.model;
        self.temperature = other.temperature;
        self.max_tokens = other.max_tokens;
        if other.system_prompt.is_some() {
            self.system_prompt = other.system_prompt;
        }
        self.streaming = other.streaming;
        self.response_format = other.response_format;
    }
}

/// Retry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Base delay in milliseconds
    pub base_delay_ms: u64,
    /// Maximum delay in milliseconds
    pub max_delay_ms: u64,
    /// Enable circuit breaker
    pub enable_circuit_breaker: bool,
    /// Circuit breaker failure threshold
    pub circuit_breaker_threshold: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 10000,
            enable_circuit_breaker: true,
            circuit_breaker_threshold: 5,
        }
    }
}

impl RetryConfig {
    fn merge(&mut self, other: Self) {
        self.max_retries = other.max_retries;
        self.base_delay_ms = other.base_delay_ms;
        self.max_delay_ms = other.max_delay_ms;
        self.enable_circuit_breaker = other.enable_circuit_breaker;
        self.circuit_breaker_threshold = other.circuit_breaker_threshold;
    }
}

/// Memory configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Enable memory
    pub enabled: bool,
    /// Maximum memories to store
    pub max_memories: usize,
    /// Enable semantic search
    pub semantic_search: bool,
    /// Embedding model
    pub embedding_model: String,
    /// Memories to include in prompt
    pub context_window: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_memories: 1000,
            semantic_search: true,
            embedding_model: "default".to_string(),
            context_window: 5,
        }
    }
}

impl MemoryConfig {
    fn merge(&mut self, other: Self) {
        self.enabled = other.enabled;
        self.max_memories = other.max_memories;
        self.semantic_search = other.semantic_search;
        self.embedding_model = other.embedding_model;
        self.context_window = other.context_window;
    }
}

/// Worker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    /// Enable workers
    pub enabled: bool,
    /// Maximum concurrent workers
    pub max_concurrent: usize,
    /// Worker timeout in seconds
    pub timeout_secs: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent: 3,
            timeout_secs: 300,
        }
    }
}

impl WorkerConfig {
    fn merge(&mut self, other: Self) {
        self.enabled = other.enabled;
        self.max_concurrent = other.max_concurrent;
        self.timeout_secs = other.timeout_secs;
    }
}

/// Telemetry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Enable telemetry
    pub enabled: bool,
    /// Log level ("error", "warn", "info", "debug")
    pub log_level: String,
    /// Log to file
    pub log_to_file: bool,
    /// Log file path
    pub log_file_path: Option<PathBuf>,
    /// Enable metrics collection
    pub enable_metrics: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_level: "info".to_string(),
            log_to_file: false,
            log_file_path: None,
            enable_metrics: true,
        }
    }
}

impl TelemetryConfig {
    fn merge(&mut self, other: Self) {
        self.enabled = other.enabled;
        self.log_level = other.log_level;
        self.log_to_file = other.log_to_file;
        if other.log_file_path.is_some() {
            self.log_file_path = other.log_file_path;
        }
        self.enable_metrics = other.enable_metrics;
    }
}

/// Environment variable configuration loader
pub struct EnvConfig;

impl EnvConfig {
    /// Load configuration from environment variables
    pub fn load() -> AgentConfig {
        use std::env;
        
        let mut config = AgentConfig::default();
        
        // Session
        if let Ok(val) = env::var("MYLM_MAX_STEPS") {
            if let Ok(steps) = val.parse() {
                config.session.max_steps = steps;
            }
        }
        
        // LLM
        if let Ok(model) = env::var("MYLM_MODEL") {
            config.llm.model = model;
        }
        if let Ok(temp) = env::var("MYLM_TEMPERATURE") {
            if let Ok(t) = temp.parse() {
                config.llm.temperature = t;
            }
        }
        
        // Tools
        if let Ok(val) = env::var("MYLM_REQUIRE_APPROVAL") {
            config.tools.require_approval = val.to_lowercase() == "true";
        }
        
        // Retry
        if let Ok(val) = env::var("MYLM_MAX_RETRIES") {
            if let Ok(retries) = val.parse() {
                config.retry.max_retries = retries;
            }
        }
        
        // Memory
        if let Ok(val) = env::var("MYLM_MEMORY_ENABLED") {
            config.memory.enabled = val.to_lowercase() == "true";
        }
        
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = AgentConfig::default();
        assert_eq!(config.session.max_steps, 50);
        assert!(config.tools.require_approval);
        assert!(config.memory.enabled);
    }
    
    #[test]
    fn test_config_merge() {
        let mut base = AgentConfig::default();
        let other = AgentConfig {
            llm: LlmConfig {
                model: "gpt-4".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        
        base.merge(other);
        assert_eq!(base.llm.model, "gpt-4");
    }
    
    #[test]
    fn test_toml_roundtrip() {
        let config = AgentConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: AgentConfig = toml::from_str(&toml_str).unwrap();
        
        assert_eq!(parsed.session.max_steps, config.session.max_steps);
        assert_eq!(parsed.llm.model, config.llm.model);
    }
    
    #[test]
    fn test_env_config() {
        // Set environment variables
        std::env::set_var("MYLM_MAX_STEPS", "75");
        std::env::set_var("MYLM_MODEL", "gpt-4");
        
        let config = EnvConfig::load();
        assert_eq!(config.session.max_steps, 75);
        assert_eq!(config.llm.model, "gpt-4");
        
        // Clean up
        std::env::remove_var("MYLM_MAX_STEPS");
        std::env::remove_var("MYLM_MODEL");
    }
}
