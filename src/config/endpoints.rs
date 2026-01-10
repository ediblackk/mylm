//! LLM endpoint configuration
//!
//! Defines the structure for OpenAI-compatible API endpoints,
//! supporting various providers like Ollama, LM Studio, and OpenAI.

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

impl EndpointConfig {
    /// Get the API key, falling back to environment variable
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
    pub fn get_base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the model identifier
    pub fn get_model(&self) -> &str {
        &self.model
    }

    /// Check if this endpoint requires authentication
    pub fn requires_auth(&self) -> bool {
        !self.api_key.is_empty() && self.api_key != "none"
    }

    /// Create default Ollama configuration
    pub fn ollama_default() -> Self {
        Self {
            name: "ollama".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            model: "llama3.2".to_string(),
            api_key: "none".to_string(),
            timeout_seconds: 120,
        }
    }

    /// Create default OpenAI configuration
    pub fn openai_default() -> Self {
        Self {
            name: "openai".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o".to_string(),
            api_key: String::new(),
            timeout_seconds: 60,
        }
    }
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self::ollama_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_config_defaults() {
        let endpoint = EndpointConfig::default();
        assert_eq!(endpoint.name, "ollama");
        assert!(endpoint.base_url.contains("localhost"));
        assert_eq!(endpoint.timeout_seconds, 120);
    }

    #[test]
    fn test_get_api_key_from_config() {
        let endpoint = EndpointConfig {
            name: "test".to_string(),
            base_url: "http://localhost:8080/v1".to_string(),
            model: "test-model".to_string(),
            api_key: "test-key-123".to_string(),
            timeout_seconds: 30,
        };

        assert_eq!(endpoint.get_api_key(), "test-key-123");
    }

    #[test]
    fn test_get_api_key_env_fallback() {
        std::env::set_var("OPENAI_API_KEY", "env-key-456");

        let endpoint = EndpointConfig {
            name: "test".to_string(),
            base_url: "http://localhost:8080/v1".to_string(),
            model: "test-model".to_string(),
            api_key: "none".to_string(),
            timeout_seconds: 30,
        };

        assert_eq!(endpoint.get_api_key(), "env-key-456");

        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn test_requires_auth() {
        let with_auth = EndpointConfig {
            name: "test".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            model: "model".to_string(),
            api_key: "some-key".to_string(),
            timeout_seconds: 30,
        };

        let without_auth = EndpointConfig {
            name: "test".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            model: "model".to_string(),
            api_key: "none".to_string(),
            timeout_seconds: 30,
        };

        assert!(with_auth.requires_auth());
        assert!(!without_auth.requires_auth());
    }

    #[test]
    fn test_chat_request_serialization() {
        let request = ChatRequest {
            model: "llama3.2".to_string(),
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: "You are a helpful assistant.".to_string(),
                    name: None,
                },
                Message {
                    role: MessageRole::User,
                    content: "Hello!".to_string(),
                    name: None,
                },
            ],
            max_tokens: Some(100),
            temperature: Some(0.7),
            stream: false,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("llama3.2"));
        assert!(json.contains("system"));
        assert!(json.contains("user"));
    }

    #[test]
    fn test_chat_response_deserialization() {
        let json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1699000000,
            "model": "llama3.2",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello! How can I help you?"
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.content, "Hello! How can I help you?");
    }
}
