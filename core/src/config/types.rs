use serde::{Deserialize, Serialize};

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

/// Escalation mode for restricted commands
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum EscalationMode {
    /// Escalate to main agent for approval
    EscalateToMain,
    /// Block restricted commands (no escalation)
    BlockRestricted,
    /// Allow all commands (debugging only - dangerous!)
    AllowAll,
}

impl std::fmt::Display for EscalationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EscalationMode::EscalateToMain => write!(f, "EscalateToMain"),
            EscalationMode::BlockRestricted => write!(f, "BlockRestricted"),
            EscalationMode::AllowAll => write!(f, "AllowAll"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentPermissions {
    /// List of allowed tool names. If None or empty, all tools are allowed.
    pub allowed_tools: Option<Vec<String>>,
    /// List of command patterns (glob) that are auto-approved without confirmation.
    /// Pattern format: "*" matches any characters, "?" matches single char.
    /// Examples: ["ls *", "echo *", "pwd"]
    pub auto_approve_commands: Option<Vec<String>>,
    /// List of command patterns (glob) that are FORBIDDEN unless explicitly confirmed.
    /// These take precedence over auto_approve.
    /// Examples: ["rm -rf *", "dd if=", "mkfs *"]
    pub forbidden_commands: Option<Vec<String>>,
    /// Worker shell execution permissions (for background workers)
    pub worker_shell: Option<WorkerShellConfig>,
}

/// Configuration for worker shell command permissions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WorkerShellConfig {
    /// Commands allowed without escalation (e.g., "ls *", "cat *", "sleep *")
    pub allowed_patterns: Option<Vec<String>>,
    /// Commands requiring escalation to main agent (e.g., "rm *", "curl *")
    pub restricted_patterns: Option<Vec<String>>,
    /// Commands always forbidden (e.g., "sudo *", "rm -rf /")
    pub forbidden_patterns: Option<Vec<String>>,
    /// How to handle restricted commands
    pub escalation_mode: Option<EscalationMode>,
}

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

impl WorkerShellConfig {
    /// Get default allowed patterns (harmless commands)
    pub fn default_allowed() -> Vec<String> {
        vec![
            "ls *".to_string(),
            "cat *".to_string(),
            "head *".to_string(),
            "tail *".to_string(),
            "pwd".to_string(),
            "echo *".to_string(),
            "sleep *".to_string(),
            "date".to_string(),
            "which *".to_string(),
            "git status*".to_string(),
            "git log*".to_string(),
            "git diff*".to_string(),
            "git branch*".to_string(),
            "cargo check*".to_string(),
            "cargo build*".to_string(),
            "cargo test*".to_string(),
        ]
    }
    
    /// Get default restricted patterns (potentially harmful)
    pub fn default_restricted() -> Vec<String> {
        vec![
            "rm *".to_string(),
            "mv *".to_string(),
            "cp *".to_string(),
            "curl *".to_string(),
            "wget *".to_string(),
            "ssh *".to_string(),
            "scp *".to_string(),
            "git push*".to_string(),
            "git reset*".to_string(),
            "npm publish*".to_string(),
        ]
    }
}
