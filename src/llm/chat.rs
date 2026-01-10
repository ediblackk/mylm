//! Chat message types for LLM communication
//!
//! Defines the message structures used for chat completions,
//! supporting both OpenAI-compatible and Google Gemini APIs.

use serde::{Deserialize, Serialize};

/// Role of the message sender
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// System message (instructions for the model)
    System,
    /// User message
    User,
    /// Assistant message (model response)
    Assistant,
    /// Tool message (result from tool execution)
    Tool,
}

/// A single message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role of the message sender
    pub role: MessageRole,
    /// Content of the message
    pub content: String,
    /// Optional name for the message author
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    /// Create a new user message
    pub fn user(content: impl Into<String>) -> Self {
        ChatMessage {
            role: MessageRole::User,
            content: content.into(),
            name: None,
        }
    }

    /// Create a new system message
    pub fn system(content: impl Into<String>) -> Self {
        ChatMessage {
            role: MessageRole::System,
            content: content.into(),
            name: None,
        }
    }

    /// Create a new assistant message
    pub fn assistant(content: impl Into<String>) -> Self {
        ChatMessage {
            role: MessageRole::Assistant,
            content: content.into(),
            name: None,
        }
    }
}

/// Request body for chat completion
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatRequest {
    /// ID of the model to use
    pub model: String,
    /// List of messages in the conversation
    pub messages: Vec<ChatMessage>,
    /// Maximum tokens to generate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Temperature for sampling (0-2)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Whether to stream the response
    #[serde(default = "default_stream")]
    pub stream: bool,
    /// Optional stop sequences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

fn default_stream() -> bool {
    false
}

impl ChatRequest {
    /// Create a new chat request
    pub fn new(model: String, messages: Vec<ChatMessage>) -> Self {
        ChatRequest {
            model,
            messages,
            max_tokens: None,
            temperature: None,
            stream: false,
            stop: None,
        }
    }

    /// Add a system message
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.messages.insert(0, ChatMessage::system(prompt));
        self
    }

    /// Set max tokens
    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    /// Set temperature
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp.clamp(0.0, 2.0));
        self
    }

    /// Enable streaming
    pub fn with_streaming(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }
}

/// Response from chat completion
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A single completion choice
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    /// Index of this choice
    pub index: u32,
    /// The generated message
    pub message: ChatMessage,
    /// Reason for stopping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Token usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens in the prompt
    pub prompt_tokens: u32,
    /// Tokens in the completion
    pub completion_tokens: u32,
    /// Total tokens used
    pub total_tokens: u32,
}

/// Stream event types for streaming responses
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Content chunk received
    Content(String),
    /// Streaming is complete
    Done,
    /// Token usage information
    Usage(super::TokenUsage),
    /// Error occurred
    Error(String),
}

impl StreamEvent {
    /// Check if this is a content event
    pub fn is_content(&self) -> bool {
        matches!(self, StreamEvent::Content(_))
    }

    /// Check if streaming is done
    pub fn is_done(&self) -> bool {
        matches!(self, StreamEvent::Done)
    }

    /// Get content if available
    pub fn content(&self) -> Option<&str> {
        match self {
            StreamEvent::Content(s) => Some(s),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_creation() {
        let user_msg = ChatMessage::user("Hello");
        assert_eq!(user_msg.role, MessageRole::User);
        assert_eq!(user_msg.content, "Hello");

        let system_msg = ChatMessage::system("You are helpful");
        assert_eq!(system_msg.role, MessageRole::System);
        assert_eq!(system_msg.content, "You are helpful");
    }

    #[test]
    fn test_chat_request_builder() {
        let request = ChatRequest::new("gpt-4".to_string(), vec![])
            .with_system_prompt("Be helpful")
            .with_max_tokens(100)
            .with_temperature(0.7)
            .with_streaming(true);

        assert_eq!(request.model, "gpt-4");
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, MessageRole::System);
        assert_eq!(request.max_tokens, Some(100));
        assert_eq!(request.temperature, Some(0.7));
        assert!(request.stream);
    }

    #[test]
    fn test_temperature_clamping() {
        let request = ChatRequest::new("gpt-4".to_string(), vec![])
            .with_temperature(3.0); // Should be clamped to 2.0

        assert_eq!(request.temperature, Some(2.0));
    }
}
