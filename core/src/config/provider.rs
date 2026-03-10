//! Provider Configuration
//!
//! LLM provider settings and configuration.

use serde::{Deserialize, Serialize};

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

    /// Create Google Gemini provider config
    pub fn gemini(api_key: String) -> Self {
        Self {
            provider_type: ProviderType::Google,
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            api_key: Some(api_key),
            default_model: "gemini-1.5-flash".to_string(),
            models: vec![
                "gemini-1.5-flash".to_string(),
                "gemini-1.5-flash-8b".to_string(),
                "gemini-1.5-pro".to_string(),
            ],
            timeout_secs: default_timeout(),
        }
    }

    /// Create Inception Labs provider config
    pub fn inception(api_key: String) -> Self {
        Self {
            provider_type: ProviderType::InceptionLabs,
            base_url: "https://api.inceptionlabs.ai/v1".to_string(),
            api_key: Some(api_key),
            default_model: "mercury-2".to_string(),
            models: vec![
                "mercury-2".to_string(),
            ],
            timeout_secs: default_timeout(),
        }
    }
}

/// Provider type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    OpenAi,
    Google,
    Ollama,
    OpenRouter,
    Kimi,
    InceptionLabs,
    Custom,
}

fn default_timeout() -> u64 {
    120
}
