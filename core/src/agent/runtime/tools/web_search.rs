//! Web search tool
//!
//! Supports multiple search providers:
//! - DuckDuckGo (free, no API key required)
//! - SerpAPI (requires API key)
//! - Brave Search (requires API key)
//! - Kimi (Moonshot AI) - uses `$web_search` builtin function via API
//!
//! ## Kimi Web Search Architecture
//!
//! Kimi does not expose a standalone web search HTTP endpoint. Instead, web search
//! works through the chat completions API using builtin functions:
//!
//! 1. Register `$web_search` as a `builtin_function` tool in the chat request
//! 2. Model returns `finish_reason: "tool_calls"` with `$web_search` when it wants to search
//! 3. Client echoes back the arguments via `role: "tool"` message
//! 4. Model performs the search internally and returns results in the follow-up response
//!
//! For standalone tool usage, the Kimi provider creates its own LLM client and
//! handles the builtin function flow directly within the tool.

use crate::agent::runtime::capability::{Capability, ToolCapability};
use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::ToolError;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

/// Circuit breaker state for preventing endless retries
#[derive(Debug)]
struct CircuitBreaker {
    /// Number of consecutive failures
    failure_count: AtomicU32,
    /// Timestamp of last failure
    last_failure: std::sync::Mutex<Option<Instant>>,
    /// Maximum failures before opening circuit
    max_failures: u32,
    /// Cooldown period before retrying (seconds)
    cooldown_secs: u64,
}

impl CircuitBreaker {
    fn new(max_failures: u32, cooldown_secs: u64) -> Self {
        Self {
            failure_count: AtomicU32::new(0),
            last_failure: std::sync::Mutex::new(None),
            max_failures,
            cooldown_secs,
        }
    }
    
    /// Check if circuit is open (too many failures)
    fn is_open(&self) -> bool {
        let count = self.failure_count.load(Ordering::SeqCst);
        if count < self.max_failures {
            return false;
        }
        
        // Check if cooldown has elapsed
        let last = self.last_failure.lock().unwrap();
        if let Some(last_time) = *last {
            let elapsed = last_time.elapsed().as_secs();
            if elapsed >= self.cooldown_secs {
                // Reset circuit after cooldown
                drop(last);
                self.failure_count.store(0, Ordering::SeqCst);
                *self.last_failure.lock().unwrap() = None;
                return false;
            }
        }
        
        true
    }
    
    /// Record a success - reset failure count
    fn record_success(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
        *self.last_failure.lock().unwrap() = None;
    }
    
    /// Record a failure
    fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        *self.last_failure.lock().unwrap() = Some(Instant::now());
        
        if count >= self.max_failures {
            crate::warn_log!("[WEB_SEARCH] Circuit breaker OPENED after {} failures. Will retry after {} seconds", 
                count, self.cooldown_secs);
        }
    }
}

/// Web search configuration
#[derive(Debug, Clone)]
pub struct WebSearchConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub provider: SearchProvider,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            provider: SearchProvider::DuckDuckGo,
        }
    }
}

/// Search provider
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchProvider {
    /// DuckDuckGo HTML search (free, no API key needed)
    DuckDuckGo,
    /// SerpAPI (requires API key)
    SerpApi,
    /// Brave Search (requires API key)
    Brave,
    /// Google Custom Search (requires API key)
    Google,
    /// Exa/Metaphor neural search (requires API key)
    Exa,
    /// OpenAI web search (requires API key)
    OpenAi,
    /// Tavily AI search (requires API key)
    Tavily,
    /// Kimi (Moonshot AI) web search (requires API key)
    Kimi,
    /// Custom search provider
    Custom,
}

/// Web search tool
#[derive(Debug)]
pub struct WebSearchTool {
    config: WebSearchConfig,
    client: reqwest::Client,
    /// Circuit breaker to prevent endless retries
    circuit_breaker: CircuitBreaker,
}

