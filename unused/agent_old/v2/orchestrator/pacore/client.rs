//! HTTP client for LLM chat completions API.
//!
//! Provides a async client with retry logic, streaming support, and
//! configurable timeouts for interacting with OpenAI-compatible APIs.
//!
//! # Main Types
//! - `ChatClient`: Configurable HTTP client for chat completions

use reqwest::Client;
use std::time::Duration;
use crate::pacore::error::Error;
use crate::pacore::model::{ChatRequest, ChatResponse};
use crate::pacore::error::retry;
use futures_util::Stream;
use std::pin::Pin;
use tracing::debug;

#[derive(Clone)]
pub struct ChatClient {
    endpoint: String,
    api_key: String,
    client: Client,
    timeout: Duration,
    max_retries: usize,
}

impl ChatClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        // Construct full endpoint URL by appending /chat/completions to base URL
        let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        Self {
            endpoint,
            api_key,
            client: Client::new(),
            timeout: Duration::from_secs(60),
            max_retries: 3,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, Error> {
        let client_clone = self.clone();
        let request_clone = request.clone();

        retry(move || {
            let client = client_clone.clone();
            let req = request_clone.clone();
            async move {
                client.execute_request(req).await
            }
        }, self.max_retries, 1000).await
    }

    async fn execute_request(&self, request: ChatRequest) -> Result<ChatResponse, Error> {
        let response = self.client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .timeout(self.timeout)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(Error::Api { status, message });
        }

        // Debug: Log response content-type and status
        let content_type = response.headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("missing");
        
        debug!("PaCoRe response status: {}, content-type: {}", response.status(), content_type);
        
        // Try to get response as text first for better error messages
        let response_bytes = response.bytes().await?;
        let response_str = String::from_utf8_lossy(&response_bytes);
        
        // Log first 500 chars for debugging
        debug!("PaCoRe response preview: {}", response_str.chars().take(500).collect::<String>());
        
        // Now parse JSON
        match serde_json::from_slice(&response_bytes) {
            Ok(chat_response) => Ok(chat_response),
            Err(e) => {
                // Include response body in error for debugging
                let preview = response_str.chars().take(200).collect::<String>();
                Err(Error::Internal(format!(
                    "Failed to decode response body: {}\nResponse preview: {}",
                    e, preview
                )))
            }
        }
    }

    pub async fn stream_chat(
        &self,
        mut request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatResponse, Error>> + Send>>, Error> {
        request.stream = Some(true);

        debug!("PaCoRe streaming request to endpoint: {}", self.endpoint);
        let response = self.client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .timeout(self.timeout)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            debug!("PaCoRe streaming error response ({}): {}", status, message);
            return Err(Error::Api { status, message });
        }

        let content_type = response.headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("missing");
        debug!("PaCoRe stream response content-type: {}", content_type);

        let stream = response.bytes_stream();
        let mut buffer = String::new();

        let mapped_stream = async_stream::try_stream! {
            for await chunk_result in stream {
                let chunk = chunk_result.map_err(|e| Error::Io(std::io::Error::other(e)))?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);

                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer.drain(..line_end + 1).collect::<String>();
                    let line = line.trim();

                    if line.is_empty() {
                        continue;
                    }

                    if line == "data: [DONE]" {
                        break;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        match serde_json::from_str::<ChatResponse>(data) {
                            Ok(chat_resp) => yield chat_resp,
                            Err(e) => {
                                debug!("Failed to parse SSE data: {} - Data: {}", e, data);
                                // Don't yield error for individual chunk failures - just log and continue
                            }
                        }
                    }
                }
            }
        };

        Ok(Box::pin(mapped_stream))
    }
}
