//! LLM Client implementation
//!
//! Supports multiple LLM providers:
//! - OpenAI-compatible API (OpenAI, Ollama, LM Studio, local models)
//! - Google Generative AI (Gemini)

use super::{
    chat::{ChatMessage, ChatRequest, ChatResponse, StreamEvent},
    LlmConfig, TokenUsage,
};
use anyhow::{bail, Context, Result};
use async_stream::stream;
use futures::Stream;
use reqwest::{
    header::{HeaderMap, CONTENT_TYPE},
    Client as HttpClient, StatusCode,
};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// LLM Provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    /// OpenAI-compatible API (works with OpenAI, Ollama, LM Studio, local models)
    OpenAiCompatible,
    /// Google Generative AI (Gemini)
    GoogleGenerativeAi,
}

impl std::str::FromStr for LlmProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" | "ollama" | "lmstudio" | "local" => Ok(LlmProvider::OpenAiCompatible),
            "google" | "gemini" | "google-ai" => Ok(LlmProvider::GoogleGenerativeAi),
            _ => Err(format!("Unknown LLM provider: {}", s)),
        }
    }
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::OpenAiCompatible => write!(f, "OpenAI Compatible"),
            LlmProvider::GoogleGenerativeAi => write!(f, "Google Generative AI"),
        }
    }
}

/// Main LLM Client
pub struct LlmClient {
    config: LlmConfig,
    http_client: HttpClient,
}

impl LlmClient {
    /// Create a new LLM client
    pub fn new(config: LlmConfig) -> Result<Self> {
        let http_client = HttpClient::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .context("Failed to build HTTP client")?;

        Ok(LlmClient {
            config,
            http_client,
        })
    }