impl WebSearchTool {
    /// Create a new web search tool with default config
    pub fn new() -> Self {
        Self::with_config(WebSearchConfig::default())
    }

    /// Create with custom config
    pub fn with_config(config: WebSearchConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("mylm-assistant/0.1")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { 
            config, 
            client,
            circuit_breaker: CircuitBreaker::new(3, 60), // 3 failures, 60 second cooldown
        }
    }

    /// Reload configuration from the global config file
    /// This allows the tool to pick up changes made via the TUI/settings menu
    pub fn reload_config_from_disk(&mut self) {
        use crate::config::store::Config;
        
        let config = Config::load_or_default();
        let profile = config.active_profile();
        
        // Update our config from the profile settings
        self.config.enabled = profile.web_search.enabled;
        self.config.api_key = profile.web_search.api_key.clone();
        
        // Map the config SearchProvider to our internal SearchProvider
        self.config.provider = match profile.web_search.provider {
            crate::config::SearchProvider::DuckDuckGo => SearchProvider::DuckDuckGo,
            crate::config::SearchProvider::Serpapi => SearchProvider::SerpApi,
            crate::config::SearchProvider::Brave => SearchProvider::Brave,
            crate::config::SearchProvider::Google => SearchProvider::Google,
            crate::config::SearchProvider::Exa => SearchProvider::Exa,
            crate::config::SearchProvider::Openai => SearchProvider::OpenAi,
            crate::config::SearchProvider::Tavily => SearchProvider::Tavily,
            crate::config::SearchProvider::Kimi => SearchProvider::Kimi,
            crate::config::SearchProvider::Custom => SearchProvider::Custom,
        };
        
        log::debug!(
            "[WebSearchTool] Reloaded config: enabled={}, provider={:?}",
            self.config.enabled,
            self.config.provider
        );
    }

    /// Search using DuckDuckGo HTML (no API key needed)
    async fn search_duckduckgo(&self, query: &str) -> Result<ToolResult, ToolError> {
        // DuckDuckGo HTML interface - simple and doesn't require API key
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );
        
