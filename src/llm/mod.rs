//! LLM client module
//!
//! Provides interfaces for communicating with various LLM providers:
//! - OpenAI-compatible API (OpenAI, Ollama, LM Studio, local models)
//! - Google Generative AI (Gemini)

pub mod client;
pub mod chat;

pub use client::{LlmClient, LlmProvider};
pub use chat::{ChatMessage, ChatRequest, ChatResponse, MessageRole, StreamEvent, TokenUsage, Usage};
pub use chat::{ChatMessage, ChatRequest, ChatResponse, StreamEvent};

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
    /// Additional provider-specific parameters
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
            extra_params: HashMap::new(),
        }
    }

    /// Set maximum tokens
    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    /// Set temperature
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp.clamp(0.0, 2.0));
        self
    }

    /// Set system prompt
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }
}

/// Token usage information
#[derive(Debug, Clone, Default)]
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
pub fn create_client(config: LlmConfig) -> Result<LlmClient> {
    LlmClient::new(config).context("Failed to create LLM client")
}

/// Get the system prompt for terminal AI assistant
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
