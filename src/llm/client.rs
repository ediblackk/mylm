//! LLM Client implementation
//!
//! Supports multiple LLM providers:
//! - OpenAI-compatible API (OpenAI, Ollama, LM Studio, local models)
//! - Google Generative AI (Gemini)

use super::{
    chat::{ChatMessage, ChatRequest, ChatResponse, Choice, StreamEvent, Usage},
    LlmConfig, TokenUsage,
};
use anyhow::{bail, Context, Result};
use futures::{Stream, StreamExt};
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
    /// Moonshot AI (Kimi)
    MoonshotKimi,
}

impl std::str::FromStr for LlmProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" | "ollama" | "lmstudio" | "local" | "openrouter" => Ok(LlmProvider::OpenAiCompatible),
            "google" | "gemini" | "google-ai" | "google-generativeai" => Ok(LlmProvider::GoogleGenerativeAi),
            "moonshot" | "kimi" => Ok(LlmProvider::MoonshotKimi),
            _ => Err(format!("Unknown LLM provider: {}", s)),
        }
    }
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::OpenAiCompatible => write!(f, "OpenAI Compatible"),
            LlmProvider::GoogleGenerativeAi => write!(f, "Google Generative AI"),
            LlmProvider::MoonshotKimi => write!(f, "Moonshot AI (Kimi)"),
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
            LlmProvider::OpenAiCompatible | LlmProvider::MoonshotKimi => self.chat_openai(request).await,
            LlmProvider::GoogleGenerativeAi => self.chat_gemini(request).await,
        }
    }

    /// Helper for main.rs and others
    pub async fn complete(&self, prompt: &str) -> Result<ChatResponse> {
        let request = ChatRequest::new(self.config.model.clone(), vec![ChatMessage::user(prompt)]);
        self.chat(&request).await
    }

    /// Send a chat request with streaming response
    #[allow(dead_code)]
    pub fn chat_stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>> {
        match self.config.provider {
            LlmProvider::OpenAiCompatible | LlmProvider::MoonshotKimi => self.chat_stream_openai(request),
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
            tools: request.tools.as_ref().map(|t| t.iter().map(|tool| OpenAiTool {
                type_: tool.type_.clone(),
                function: OpenAiFunction {
                    name: tool.function.name.clone(),
                    description: tool.function.description.clone(),
                    parameters: tool.function.parameters.clone(),
                },
            }).collect()),
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
                
                let choices = response_body.choices.into_iter().map(|c| Choice {
                    index: c.index,
                    message: ChatMessage {
                        role: super::chat::MessageRole::Assistant,
                        content: c.message.content,
                        name: None,
                        tool_call_id: None,
                        tool_calls: c.message.tool_calls.as_ref().map(|tcs| tcs.iter().map(|tc| crate::llm::chat::ToolCall {
                            id: tc.id.clone(),
                            type_: tc.type_.clone(),
                            function: crate::llm::chat::ToolCallFunction {
                                name: tc.function.name.clone(),
                                arguments: tc.function.arguments.clone(),
                            },
                        }).collect()),
                    },
                    finish_reason: c.finish_reason,
                }).collect();

                Ok(ChatResponse {
                    id: response_body.id,
                    object: response_body.object,
                    created: response_body.created,
                    model: response_body.model,
                    choices,
                    usage: Some(Usage {
                        prompt_tokens: response_body.usage.prompt_tokens,
                        completion_tokens: response_body.usage.completion_tokens,
                        total_tokens: response_body.usage.total_tokens,
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
        // Convert messages to Gemini format, merging consecutive messages of the same role
        let mut contents: Vec<GeminiContent> = Vec::new();
        let mut system_parts = Vec::new();

        for m in &request.messages {
            let content = m.content.trim();
            if content.is_empty() && m.tool_calls.is_none() {
                continue; // Skip empty messages that don't have tool calls
            }

            match m.role.as_str() {
                "system" => {
                    system_parts.push(GeminiPart { text: m.content.clone() });
                }
                _ => {
                    let role = match m.role.as_str() {
                        "assistant" => "model",
                        "user" | "tool" => "user",
                        _ => "user",
                    };

                    if let Some(last) = contents.last_mut() {
                        if last.role == role {
                            // Merge text parts into one to avoid "multiple parts" issues on some proxies
                            if let Some(first_part) = last.parts.first_mut() {
                                first_part.text.push_str("\n\n");
                                first_part.text.push_str(&m.content);
                            } else {
                                last.parts.push(GeminiPart { text: m.content.clone() });
                            }
                            continue;
                        }
                    }
                    
                    contents.push(GeminiContent {
                        role: role.to_string(),
                        parts: vec![GeminiPart { text: m.content.clone() }],
                    });
                }
            }
        }

        // Gemini API Requirement: contents must start with a "user" role.
        // If pruning or history manipulation left us starting with "model", drop it.
        while !contents.is_empty() && contents[0].role != "user" {
            contents.remove(0);
        }

        // Add system prompt from config if present
        if let Some(sys_prompt) = &self.config.system_prompt {
            system_parts.insert(0, GeminiPart { text: sys_prompt.clone() });
        }

        let system_instruction_text = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n\n"))
        };

        // For Gemini, we also prepend the system instruction to the first user message
        // as some models/proxies ignore system_instruction.
        if let (Some(sys_text), Some(first_msg)) = (&system_instruction_text, contents.first_mut()) {
            if first_msg.role == "user" {
                let original_text = first_msg.parts[0].text.clone();
                first_msg.parts[0].text = format!("SYSTEM INSTRUCTIONS:\n{}\n\nUSER MESSAGE:\n{}", sys_text, original_text);
            }
        }

        let system_instruction = system_instruction_text.map(|text| GeminiContent {
            role: "system".to_string(),
            parts: vec![GeminiPart { text }],
        });

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.config.base_url,
            self.config.model,
            self.config.api_key.as_ref().unwrap_or(&String::new())
        );

        let body = GeminiRequest {
            contents,
            system_instruction,
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: self.config.max_tokens,
                temperature: self.config.temperature,
            }),
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
                let text = response.text().await.context("Failed to read Gemini response text")?;
                let response_body: GeminiResponse = match serde_json::from_str(&text) {
                    Ok(body) => body,
                    Err(e) => {
                        bail!("Failed to parse Gemini response: {}. Response body: {}", e, text);
                    }
                };
                
                let choices = response_body.candidates.into_iter().map(|c| Choice {
                    index: c.index,
                    message: ChatMessage {
                        role: super::chat::MessageRole::Assistant,
                        content: c.content.parts.first().map(|p| p.text.clone()).unwrap_or_default(),
                        name: None,
                        tool_call_id: None,
                        tool_calls: None,
                    },
                    finish_reason: c.finish_reason,
                }).collect();

                Ok(ChatResponse {
                    id: "gemini".to_string(),
                    object: "chat.completion".to_string(),
                    created: 0,
                    model: self.config.model.clone(),
                    choices,
                    usage: response_body.usage_metadata.map(|u| Usage {
                        prompt_tokens: u.prompt_token_count,
                        completion_tokens: u.candidates_token_count,
                        total_tokens: u.total_token_count,
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
                bail!("Gemini API request failed ({}): {}", status, error_msg);
            }
        }
    }

    /// OpenAI-compatible streaming chat
    #[allow(dead_code)]
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
            tools: request.tools.as_ref().map(|t| t.iter().map(|tool| OpenAiTool {
                type_: tool.type_.clone(),
                function: OpenAiFunction {
                    name: tool.function.name.clone(),
                    description: tool.function.description.clone(),
                    parameters: tool.function.parameters.clone(),
                },
            }).collect()),
        };

        let http_client = self.http_client.clone();
        let headers_res = self.build_headers();

        Box::pin(async_stream::try_stream! {
            let headers = headers_res?;
            let response = http_client
                .post(&url)
                .headers(headers)
                .json(&body)
                .send()
                .await
                .context("Failed to send streaming request")?;

            if !response.status().is_success() {
                let status = response.status();
                Err(anyhow::anyhow!("API request failed with status: {}", status))?;
            }

            let mut stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_res) = stream.next().await {
                let chunk = chunk_res.context("Failed to read chunk")?;
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
                                        prompt_tokens: usage.prompt_tokens,
                                        completion_tokens: usage.completion_tokens,
                                        total_tokens: usage.total_tokens,
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
        })
    }

    /// Gemini streaming chat
    #[allow(dead_code)]
    fn chat_stream_gemini<'a>(
        &'a self,
        _request: &'a ChatRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>> {
        // Gemini API streaming is more complex, return a simple message
        Box::pin(async_stream::try_stream! {
            yield StreamEvent::Content(
                "Streaming for Gemini is not yet implemented. Use non-streaming mode.".to_string(),
            );
        })
    }

    /// Build headers for API requests
    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();

        match self.config.provider {
            LlmProvider::OpenAiCompatible | LlmProvider::MoonshotKimi => {
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
    #[allow(dead_code)]
    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// Get the provider type
    #[allow(dead_code)]
    pub fn provider(&self) -> LlmProvider {
        self.config.provider
    }

    /// Get the configuration
    pub fn config(&self) -> &LlmConfig {
        &self.config
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
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
}

#[derive(Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    type_: String,
    function: OpenAiFunction,
}

#[derive(Serialize)]
struct OpenAiFunction {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<OpenAiChoice>,
    usage: OpenAiUsage,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiChoice {
    index: u32,
    message: OpenAiMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[allow(dead_code)]
struct OpenAiMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiResponseToolCall>>,
}

#[derive(Deserialize, Serialize)]
struct OpenAiResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    type_: String,
    function: OpenAiResponseToolFunction,
}

#[derive(Deserialize, Serialize)]
struct OpenAiResponseToolFunction {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiStreamResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<OpenAiStreamChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiStreamChoice {
    index: u32,
    delta: OpenAiDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAiDelta {
    role: Option<String>,
    content: Option<String>,
}

// Gemini API types
#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize, Clone)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    content: GeminiContent,
    finish_reason: Option<String>,
    index: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    prompt_token_count: u32,
    candidates_token_count: u32,
    total_token_count: u32,
}