        crate::debug_log!("[WEB_SEARCH:DuckDuckGo] Sending request to: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| {
                crate::error_log!("[WEB_SEARCH:DuckDuckGo] HTTP request failed: {}", e);
                ToolError::new(format!("HTTP request failed: {}", e))
            })?;
        
        let status = response.status();
        crate::debug_log!("[WEB_SEARCH:DuckDuckGo] Response status: {}", status);

        let html = response
            .text()
            .await
            .map_err(|e| {
                crate::error_log!("[WEB_SEARCH:DuckDuckGo] Failed to read response body: {}", e);
                ToolError::new(format!("Failed to read response: {}", e))
            })?;
        
        crate::debug_log!("[WEB_SEARCH:DuckDuckGo] Response body length: {} bytes", html.len());

        // Simple HTML parsing to extract results
        let results = self.parse_duckduckgo_results(&html);
        
        crate::info_log!("[WEB_SEARCH:DuckDuckGo] Parsed {} results", results.len());

        if results.is_empty() {
            Ok(ToolResult::Success {
                output: "No results found.".to_string(),
                structured: None,
            })
        } else {
            Ok(ToolResult::Success {
                output: results.join("\n\n"),
                structured: None,
            })
        }
    }

    /// Parse DuckDuckGo HTML results
    fn parse_duckduckgo_results(&self, html: &str) -> Vec<String> {
        let mut results = Vec::new();

        // Simple regex-like parsing for DuckDuckGo results
        // Look for result blocks
        for block in html.split("class=\"result\"") {
            if let Some(title_start) = block.find("class=\"result__a\"") {
                if let Some(href_start) = block[title_start..].find("href=\"") {
                    let href_pos = title_start + href_start + 6;
                    if let Some(href_end) = block[href_pos..].find("\"") {
                        let url = &block[href_pos..href_pos + href_end];

                        // Get title
                        if let Some(title_close) = block[href_pos..].find(">") {
                            let title_pos = href_pos + title_close + 1;
                            if let Some(title_end) = block[title_pos..].find("</a>") {
                                let title = &block[title_pos..title_pos + title_end];
                                let title = html_escape::decode_html_entities(title);

                                // Get snippet
                                let snippet = if let Some(snippet_start) =
                                    block.find("class=\"result__snippet\"")
                                {
                                    if let Some(snippet_close) = block[snippet_start..].find(">") {
                                        let snippet_pos = snippet_start + snippet_close + 1;
                                        if let Some(snippet_end) =
                                            block[snippet_pos..].find("</a>")
                                        {
                                            let s = &block[snippet_pos..snippet_pos + snippet_end];
                                            html_escape::decode_html_entities(s).to_string()
                                        } else {
                                            String::new()
                                        }
                                    } else {
                                        String::new()
                                    }
                                } else {
                                    String::new()
                                };

                                if !title.is_empty() {
                                    results.push(format!(
                                        "### {}\n{}\nURL: {}",
                                        title,
                                        if snippet.is_empty() {
                                            ""
                                        } else {
                                            &snippet
                                        },
                                        url
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        results.into_iter().take(5).collect()
    }

    /// Search using SerpAPI
    async fn search_serpapi(&self, query: &str, api_key: Option<&str>) -> Result<ToolResult, ToolError> {
        let api_key = api_key
            .ok_or_else(|| {
                log::error!("[WEB_SEARCH:SerpAPI] API key not configured");
                ToolError::new("SerpAPI key not configured")
            })?;

        // Mask API key for logging (show only first 8 chars)
        let key_preview = if api_key.len() > 8 {
            format!("{}...", &api_key[..8])
        } else {
            "***".to_string()
        };
        crate::debug_log!("[WEB_SEARCH:SerpAPI] Using API key starting with: {}", key_preview);

        let url = format!(
            "https://serpapi.com/search.json?q={}&api_key={}&engine=google",
            urlencoding::encode(query),
            api_key
        );
        
        crate::debug_log!("[WEB_SEARCH:SerpAPI] Sending request for query: '{}'", query);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| {
                crate::error_log!("[WEB_SEARCH:SerpAPI] HTTP request failed: {}", e);
                ToolError::new(format!("HTTP request failed: {}", e))
            })?;
        
        let status = response.status();
        crate::debug_log!("[WEB_SEARCH:SerpAPI] Response status: {}", status);

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| {
                crate::error_log!("[WEB_SEARCH:SerpAPI] Failed to parse JSON response: {}", e);
                ToolError::new(format!("Failed to parse JSON: {}", e))
            })?;

        // Extract results
        let mut results = Vec::new();

        if let Some(organic) = data.get("organic_results").and_then(|r| r.as_array()) {
            for res in organic.iter().take(5) {
                let title = res
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No Title");
                let snippet = res.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
                let link = res.get("link").and_then(|v| v.as_str()).unwrap_or("");

                results.push(format!(
                    "### {}\n{}\nURL: {}",
                    title, snippet, link
                ));
            }
        }
        
        crate::info_log!("[WEB_SEARCH:SerpAPI] Parsed {} results", results.len());

        if results.is_empty() {
            crate::warn_log!("[WEB_SEARCH:SerpAPI] No organic_results found in response");
            Ok(ToolResult::Success {
                output: "No results found.".to_string(),
                structured: None,
            })
        } else {
            Ok(ToolResult::Success {
                output: results.join("\n\n"),
                structured: None,
            })
        }
    }

    /// Search using Kimi (Moonshot AI) web search API
    /// 
    /// Kimi's `$web_search` is a builtin_function. This implementation creates an LLM client
    /// and uses Kimi's builtin web search directly within this tool.
    ///
    /// How it works:
    /// 1. Create an LlmClient with MoonshotKimi provider
    /// 2. Register `$web_search` as a `builtin_function` tool
    /// 3. When Kimi returns `finish_reason: "tool_calls"` with `$web_search`, echo back the arguments
    /// 4. Kimi performs the search internally and returns results in the follow-up response
    async fn search_kimi(&self, query: &str, api_key: Option<&str>) -> Result<ToolResult, ToolError> {
        use crate::llm::{
            chat::{ChatFunction, ChatMessage, ChatRequest, ChatTool},
            LlmClient, LlmConfig, LlmProvider,
        };
        
        crate::info_log!("[WEB_SEARCH:Kimi] Starting standalone Kimi web search for: '{}'", query);
        
        let api_key = api_key
            .ok_or_else(|| {
                crate::error_log!("[WEB_SEARCH:Kimi] API key not configured");
                ToolError::new("Kimi API key not configured")
            })?;
        
        let base_url = "https://api.moonshot.ai/v1".to_string();
        let model = "kimi-k2-turbo-preview".to_string();
        
        let llm_config = LlmConfig::new(
            LlmProvider::MoonshotKimi,
            base_url,
            model,
            Some(api_key.to_string()),
            128000,
        );
        
        let client = LlmClient::new(llm_config)
            .map_err(|e| {
                crate::error_log!("[WEB_SEARCH:Kimi] Failed to create LLM client: {}", e);
                ToolError::new(format!("Failed to create LLM client: {}", e))
            })?;
        
        let web_search_tool = ChatTool {
            type_: "builtin_function".to_string(),
            function: ChatFunction {
                name: "$web_search".to_string(),
                description: None,
                parameters: None,
            },
        };
        
        // Initial messages to trigger web search
        let mut messages = vec![
            ChatMessage::system("You are a helpful assistant with web search capabilities. When asked to search, use the $web_search tool."),
            ChatMessage::user(query),
        ];
        
        // Handle the tool call loop (max 2 turns to prevent infinite loops)
        for turn in 0..2 {
            crate::debug_log!("[WEB_SEARCH:Kimi] Turn {}: Sending request", turn);
            
            let request = ChatRequest::new(client.model().to_string(), messages.clone())
                .with_tools(vec![web_search_tool.clone()]);
            
            let response = client.chat(&request).await
                .map_err(|e| {
                    crate::error_log!("[WEB_SEARCH:Kimi] API request failed: {}", e);
                    ToolError::new(format!("Kimi API request failed: {}", e))
                })?;
            
            let choice = response.choices.first()
                .ok_or_else(|| ToolError::new("Kimi returned no choices"))?;
            
            // Check if we need to handle tool calls
            if choice.finish_reason.as_deref() == Some("tool_calls") {
                if let Some(tool_calls) = &choice.message.tool_calls {
                    crate::debug_log!("[WEB_SEARCH:Kimi] Got {} tool call(s)", tool_calls.len());
                    
                    // Add assistant message to context
                    messages.push(choice.message.clone());
                    
                    for tool_call in tool_calls {
                        if tool_call.function.name == "$web_search" {
                            crate::debug_log!("[WEB_SEARCH:Kimi] Echoing back $web_search arguments");
                            
                            // Echo back the arguments as the result for builtin_function.$web_search
                            messages.push(ChatMessage::tool(
                                tool_call.id.clone(),
                                tool_call.function.name.clone(),
                                tool_call.function.arguments.clone(),
                            ));
                        }
                    }
                    continue; // Go to next iteration to get final response
                }
            }
            
            // We have the final response
            let content = response.content().trim().to_string();
            crate::info_log!("[WEB_SEARCH:Kimi] Search completed, response length: {} chars", content.len());
            
            if content.is_empty() {
                return Ok(ToolResult::Success {
                    output: "No results found.".to_string(),
                    structured: None,
                });
            }
            
            return Ok(ToolResult::Success {
                output: content,
                structured: None,
            });
        }
        
        crate::error_log!("[WEB_SEARCH:Kimi] Search timed out (too many tool call turns)");
        Err(ToolError::new("Kimi search timed out (too many tool call turns)"))
    }

    /// Search using Exa AI API
    /// 
    /// Exa is a neural search engine optimized for LLM consumption.
    /// API docs: https://docs.exa.ai/reference/search
    ///
    /// Extra params that can be configured:
    /// - `type`: "auto" (default), "instant", or "deep"
    /// - `numResults`: Number of results (1-100), default 5
    /// - `category`: Filter by category - "company", "people", "tweet", "news", "research paper", etc.
    /// - `maxAgeHours`: Content freshness - 0=always livecrawl, -1=never, default uses cache
    /// - `includeDomains`: Comma-separated list of domains to include
    /// - `excludeDomains`: Comma-separated list of domains to exclude
    /// - `contents.text`: Set to "true" to get full text instead of highlights
    /// - `contents.highlights.maxCharacters`: Max characters in highlights (default 2000)
    async fn search_exa(
        &self,
        query: &str,
        api_key: Option<&str>,
        extra_params: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<ToolResult, ToolError> {
        let api_key = api_key
            .ok_or_else(|| {
                crate::error_log!("[WEB_SEARCH:Exa] API key not configured");
                ToolError::new("Exa API key not configured")
            })?;
        
        crate::info_log!("[WEB_SEARCH:Exa] Searching for: '{}'", query);
        
        // Build the Exa search request with extra params
        let mut request_body = serde_json::Map::new();
        request_body.insert("query".to_string(), serde_json::json!(query));
        
        // Set defaults with overrides from extra_params
        let num_results = extra_params
            .and_then(|p| p.get("numResults"))
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(5);
        request_body.insert("numResults".to_string(), serde_json::json!(num_results));
        
        let search_type = extra_params
            .and_then(|p| p.get("type"))
            .map(|v| v.as_str())
            .unwrap_or("auto");
        request_body.insert("type".to_string(), serde_json::json!(search_type));
        
        // Optional category filter
        if let Some(category) = extra_params.and_then(|p| p.get("category")) {
            request_body.insert("category".to_string(), serde_json::json!(category));
        }
        
        // Optional maxAgeHours for content freshness
        if let Some(max_age) = extra_params.and_then(|p| p.get("maxAgeHours")) {
            if let Ok(age) = max_age.parse::<i64>() {
                request_body.insert("maxAgeHours".to_string(), serde_json::json!(age));
            }
        }
        
        // Optional domain filters
        if let Some(domains) = extra_params.and_then(|p| p.get("includeDomains")) {
            let domain_list: Vec<String> = domains.split(',').map(|s| s.trim().to_string()).collect();
            request_body.insert("includeDomains".to_string(), serde_json::json!(domain_list));
        }
        
        if let Some(domains) = extra_params.and_then(|p| p.get("excludeDomains")) {
            let domain_list: Vec<String> = domains.split(',').map(|s| s.trim().to_string()).collect();
            request_body.insert("excludeDomains".to_string(), serde_json::json!(domain_list));
        }
        
        // Build contents configuration
        let mut contents = serde_json::Map::new();
        
        // Check if user wants full text instead of highlights
        let use_full_text = extra_params
            .and_then(|p| p.get("contents.text"))
            .map(|v| v == "true")
            .unwrap_or(false);
        
        if use_full_text {
            let max_chars = extra_params
                .and_then(|p| p.get("contents.maxCharacters"))
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(15000);
            contents.insert("text".to_string(), serde_json::json!({
                "maxCharacters": max_chars
            }));
        } else {
            // Default: use highlights for token efficiency
            let max_chars = extra_params
                .and_then(|p| p.get("contents.highlights.maxCharacters"))
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(2000);
            contents.insert("highlights".to_string(), serde_json::json!({
                "maxCharacters": max_chars
            }));
        }
        
        request_body.insert("contents".to_string(), serde_json::json!(contents));
        
        let request_body = serde_json::json!(request_body);
        
        let response = self
            .client
            .post("https://api.exa.ai/search")
            .header("x-api-key", api_key)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                crate::error_log!("[WEB_SEARCH:Exa] HTTP request failed: {}", e);
                ToolError::new(format!("Exa API request failed: {}", e))
            })?;
        
        let status = response.status();
        crate::debug_log!("[WEB_SEARCH:Exa] Response status: {}", status);
        
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            crate::error_log!("[WEB_SEARCH:Exa] API error: {} - {}", status, error_text);
            return Err(ToolError::new(format!("Exa API error: {} - {}", status, error_text)));
        }
        
        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| {
                crate::error_log!("[WEB_SEARCH:Exa] Failed to parse JSON: {}", e);
                ToolError::new(format!("Failed to parse Exa response: {}", e))
            })?;
        
        // Parse Exa results
        let mut results = Vec::new();
        
        if let Some(results_array) = data.get("results").and_then(|r| r.as_array()) {
            for result in results_array.iter().take(5) {
                let title = result
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No Title");
                let url = result
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                
                // Get highlights if available, fall back to text
                let snippet = result
                    .get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        result
                            .get("highlights")
                            .and_then(|h| h.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|h| h.as_str())
                    })
                    .unwrap_or("");
                
                results.push(format!(
                    "### {}\n{}\nURL: {}",
                    title,
                    if snippet.is_empty() { "" } else { snippet },
                    url
                ));
            }
        }
        
        crate::info_log!("[WEB_SEARCH:Exa] Parsed {} results", results.len());
        
        if results.is_empty() {
            Ok(ToolResult::Success {
                output: "No results found.".to_string(),
                structured: None,
            })
        } else {
            Ok(ToolResult::Success {
                output: results.join("\n\n"),
                structured: None,
            })
        }
    }

    /// Stub for unimplemented search providers
    async fn search_not_implemented(&self, provider: &str) -> Result<ToolResult, ToolError> {
        crate::warn_log!("[WEB_SEARCH] Provider '{}' not implemented yet", provider);
        Err(ToolError::new(format!(
            "Provider '{}' is not implemented yet. Please use DuckDuckGo, SerpAPI, Exa, or Kimi.",
            provider
        )))
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Capability for WebSearchTool {
    fn name(&self) -> &'static str {
        "web_search"
    }
}

#[async_trait::async_trait]
impl ToolCapability for WebSearchTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Reload config from disk to pick up any changes made via TUI/settings
        let config = crate::config::store::Config::load_or_default();
        let profile = config.active_profile();
        let enabled = profile.web_search.enabled;
        
        crate::info_log!("[WEB_SEARCH] Tool called - enabled: {}, profile: {}", enabled, config.active_profile);
        crate::debug_log!("[WEB_SEARCH] Raw arguments: {}", call.arguments);
        
        if !enabled {
            crate::warn_log!("[WEB_SEARCH] Web search is disabled in configuration");
            return Ok(ToolResult::Error {
                message: "Web search is currently disabled. Enable it in configuration.".to_string(),
                code: Some("DISABLED".to_string()),
                retryable: false,
            });
        }
        
        // Check circuit breaker to prevent endless retries
        if self.circuit_breaker.is_open() {
            crate::warn_log!("[WEB_SEARCH] Circuit breaker is OPEN - too many recent failures. Skipping search.");
            return Ok(ToolResult::Error {
                message: "Web search is temporarily unavailable due to repeated failures. Please try again later or use a different provider.".to_string(),
                code: Some("CIRCUIT_OPEN".to_string()),
                retryable: false,
            });
        }

        // Extract query from args
        let query = call
            .arguments
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| {
                call.arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .ok_or_else(|| ToolError::new("Expected query string or {\"query\": \"...\"}"))?;

        if query.is_empty() {
            crate::warn_log!("[WEB_SEARCH] Empty search query received");
            return Ok(ToolResult::Error {
                message: "Search query cannot be empty".to_string(),
                code: Some("INVALID_QUERY".to_string()),
                retryable: false,
            });
        }

        // Map the config SearchProvider to our internal SearchProvider
        let config_provider = &profile.web_search.provider;
        let provider = match config_provider {
            crate::config::SearchProvider::DuckDuckGo => SearchProvider::DuckDuckGo,
            crate::config::SearchProvider::Serpapi => SearchProvider::SerpApi,
            crate::config::SearchProvider::Brave => SearchProvider::Brave,
            crate::config::SearchProvider::Google => SearchProvider::Google,
            crate::config::SearchProvider::Exa => SearchProvider::Exa,
            crate::config::SearchProvider::Openai => SearchProvider::OpenAi,
            crate::config::SearchProvider::Tavily => SearchProvider::Tavily,
            crate::config::SearchProvider::Kimi => SearchProvider::Kimi,
            crate::config::SearchProvider::Custom => SearchProvider::Custom,
        };
        
        // Get API key status (without logging the actual key)
        let api_key_status = if profile.web_search.api_key.is_some() {
            "set"
        } else {
            "not set"
        };
        
        crate::info_log!("[WEB_SEARCH] Config - provider: {:?} (resolved to: {:?}), API key: {}", 
            config_provider, provider, api_key_status);
        crate::info_log!("[WEB_SEARCH] Searching for query: '{}'", query);
        
        // Get API key from disk config for providers that need it
        let api_key = profile.web_search.api_key.as_deref();
        
        // Execute search and log results
        let result = match provider {
            SearchProvider::DuckDuckGo => {
                crate::debug_log!("[WEB_SEARCH] Using DuckDuckGo (no API key required)");
                self.search_duckduckgo(&query).await
            }
            SearchProvider::SerpApi => {
                crate::debug_log!("[WEB_SEARCH] Using SerpAPI (API key required)");
                self.search_serpapi(&query, api_key).await
            }
            SearchProvider::Kimi => {
                crate::debug_log!("[WEB_SEARCH] Using Kimi");
                self.search_kimi(&query, api_key).await
            }
            SearchProvider::Exa => {
                crate::debug_log!("[WEB_SEARCH] Using Exa (API key required)");
                let extra_params = profile.web_search.extra_params.as_ref();
                self.search_exa(&query, api_key, extra_params).await
            }
            SearchProvider::Brave => {
                self.search_not_implemented("Brave").await
            }
            SearchProvider::Google => {
                self.search_not_implemented("Google").await
            }
            SearchProvider::OpenAi => {
                self.search_not_implemented("OpenAI").await
            }
            SearchProvider::Tavily => {
                self.search_not_implemented("Tavily").await
            }
            SearchProvider::Custom => {
                self.search_not_implemented("Custom").await
            }
        };
        
        // Log the result and update circuit breaker
        match &result {
            Ok(ToolResult::Success { output, .. }) => {
                crate::info_log!("[WEB_SEARCH] Search successful - output length: {} bytes", output.len());
                crate::debug_log!("[WEB_SEARCH] Search results preview: {}", 
                    output.chars().take(200).collect::<String>());
                self.circuit_breaker.record_success();
            }
            Ok(ToolResult::Error { message, code, .. }) => {
                crate::error_log!("[WEB_SEARCH] Search failed - code: {:?}, error: {}", code, message);
                self.circuit_breaker.record_failure();
            }
            Ok(ToolResult::Cancelled) => {
                crate::warn_log!("[WEB_SEARCH] Search was cancelled");
                // Don't record cancellation as failure
            }
            Err(e) => {
                crate::error_log!("[WEB_SEARCH] Tool execution error: {}", e);
                self.circuit_breaker.record_failure();
            }
        }
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duckduckgo() {
        let tool = WebSearchTool::new();
        let html = r#"
            <div class="result">
                <a class="result__a" href="https://example.com">Test Title</a>
                <a class="result__snippet">Test snippet here</a>
            </div>
        "#;

        let results = tool.parse_duckduckgo_results(html);
        assert!(!results.is_empty());
        assert!(results[0].contains("Test Title"));
    }
}
