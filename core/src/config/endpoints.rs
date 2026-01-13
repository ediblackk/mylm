//! LLM endpoint configuration
//!
//! Defines the structure for OpenAI-compatible API endpoints,
//! supporting various providers like Ollama, LM Studio, and OpenAI.

#![allow(dead_code)]
use serde::{Deserialize, Serialize};
use std::env;

/// Configuration for an LLM endpoint
///
/// Supports OpenAI-compatible API format used by:
/// - OpenAI
/// - Ollama
/// - LM Studio
/// - Local models
/// - Any OpenAI-compatible proxy
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EndpointConfig {
    /// Unique name for this endpoint (used for selection)
    pub name: String,

    /// Provider type (openai, google, etc.)
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Base URL of the API endpoint (including /v1 suffix for OpenAI-compatible)
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Model identifier to use
    #[serde(default = "default_model")]
    pub model: String,

    /// API key for authentication
    ///
    /// Use "none" or empty string for local models that don't require auth.
    /// Can also be set via OPENAI_API_KEY environment variable.
    #[serde(default = "default_api_key")]
    pub api_key: String,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,

    /// Cost per 1000 input tokens (in USD)
    #[serde(default = "default_price")]
    pub input_price_per_1k: f64,

    /// Cost per 1000 output tokens (in USD)
    #[serde(default = "default_price")]
    pub output_price_per_1k: f64,

    /// Maximum tokens for the context window
    #[serde(default = "default_max_context")]
    pub max_context_tokens: usize,

    /// Threshold to trigger condensation (0.0 to 1.0)
    #[serde(default = "default_condense_threshold")]
    pub condense_threshold: f64,
}

/// Message role for chat completion
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A single message in a conversation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    /// Role of the message sender
    pub role: MessageRole,

    /// Content of the message
    pub content: String,

    /// Optional name for the message author
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Request body for chat completion
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatRequest {
    /// ID of the model to use
    pub model: String,

    /// List of messages in the conversation
    pub messages: Vec<Message>,

    /// Maximum tokens to generate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Temperature for sampling (0-2)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Whether to stream the response
    #[serde(default = "default_stream")]
    pub stream: bool,
}

/// Response from chat completion
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    /// Unique identifier for the response
    pub id: String,

    /// Type of response (e.g., "chat.completion")
    pub object: String,

    /// Unix timestamp of creation
    pub created: u64,

    /// Model that generated the response
    pub model: String,

    /// List of generated completions
    pub choices: Vec<Choice>,

    /// Usage statistics
    pub usage: Option<Usage>,
}

/// A single completion choice
#[derive(Debug, Serialize, Deserialize)]
pub struct Choice {
    /// Index of this choice
    pub index: u32,

    /// The generated message
    pub message: Message,

    /// Reason for stopping
    pub finish_reason: Option<String>,
}

/// Token usage statistics
#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens in the prompt
    pub prompt_tokens: u32,

    /// Tokens in the completion
    pub completion_tokens: u32,

    /// Total tokens used
    pub total_tokens: u32,
}

// Default value functions
fn default_provider() -> String {
    "openai".to_string()
}

fn default_base_url() -> String {
    "http://localhost:11434/v1".to_string()
}

fn default_model() -> String {
    "llama3.2".to_string()
}

fn default_api_key() -> String {
    "none".to_string()
}

fn default_timeout() -> u64 {
    60
}

fn default_stream() -> bool {
    false
}

fn default_price() -> f64 {
    0.0
}

fn default_max_context() -> usize {
    32768
}

fn default_condense_threshold() -> f64 {
    0.8
}

impl EndpointConfig {
    /// Get the API key, falling back to environment variable
    #[allow(dead_code)]
    pub fn get_api_key(&self) -> String {
        if self.api_key != "none" && !self.api_key.is_empty() {
            return self.api_key.clone();
        }

        // Check environment variable
        if let Ok(key) = env::var("OPENAI_API_KEY") {
            return key;
        }

        if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
            return key;
        }

        // Return empty for local models
        String::new()
    }

    /// Get the base URL for API requests
    #[allow(dead_code)]
    pub fn get_base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the model identifier
    #[allow(dead_code)]
    pub fn get_model(&self) -> &str {
        &self.model
    }

    /// Check if this endpoint requires authentication
    #[allow(dead_code)]
    pub fn requires_auth(&self) -> bool {
        !self.api_key.is_empty() && self.api_key != "none"
    }

    /// Create default Ollama configuration
    pub fn ollama_default() -> Self {
        Self {
            name: "ollama".to_string(),
            provider: "openai".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            model: "llama3.2".to_string(),
            api_key: "none".to_string(),
            timeout_seconds: 120,
            input_price_per_1k: 0.0,
            output_price_per_1k: 0.0,
            max_context_tokens: 32768,
            condense_threshold: 0.8,
        }
    }

    /// Create default OpenAI configuration
    #[allow(dead_code)]
    pub fn openai_default() -> Self {
        Self {
            name: "openai".to_string(),
            provider: "openai".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o".to_string(),
            api_key: String::new(),
            timeout_seconds: 60,
            input_price_per_1k: 0.0025, // GPT-4o input
            output_price_per_1k: 0.01,   // GPT-4o output
            max_context_tokens: 128000,
            condense_threshold: 0.8,
        }
    }
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self::ollama_default()
    }
}

