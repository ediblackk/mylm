//! Profile Configuration
//!
//! User profiles combining provider settings with behavior preferences.

use serde::{Deserialize, Serialize};
use super::SearchProvider;

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
    pub web_search: WebSearchConfig,
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
            web_search: WebSearchConfig::default(),
        }
    }
}

/// Web search configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    /// Enable web search for this profile
    #[serde(default)]
    pub enabled: bool,

    /// Search provider to use
    #[serde(default)]
    pub provider: SearchProvider,

    /// API key for search provider
    #[serde(default)]
    pub api_key: Option<String>,

    /// Number of results to return
    #[serde(default = "default_search_results")]
    pub num_results: usize,

    /// Extra provider-specific configuration options
    #[serde(default)]
    pub extra_params: Option<std::collections::HashMap<String, String>>,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: SearchProvider::default(),
            api_key: None,
            num_results: default_search_results(),
            extra_params: None,
        }
    }
}

/// Resolved profile configuration (effective settings after combining profile + provider)
#[derive(Debug, Clone)]
pub struct ResolvedProfile {
    /// Provider type (if available)
    pub provider: Option<super::ProviderType>,
    /// Model to use (profile override or provider default)
    pub model: Option<String>,
    /// API base URL
    pub base_url: Option<String>,
    /// API key
    pub api_key: Option<String>,
    /// Request timeout
    pub timeout_secs: u64,
    /// Maximum context tokens
    pub max_context_tokens: usize,
    /// Maximum iterations
    pub max_iterations: usize,
    /// Temperature
    pub temperature: f32,
}

impl ResolvedProfile {
    /// Get default URL for provider (used as fallback)
    pub fn default_url(&self) -> String {
        match self.provider {
            Some(super::ProviderType::OpenAi) => "https://api.openai.com/v1".to_string(),
            Some(super::ProviderType::Google) => "https://generativelanguage.googleapis.com/v1beta".to_string(),
            Some(super::ProviderType::Ollama) => "http://localhost:11434/v1".to_string(),
            Some(super::ProviderType::OpenRouter) => "https://openrouter.ai/api/v1".to_string(),
            Some(super::ProviderType::Kimi) => "https://api.moonshot.cn/v1".to_string(),
            Some(super::ProviderType::InceptionLabs) => "https://api.inceptionlabs.ai/v1".to_string(),
            Some(super::ProviderType::Custom) | None => "https://api.openai.com/v1".to_string(),
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

fn default_search_results() -> usize {
    5
}