    /// Send a chat request and get a response
    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        match self.config.provider {
            LlmProvider::OpenAiCompatible => self.chat_openai(request).await,
            LlmProvider::GoogleGenerativeAi => self.chat_gemini(request).await,
        }
    }

    /// Send a chat request with streaming response
    pub fn chat_stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>> {
        match self.config.provider {
            LlmProvider::OpenAiCompatible => self.chat_stream_openai(request),
            LlmProvider::GoogleGenerativeAi => self.chat_stream_gemini(request),
        }
    }

    /// OpenAI-compatible API chat
    async fn chat_openai(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.config.base_url);

        let body = OpenAiRequest {
            model: self.config.model.clone(),
            messages: &request.messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            stream: Some(false),
        };

        let response = self
            .http_client
            .post(&url)
            .headers(self.build_headers()?)
            .json(&body)
            .send()
            .await
            .context("Failed to send request to OpenAI API")?;

        match response.status() {
            StatusCode::OK => {
                let response_body: OpenAiResponse = response
                    .json()
                    .await
                    .context("Failed to parse OpenAI response")?;
                Ok(ChatResponse {
                    content: response_body
                        .choices
                        .first()
                        .map(|c| c.message.content.clone())
                        .unwrap_or_default(),
                    usage: Some(TokenUsage {
                        prompt_tokens: response_body.usage.prompt_tokens as u32,
                        completion_tokens: response_body.usage.completion_tokens as u32,
                        total_tokens: response_body.usage.total_tokens as u32,
                    }),
                })
            }
            StatusCode::UNAUTHORIZED => {
                bail!("Authentication failed. Check your API key.");
            }
            StatusCode::TOO_MANY_REQUESTS => {
                bail!("Rate limit exceeded. Please try again later.");
            }
            status => {
                let error_body: Option<serde_json::Value> = response.json().await.ok();
                let error_msg = error_body
                    .as_ref()
                    .and_then(|v| v.get("error").and_then(|e| e.get("message")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                bail!("API request failed ({}): {}", status, error_msg);
            }
        }
    }

    /// Google Gemini API chat
    async fn chat_gemini(&self, request: &ChatRequest) -> Result<ChatResponse> {
        // Convert messages to Gemini format
        let contents: Vec<GeminiContent> = request
            .messages
            .iter()
            .map(|m| GeminiContent {
                role: match m.role.as_str() {
                    "user" => "user",
                    "assistant" => "model",
                    "system" => "user", // Gemini doesn't have system role, prepend to user
                    _ => "user",
                }
                .to_string(),
                parts: vec![GeminiPart {
                    text: m.content.clone(),
                }],
            })
            .collect();

        // If there's a system prompt, prepend it to the first user message
        let contents = if let Some(sys_prompt) = &self.config.system_prompt {
            let mut with_system = vec![GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: format!("System: {}\n\nUser: {}", sys_prompt, contents[0].parts[0].text),
                }],
            }];
            with_system.extend_from_slice(&contents[1..]);
            with_system
        } else {
            contents
        };

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.config.base_url,
            self.config.model,
            self.config.api_key.as_ref().unwrap_or(&String::new())
        );

        let body = GeminiRequest {
            contents,
            generation_config: GeminiGenerationConfig {
                max_output_tokens: self.config.max_tokens,
                temperature: self.config.temperature,
            },
        };

        let response = self
            .http_client
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        match response.status() {
            StatusCode::OK => {
                let response_body: GeminiResponse = response
                    .json()
                    .await
                    .context("Failed to parse Gemini response")?;
                let content = response_body
                    .candidates
                    .first()
                    .and_then(|c| c.content.parts.first())
                    .map(|p| p.text.clone())
                    .unwrap_or_default();
                Ok(ChatResponse { content, usage: None })
            }
            StatusCode::UNAUTHORIZED => {
                bail!("Authentication failed. Check your API key.");
            }
            StatusCode::TOO_MANY_REQUESTS => {
                bail!("Rate limit exceeded. Please try again later.");
            }
            status => {
                let error_body: Option<serde_json::Value> = response.json().await.ok();
                let error_msg = error_body
                    .as_ref()
                    .and_then(|v| v.get("error").and_then(|e| e.get("message")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                bail!("Gemini API request failed ({}): {}", status, error_msg);
            }
        }
    }

    /// OpenAI-compatible streaming chat
    fn chat_stream_openai<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>> {
        let url = format!("{}/chat/completions", self.config.base_url);

        let body = OpenAiRequest {
            model: self.config.model.clone(),
            messages: &request.messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            stream: Some(true),
        };

        let http_client = self.http_client.clone();
        let headers = self.build_headers();

        Box::pin(stream(move || {
            async_stream::try_stream! {
                let response = http_client
                    .post(&url)
                    .headers(headers?)
                    .json(&body)
                    .send()
                    .await
                    .context("Failed to send streaming request")?;

                if !response.status().is_success() {
                    bail!("API request failed with status: {}", response.status());
                }

                let mut stream = response.bytes_stream();
                let mut buffer = String::new();
                let mut in_message = false;

                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.context("Failed to read chunk")?;
                    buffer.push_str(&String::from_utf8_lossy(&chunk));

                    // Process complete SSE events
                    loop {
                        if let Some(newline_pos) = buffer.find('\n') {
                            let line = buffer[..newline_pos].to_string();
                            buffer = buffer[newline_pos + 1..].to_string();

                            if line.starts_with("data: ") {
                                let data = &line[6..];
                                if data == "[DONE]" {
                                    yield StreamEvent::Done;
                                    return;
                                }

                                if let Ok(parsed) = serde_json::from_str::<OpenAiStreamResponse>(data) {
                                    if let Some(delta) = parsed.choices.first().and_then(|c| c.delta.content.as_ref()) {
                                        yield StreamEvent::Content(delta.clone());
                                    }
                                    if let Some(usage) = parsed.usage {
                                        yield StreamEvent::Usage(TokenUsage {
                                            prompt_tokens: usage.prompt_tokens as u32,
                                            completion_tokens: usage.completion_tokens as u32,
                                            total_tokens: usage.total_tokens as u32,
                                        });
                                    }
                                }
                            }
                        } else {
                            break;
                        }
                    }
                }

                yield StreamEvent::Done;
            }
        }))
    }

    /// Gemini streaming chat
    fn chat_stream_gemini<'a>(
        &'a self,
        _request: &'a ChatRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>> {
        // Gemini API streaming is more complex, return a simple message
        // For now, fall back to non-streaming
        Box::pin(stream(async move {
            Ok::<_, anyhow::Error>(StreamEvent::Content(
                "Streaming for Gemini is not yet implemented. Use non-streaming mode.".to_string(),
            ))
        }))
    }

    /// Build headers for API requests
    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();

        match self.config.provider {
            LlmProvider::OpenAiCompatible => {
                headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
                if let Some(api_key) = &self.config.api_key {
                    if api_key != "none" && !api_key.is_empty() {
                        headers.insert(
                            "Authorization",
                            format!("Bearer {}", api_key).parse().unwrap(),
                        );
                    }
                }
            }
            LlmProvider::GoogleGenerativeAi => {
                // API key is included in URL, not headers
                headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
            }
        }

        Ok(headers)
    }

    /// Get the model name
    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// Get the provider type
    pub fn provider(&self) -> LlmProvider {
        self.config.provider
    }
}

// OpenAI-compatible API types
#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: String,
    #[serde(borrow)]
    messages: &'a Vec<ChatMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    #[serde(default)]
    stream: Option<bool>,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<OpenAiChoice>,
    usage: OpenAiUsage,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    index: u32,
    message: OpenAiMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Deserialize)]
struct OpenAiStreamResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<OpenAiStreamChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiStreamChoice {
    index: u32,
    delta: OpenAiDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiDelta {
    role: Option<String>,
    content: Option<String>,
}

// Gemini API types
#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
    finish_reason: Option<String>,
    index: u32,
}

#[derive(Deserialize)]
struct GeminiUsageMetadata {
    prompt_token_count: u32,
    candidates_token_count: u32,
    total_token_count: u32,
}
