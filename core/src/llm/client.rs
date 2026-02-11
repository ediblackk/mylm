//! LLM Client implementation
//!
//! Supports multiple LLM providers:
//! - OpenAI-compatible API (OpenAI, Ollama, LM Studio, local models)
//! - Google Generative AI (Gemini)

use super::{
    chat::{ChatMessage, ChatRequest, ChatResponse, Choice, StreamEvent, Usage},
    LlmConfig, TokenUsage,
};
use super::super::util::{sanitize_base_url, validate_api_key};
use super::super::config::ConfigManager;
use super::super::rate_limiter::RateLimiter;
use anyhow::{bail, Context, Result};
use futures::{Stream, StreamExt};
use reqwest::{
    header::{HeaderMap, CONTENT_TYPE},
    Client as HttpClient, StatusCode,
};
use serde::{Deserialize, Serialize};
use parking_lot::Mutex;
use std::pin::Pin;
use std::sync::Arc;
use rand::Rng;
use tokio::time::{sleep, Duration};

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
            "openai" | "ollama" | "lmstudio" | "local" | "openrouter" | "custom" => Ok(LlmProvider::OpenAiCompatible),
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
    config_manager: Option<Arc<ConfigManager>>,
    /// Optional callback for status updates (e.g., retry messages)
    /// Uses Mutex to allow setting after Arc wrapping
    status_callback: Mutex<Option<crate::llm::StatusCallback>>,
    /// Rate limiter for this client
    rate_limiter: Option<Arc<RateLimiter>>,
    /// Whether this is a worker client (uses worker rate limits)
    is_worker: bool,
    /// Optional job ID for tracking metrics
    job_id: Mutex<Option<String>>,
    /// Cancellation token for aborting retries
    cancel_token: Mutex<Option<tokio_util::sync::CancellationToken>>,
    /// Optional job registry for updating metrics
    job_registry: Mutex<Option<crate::agent::v2::jobs::JobRegistry>>,
}

impl LlmClient {
    /// Create a new LLM client
    pub fn new(config: LlmConfig) -> Result<Self> {
        let http_client = HttpClient::builder()
            .timeout(std::time::Duration::from_secs(300))
            .user_agent("mylm/1.0")
            .build()
            .context("Failed to build HTTP client")?;

        Ok(LlmClient {
            config,
            http_client,
            config_manager: None,
            status_callback: Mutex::new(None),
            rate_limiter: None,
            is_worker: false,
            job_id: Mutex::new(None),
            cancel_token: Mutex::new(None),
            job_registry: Mutex::new(None),
        })
    }

    /// Set the config manager for rate limiting
    pub fn with_config_manager(mut self, config_manager: Arc<ConfigManager>) -> Self {
        self.config_manager = Some(config_manager);
        self
    }

    /// Set a status callback for reporting retry attempts and other status updates
    pub fn with_status_callback(self, callback: crate::llm::StatusCallback) -> Self {
        *self.status_callback.lock() = Some(callback);
        self
    }

    /// Set a status callback after the client has been created (for use with Arc<LlmClient>)
    pub fn set_status_callback(&self, callback: crate::llm::StatusCallback) {
        *self.status_callback.lock() = Some(callback);
    }

    /// Set the rate limiter for this client
    pub fn with_rate_limiter(mut self, rate_limiter: Arc<RateLimiter>) -> Self {
        self.rate_limiter = Some(rate_limiter);
        self
    }

    /// Set whether this is a worker client
    pub fn set_worker(mut self, is_worker: bool) -> Self {
        self.is_worker = is_worker;
        self
    }

    /// Set the job ID for tracking metrics
    pub fn set_job_id(&self, job_id: Option<String>) {
        *self.job_id.lock() = job_id;
    }

    /// Get the job ID if set
    pub fn get_job_id(&self) -> Option<String> {
        self.job_id.lock().clone()
    }

