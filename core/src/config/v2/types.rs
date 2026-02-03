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
