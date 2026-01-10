//! LLM client module
//!
//! Provides interfaces for communicating with various LLM providers:
//! - OpenAI-compatible API (OpenAI, Ollama, LM Studio, local models)
//! - Google Generative AI (Gemini)

pub mod client;
pub mod chat;

pub use client::{LlmClient, LlmProvider};
pub use chat::ChatResponse;

use anyhow::{Context, Result};
use std::collections::HashMap;

/// LLM Configuration
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// Provider type
    pub provider: LlmProvider,
    /// API endpoint base URL
    pub base_url: String,
    /// Model identifier
    pub model: String,
    /// API key (if required)
    pub api_key: Option<String>,
    /// Maximum tokens in response
    pub max_tokens: Option<u32>,
    /// Temperature for sampling (0.0 - 2.0)
    pub temperature: Option<f32>,
    /// System prompt for the model
    pub system_prompt: Option<String>,
    /// Cost per 1000 input tokens
    pub input_price_per_1k: f64,
    /// Cost per 1000 output tokens
    pub output_price_per_1k: f64,
    /// Maximum context tokens
    pub max_context_tokens: usize,
    /// Condense threshold (0.0 - 1.0)
    pub condense_threshold: f64,
    /// Additional provider-specific parameters
    #[allow(dead_code)]
    pub extra_params: HashMap<String, String>,
}

impl LlmConfig {
    /// Create a new LLM config
    pub fn new(
        provider: LlmProvider,
        base_url: String,
        model: String,
        api_key: Option<String>,
    ) -> Self {
        LlmConfig {
            provider,
            base_url,
            model,
            api_key,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            system_prompt: None,
            input_price_per_1k: 0.0,
            output_price_per_1k: 0.0,
            max_context_tokens: 32768,
            condense_threshold: 0.8,
            extra_params: HashMap::new(),
        }
    }

    /// Set pricing
    pub fn with_pricing(mut self, input_1k: f64, output_1k: f64) -> Self {
        self.input_price_per_1k = input_1k;
        self.output_price_per_1k = output_1k;
        self
    }

    /// Set context management settings
    pub fn with_context_management(mut self, max_tokens: usize, threshold: f64) -> Self {
        self.max_context_tokens = max_tokens;
        self.condense_threshold = threshold;
        self
    }

    /// Set maximum tokens
    #[allow(dead_code)]
    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    /// Set temperature
    #[allow(dead_code)]
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp.clamp(0.0, 2.0));
        self
    }

    /// Set system prompt
    #[allow(dead_code)]
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }
}

/// Token usage information
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Convert TokenUsage to a display string
impl std::fmt::Display for TokenUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Tokens: {} (prompt: {}, completion: {})",
            self.total_tokens, self.prompt_tokens, self.completion_tokens
        )
    }
}

/// Create a default LLM client with the given configuration
#[allow(dead_code)]
pub fn create_client(config: LlmConfig) -> Result<LlmClient> {
    LlmClient::new(config).context("Failed to create LLM client")
}

/// Get the system prompt for terminal AI assistant
#[allow(dead_code)]
pub fn get_terminal_system_prompt() -> String {
    String::from(
        "You are a helpful AI assistant that helps with terminal operations and system administration. \
        You have access to terminal context information and can help users with:\n\
        - Analyzing system status and processes\n\
        - Explaining git changes and repository status\n\
        - Troubleshooting system issues\n\
        - Suggesting and executing safe terminal commands\n\
        - Providing information about files and directories\n\n\
        When suggesting commands:\n\
        - Prefer safe, read-only commands when possible\n\
        - Explain what each command will do before execution\n\
        - For potentially dangerous commands, explain the risks\n\
        - Use --dry-run flag when available\n\n\
        Always be concise but thorough. Use code blocks for command examples.",
    )
}