    /// Set the job registry for updating metrics
    pub fn set_job_registry(&self, job_registry: crate::agent::v2::jobs::JobRegistry) {
        *self.job_registry.lock() = Some(job_registry);
    }

    /// Set the cancellation token for this client
    pub fn set_cancel_token(&self, token: tokio_util::sync::CancellationToken) {
        *self.cancel_token.lock() = Some(token);
    }

    /// Check if the operation has been cancelled
    fn is_cancelled(&self) -> bool {
        if let Some(token) = self.cancel_token.lock().as_ref() {
            return token.is_cancelled();
        }
        false
    }

    /// Report a status update through the callback if one is set
    fn report_status(&self, message: &str) {
        if let Some(callback) = self.status_callback.lock().as_ref() {
            callback(message);
        }
    }

    /// Check rate limit before making a request (legacy config manager method)
    async fn check_rate_limit_legacy(&self, estimated_tokens: usize) -> Result<()> {
        if let Some(cm) = &self.config_manager {
            if let Err(e) = cm.check_rate_limit(estimated_tokens).await {
                bail!("Rate limit exceeded: retry after {:?}", e.retry_after);
            }
        }
        Ok(())
    }

    /// Check rate limit using the new rate limiter
    async fn check_rate_limit(&self, estimated_tokens: usize) -> Result<()> {
        // First check legacy rate limit
        self.check_rate_limit_legacy(estimated_tokens).await?;

        // Then check new rate limiter if available
        if let Some(ref limiter) = self.rate_limiter {
            let base_url = &self.config.base_url;
            match limiter.acquire(base_url, self.is_worker, estimated_tokens as u32).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    bail!("Rate limit exceeded: {}", e);
                }
            }
        } else {
            Ok(())
        }
    }

    /// Update job metrics after a successful request
    fn update_job_metrics(&self, prompt_tokens: u32, completion_tokens: u32, _estimated_input_tokens: usize) {
        if let Some(ref job_id) = *self.job_id.lock() {
            // Log the metrics
            crate::info_log!("Job {}: tokens used - prompt: {}, completion: {}",
                job_id, prompt_tokens, completion_tokens);
            
            // Debug: Check for suspicious values
            if prompt_tokens > 1000000 || completion_tokens > 1000000 {
                crate::error_log!("[METRICS BUG] Suspicious token values! job={}, prompt={}, completion={}",
                    job_id, prompt_tokens, completion_tokens);
            }
            
            // Update the job registry if available
            let registry_guard = self.job_registry.lock();
            if let Some(ref registry) = *registry_guard {
                // Calculate total context tokens (accumulated prompt + completion tokens)
                // This gives us the actual context window usage
                let (old_total, total_tokens) = registry.list_all_jobs()
                    .iter()
                    .find(|j| &j.id == job_id)
                    .map(|j| {
                        let old = j.metrics.total_tokens;
                        let new = old + prompt_tokens + completion_tokens;
                        (old, new)
                    })
                    .unwrap_or((0, prompt_tokens + completion_tokens));
                
                crate::info_log!("[METRICS CALC] job={}, old_total={}, new_prompt={}, new_completion={}, calculated_total={}",
                    &job_id[..8.min(job_id.len())], old_total, prompt_tokens, completion_tokens, total_tokens);
                
                let max_context = self.config.max_context_tokens;
                registry.update_metrics(job_id, prompt_tokens, completion_tokens, total_tokens as usize, max_context);
                
                // Publish metrics update event for real-time UI updates
                if let Some(event_bus) = registry.get_event_bus() {
                    let (current_prompt, current_completion, current_total) = registry.list_all_jobs()
                        .iter()
                        .find(|j| &j.id == job_id)
                        .map(|j| (j.metrics.prompt_tokens, j.metrics.completion_tokens, j.metrics.total_tokens))
                        .unwrap_or((prompt_tokens, completion_tokens, prompt_tokens + completion_tokens));
                    
                    event_bus.publish(crate::agent::event_bus::CoreEvent::WorkerMetricsUpdate {
                        job_id: job_id.clone(),
                        prompt_tokens: current_prompt,
                        completion_tokens: current_completion,
                        total_tokens: current_total,
                        context_tokens: total_tokens as usize,
                    });
                }
            }
        }
    }

    /// Send a chat request and get a response
    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let agent_type = if self.is_worker { "WORKER" } else { "MAIN" };
        let job_info = self.job_id.lock().as_ref().map(|j| format!("job={}", &j[..8.min(j.len())])).unwrap_or_default();
        
        // Estimate input tokens (rough approximation: 3 chars per token - more conservative)
        let estimated_input_tokens: usize = request.messages.iter()
            .map(|m| m.content.len() / 3 + 1)
            .sum();
        
        crate::info_log!("[{}] {} Chat request: model={}, messages={}, estimated_tokens={}",
            agent_type, job_info, self.config.model, request.messages.len(), estimated_input_tokens);

        // Pre-flight context size check
        let max_context = self.config.max_context_tokens;
        let threshold = (max_context as f64 * 0.8) as usize; // 80% threshold
        
        if estimated_input_tokens > threshold {
            // Log context to file for debugging
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let filename = format!("/tmp/mylm_context_bloat_{}_{}.txt", agent_type.to_lowercase(), timestamp);
            
            let mut content = format!("Context Bloat Debug Log\n");
            content.push_str(&format!("========================\n"));
            content.push_str(&format!("Agent Type: {}\n", agent_type));
            content.push_str(&format!("Job Info: {}\n", job_info));
            content.push_str(&format!("Model: {}\n", self.config.model));
            content.push_str(&format!("Max Context: {}\n", max_context));
            content.push_str(&format!("Threshold (80%): {}\n", threshold));
            content.push_str(&format!("Estimated Tokens: {}\n", estimated_input_tokens));
            content.push_str(&format!("Exceeds Threshold: {}\n", estimated_input_tokens > threshold));
            content.push_str(&format!("Message Count: {}\n\n", request.messages.len()));
            content.push_str(&format!("MESSAGES:\n"));
            content.push_str(&format!("=========\n\n"));
            
            for (i, msg) in request.messages.iter().enumerate() {
                content.push_str(&format!("--- Message {} ---\n", i));
                content.push_str(&format!("Role: {:?}\n", msg.role));
                content.push_str(&format!("Content Length: {} chars\n", msg.content.len()));
                content.push_str(&format!("Content Preview (first 500 chars):\n{}", &msg.content[..msg.content.len().min(500)]));
                if msg.content.len() > 500 {
                    content.push_str("\n... (truncated)");
                }
                content.push_str("\n\n");
            }
            
            // Write to file
            if let Err(e) = std::fs::write(&filename, content) {
                crate::error_log!("[{}] {} Failed to write context debug file: {}", agent_type, job_info, e);
            } else {
                crate::error_log!("[{}] {} Context bloat detected! Debug log written to: {}", 
                    agent_type, job_info, filename);
            }
            
            // If exceeds max_context, return error immediately
            if estimated_input_tokens > max_context {
                return Err(anyhow::anyhow!(
                    "Context limit exceeded: estimated {} tokens > max {} tokens. Debug log: {}",
                    estimated_input_tokens, max_context, filename
                ));
            }
        }

        // Check rate limit before making request
        let rate_limit_start = std::time::Instant::now();
        if let Err(e) = self.check_rate_limit(estimated_input_tokens).await {
            crate::error_log!("[{}] {} Rate limit check failed after {:?}: {}", 
                agent_type, job_info, rate_limit_start.elapsed(), e);
            return Err(e);
        }
        let rate_limit_duration = rate_limit_start.elapsed();
        if rate_limit_duration > std::time::Duration::from_millis(100) {
            crate::warn_log!("[{}] {} Rate limit wait took {:?}", 
                agent_type, job_info, rate_limit_duration);
        }

        let request_start = std::time::Instant::now();
        let result = match self.config.provider {
            LlmProvider::OpenAiCompatible | LlmProvider::MoonshotKimi => self.chat_openai(request).await,
            LlmProvider::GoogleGenerativeAi => self.chat_gemini(request).await,
        };
        let request_duration = request_start.elapsed();
        
        match &result {
            Ok(response) => {
                if let Some(ref usage) = response.usage {
                    crate::info_log!("[{}] {} Chat completed in {:?}: prompt={} completion={} total={}",
                        agent_type, job_info, request_duration, 
                        usage.prompt_tokens, usage.completion_tokens, usage.total_tokens);
                } else {
                    crate::info_log!("[{}] {} Chat completed in {:?} (no usage data)",
                        agent_type, job_info, request_duration);
                }
            }
            Err(e) => {
                crate::error_log!("[{}] {} Chat failed after {:?}: {}", 
                    agent_type, job_info, request_duration, e);
            }
        }

        // Update metrics and rate limiter on success
        // CRITICAL FIX: Only update job metrics for worker calls, NOT for main agent processing.
        // The main agent calls LLM to process worker results - we don't want to count those
        // tokens toward the worker's job metrics (causes inflation/corruption).
        if let Ok(ref response) = result {
            if let Some(ref usage) = response.usage {
                // Only workers update job metrics - main agent LLM calls for processing
                // should not count toward worker job token usage
                if self.is_worker {
                    self.update_job_metrics(usage.prompt_tokens, usage.completion_tokens, estimated_input_tokens);
                }
                
                // Record actual usage to correct rate limiter state if needed
                if let Some(ref limiter) = self.rate_limiter {
                    limiter.record_usage(
                        &self.config.base_url,
                        self.is_worker,
                        usage.total_tokens,
                        estimated_input_tokens as u32
                    );
                }
            }
        }

        result
    }

    /// Helper for main.rs and others
    pub async fn complete(&self, prompt: &str) -> Result<ChatResponse> {
        let request = ChatRequest::new(self.config.model.clone(), vec![ChatMessage::user(prompt)]);
        self.chat(&request).await
    }

    /// Send a chat request with streaming response
    
    pub fn chat_stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>> {
        match self.config.provider {
            LlmProvider::OpenAiCompatible | LlmProvider::MoonshotKimi => self.chat_stream_openai(request),
            LlmProvider::GoogleGenerativeAi => self.chat_stream_gemini(request),
        }
    }

    /// Helper with jittered backoff retry, respecting Retry-After headers and cancellation
    async fn retry_with_backoff<F, Fut>(
        &self,
        operation: F,
    ) -> Result<reqwest::Response>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
    {
        let mut attempt = 0;
        let max_retries = 5;
        let mut delay = Duration::from_secs(3);

        loop {
            // Check for cancellation before making request
            if self.is_cancelled() {
                bail!("Request cancelled by user");
            }

            match operation().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        return Ok(response);
                    }
                    
                    // Handle 429 Rate Limit
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        // Extract Retry-After header
                        let retry_after = response.headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                            .map(Duration::from_secs);
                        
                        // Record rate limit error
                        if let Some(ref limiter) = self.rate_limiter {
                            limiter.record_rate_limit_error(&self.config.base_url, self.is_worker, retry_after);
                        }

                        if attempt >= max_retries {
                            crate::error_log!("Rate limit (429) exceeded max retries ({}), giving up", max_retries);
                            return Ok(response);
                        }

                        // Use Retry-After if available, otherwise use exponential backoff
                        let wait_duration = retry_after.unwrap_or(delay);
                        let agent_type = if self.is_worker { "WORKER" } else { "MAIN" };
                        let job_info = self.job_id.lock().as_ref().map(|j| format!("job={}", &j[..8.min(j.len())])).unwrap_or_default();
                        
                        crate::error_log!("[{}] {} Rate limited (429), waiting {:?} before retry (attempt {}/{})", 
                            agent_type, job_info, wait_duration, attempt + 1, max_retries);
                        
                        let msg = format!("Rate limited (429), waiting {:?} before retry...", wait_duration);
                        self.report_status(&msg);

                        // Check cancellation during wait
                        let token_opt = self.cancel_token.lock().clone();
                        if let Some(token) = token_opt {
                            tokio::select! {
                                _ = sleep(wait_duration) => {},
                                _ = token.cancelled() => {
                                    bail!("Request cancelled by user during rate limit wait");
                                }
                            }
                        } else {
                            sleep(wait_duration).await;
                        }
                        delay *= 2;
                        attempt += 1;
                        continue;
                    }
                    
                    if status.is_server_error() && attempt < max_retries {
                        let msg = format!("Provider error {}, retrying in {:?}...", status, delay);
                        self.report_status(&msg);
                    } else {
                        return Ok(response);
                    }
                }
                Err(e) => {
                    if attempt >= max_retries {
                        return Err(e.into());
                    }
                    let msg = format!("Network error, retrying in {:?}...", delay);
                    self.report_status(&msg);
                }
            }

            attempt += 1;
            
            // Check cancellation before sleep
            let token_opt = self.cancel_token.lock().clone();
            if let Some(token) = token_opt {
                tokio::select! {
                    _ = sleep(delay) => {},
                    _ = token.cancelled() => {
                        bail!("Request cancelled by user");
                    }
                }
            } else {
                sleep(delay).await;
            }
            
            // Jitter: +/- 500ms
            let jitter_ms = rand::thread_rng().gen_range(-500..=500);
            let delay_ms = (delay.as_millis() as i64 + jitter_ms).max(0) as u64;
            delay = Duration::from_millis(delay_ms);
        }
    }

    /// OpenAI-compatible API chat
    async fn chat_openai(&self, request: &ChatRequest) -> Result<ChatResponse> {
        // Validate and sanitize the base URL before constructing the request URL
        let base_url = sanitize_base_url(&self.config.base_url, "Base URL")?;
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        let body = OpenAiRequest {
            model: self.config.model.clone(),
            messages: &request.messages,
            max_completion_tokens: self.config.max_tokens,
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

        let headers = self.build_headers()?;
        let response = self
            .retry_with_backoff(|| async {
                self.http_client
                    .post(&url)
                    .headers(headers.clone())
                    .json(&body)
                    .send()
                    .await
            })
            .await
            .context("Failed to send request to OpenAI API")?;

        match response.status() {
            StatusCode::OK => {
                let text = response.text().await.context("Failed to read OpenAI response text")?;
                let response_body: OpenAiResponse = match serde_json::from_str(&text) {
                    Ok(body) => body,
                    Err(e) => {
                        crate::error_log!("Failed to parse OpenAI response: {}. Raw body: {}", e, text);
                        bail!("Failed to parse OpenAI response: {}. See /logs for raw body.", e);
                    }
                };
                
                let choices = response_body.choices.into_iter().map(|c| Choice {
                    index: c.index,
                    message: ChatMessage {
                        role: super::chat::MessageRole::Assistant,
                        content: c.message.content.unwrap_or_default(),
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
                        reasoning_content: c.message.reasoning_content,
                    },
                    finish_reason: c.finish_reason,
                }).collect();

                Ok(ChatResponse {
                    id: response_body.id,
                    object: response_body.object,
                    created: response_body.created,
                    model: response_body.model,
                    choices,
                    usage: response_body.usage.map(|u| Usage {
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                        total_tokens: u.total_tokens,
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

        // Validate and sanitize the base URL before constructing the request URL
        let base_url = sanitize_base_url(&self.config.base_url, "Base URL")?;
        let api_key = self.config.api_key.as_deref().unwrap_or("");
        
        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            base_url.trim_end_matches('/'),
            self.config.model,
            api_key
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
            .retry_with_backoff(|| async {
                self.http_client
                    .post(&url)
                    .header(CONTENT_TYPE, "application/json")
                    .json(&body)
                    .send()
                    .await
            })
            .await
            .context("Failed to send request to Gemini API")?;

        match response.status() {
            StatusCode::OK => {
                let text = response
                    .text()
                    .await
                    .context("Failed to read Gemini response text")?;
                let response_body: GeminiResponse = match serde_json::from_str(&text) {
                    Ok(body) => body,
                    Err(e) => {
                        bail!(
                            "Failed to parse Gemini response: {}. Response body: {}",
                            e,
                            text
                        );
                    }
                };

                let choices = response_body
                    .candidates
                    .into_iter()
                    .map(|c| Choice {
                        index: c.index,
                        message: ChatMessage {
                            role: super::chat::MessageRole::Assistant,
                            content: c
                                .content
                                .parts
                                .first()
                                .map(|p| p.text.clone())
                                .unwrap_or_default(),
                            name: None,
                            tool_call_id: None,
                            tool_calls: None,
                            reasoning_content: None,
                        },
                        finish_reason: c.finish_reason,
                    })
                    .collect();

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
    
    fn chat_stream_openai<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'a>> {
        // Validate and sanitize the base URL before constructing the request URL
        let base_url = sanitize_base_url(&self.config.base_url, "Base URL").expect("Base URL should have been validated in LlmClient::new");
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        let body = OpenAiRequest {
            model: self.config.model.clone(),
            messages: &request.messages,
            max_completion_tokens: self.config.max_tokens,
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
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
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
                }
            }

            yield StreamEvent::Done;
        })
    }

    /// Gemini streaming chat
    
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
                headers.insert(CONTENT_TYPE, "application/json".parse().context("Invalid content-type header")?);
                
                // OpenRouter specific headers
                if self.config.base_url.contains("openrouter.ai") {
                    headers.insert("HTTP-Referer", "https://github.com/edward/mylm".parse().context("Invalid HTTP-Referer header")?);
                    headers.insert("X-Title", "mylm".parse().context("Invalid X-Title header")?);
                }

                if let Some(api_key) = &self.config.api_key {
                    // Use shared validation to ensure API key is safe for HTTP headers
                    let validated_key = validate_api_key(api_key)?;
                    if !validated_key.is_empty() {
                        let auth_value = format!("Bearer {}", validated_key);
                        headers.insert(
                            "Authorization",
                            auth_value.parse().context("Invalid Authorization header")?,
                        );
                    }
                }
            }
            LlmProvider::GoogleGenerativeAi => {
                // API key is included in URL, not headers
                headers.insert(CONTENT_TYPE, "application/json".parse().context("Invalid content-type header")?);
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
    #[serde(rename = "max_completion_tokens")]
    max_completion_tokens: Option<u32>,
    // Note: max_tokens removed - newer models only support max_completion_tokens
    // Note: temperature removed - not all OpenAI-compatible endpoints support it
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

struct OpenAiResponse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    object: String,
    #[serde(default)]
    created: u64,
    #[serde(default)]
    model: String,
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]

struct OpenAiChoice {
    #[serde(default)]
    index: u32,
    message: OpenAiMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Serialize)]

struct OpenAiMessage {
    role: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiResponseToolCall>>,
    /// Reasoning content for thinking-enabled models (e.g., Kimi K2.5, DeepSeek)
    /// Must be preserved when sending messages back to the API
    #[serde(default, rename = "reasoning_content")]
    reasoning_content: Option<String>,
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

struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Deserialize)]
struct OpenAiStreamResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    object: String,
    #[allow(dead_code)]
    created: u64,
    #[allow(dead_code)]
    model: String,
    choices: Vec<OpenAiStreamChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiStreamChoice {
    #[allow(dead_code)]
    index: u32,
    delta: OpenAiDelta,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiDelta {
    #[allow(dead_code)]
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
