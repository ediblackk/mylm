//! Web Search Capability
//!
//! Searches the web using various providers (Kimi, SerpAPI, Brave, etc.)

use crate::agent::runtime::{
    capability::{Capability, ToolCapability},
    context::RuntimeContext,
    error::ToolError,
};
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use reqwest::Client;

/// Web search capability
pub struct WebSearchCapability {
    client: Client,
    api_key: String,
    provider: SearchProvider,
}

#[derive(Debug, Clone)]
pub enum SearchProvider {
    Kimi,
    SerpApi,
    Brave,
    Custom(String),
}

impl WebSearchCapability {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            provider: SearchProvider::Kimi,
        }
    }
    
    pub fn with_provider(mut self, provider: SearchProvider) -> Self {
        self.provider = provider;
        self
    }
    
    /// Search the web
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
        match self.provider {
            SearchProvider::Kimi => self.search_kimi(query, max_results).await,
            SearchProvider::SerpApi => self.search_serpapi(query, max_results).await,
            SearchProvider::Brave => self.search_brave(query, max_results).await,
            SearchProvider::Custom(ref url) => self.search_custom(url, query, max_results).await,
        }
    }
    
    async fn search_kimi(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
        let url = "https://api.moonshot.cn/v1/web/search";
        
        let response = self.client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&[("query", query), ("limit", &max_results.to_string())])
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        if !response.status().is_success() {
            return Err(format!("API error: {}", response.status()));
        }
        
        let json: serde_json::Value = response.json().await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        
        // Parse Kimi search response format
        let mut results = Vec::new();
        
        if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
            for item in data.iter().take(max_results) {
                let title = item.get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("No title");
                let url = item.get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("#");
                let snippet = item.get("snippet")
                    .and_then(|s| s.as_str())
                    .or_else(|| item.get("content").and_then(|c| c.as_str()))
                    .unwrap_or("No description");
                
                results.push(SearchResult {
                    title: title.to_string(),
                    url: url.to_string(),
                    snippet: snippet.to_string(),
                });
            }
        }
        
        if results.is_empty() {
            // Fallback if no results or unexpected format
            results.push(SearchResult {
                title: "Search completed".to_string(),
                url: "#".to_string(),
                snippet: format!("No results found for '{}'", query),
            });
        }
        
        Ok(results)
    }
    
    async fn search_serpapi(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
        let url = "https://serpapi.com/search";
        
        let response = self.client
            .get(url)
            .query(&[
                ("q", query),
                ("api_key", &self.api_key),
                ("engine", "google"),
                ("num", &max_results.to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        if !response.status().is_success() {
            return Err(format!("API error: {}", response.status()));
        }
        
        let json: serde_json::Value = response.json().await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        
        let mut results = Vec::new();
        
        // SerpAPI returns results in organic_results array
        if let Some(organic_results) = json.get("organic_results").and_then(|r| r.as_array()) {
            for item in organic_results.iter().take(max_results) {
                let title = item.get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("No title");
                let url = item.get("link")
                    .and_then(|l| l.as_str())
                    .unwrap_or("#");
                let snippet = item.get("snippet")
                    .and_then(|s| s.as_str())
                    .unwrap_or("No description");
                
                results.push(SearchResult {
                    title: title.to_string(),
                    url: url.to_string(),
                    snippet: snippet.to_string(),
                });
            }
        }
        
        if results.is_empty() {
            results.push(SearchResult {
                title: "Search completed".to_string(),
                url: "#".to_string(),
                snippet: format!("No results found for '{}'", query),
            });
        }
        
        Ok(results)
    }
    
    async fn search_brave(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
        let url = "https://api.search.brave.com/res/v1/web/search";
        
        let response = self.client
            .get(url)
            .header("X-Subscription-Token", &self.api_key)
            .query(&[("q", query), ("count", &max_results.to_string())])
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        if !response.status().is_success() {
            return Err(format!("API error: {}", response.status()));
        }
        
        let json: serde_json::Value = response.json().await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        
        let mut results = Vec::new();
        
        // Brave returns results in web.results array
        if let Some(web) = json.get("web").and_then(|w| w.get("results")).and_then(|r| r.as_array()) {
            for item in web.iter().take(max_results) {
                let title = item.get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("No title");
                let url = item.get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("#");
                let snippet = item.get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("No description");
                
                results.push(SearchResult {
                    title: title.to_string(),
                    url: url.to_string(),
                    snippet: snippet.to_string(),
                });
            }
        }
        
        if results.is_empty() {
            results.push(SearchResult {
                title: "Search completed".to_string(),
                url: "#".to_string(),
                snippet: format!("No results found for '{}'", query),
            });
        }
        
        Ok(results)
    }
    
    async fn search_custom(&self, url: &str, query: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
        // Custom search endpoint
        let response = self.client
            .get(url)
            .query(&[("q", query), ("limit", &max_results.to_string())])
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        if !response.status().is_success() {
            return Err(format!("API error: {}", response.status()));
        }
        
        Ok(vec![SearchResult {
            title: "Custom search result".to_string(),
            url: url.to_string(),
            snippet: format!("Search for '{}' via custom provider", query),
        }])
    }
}

impl Capability for WebSearchCapability {
    fn name(&self) -> &'static str {
        "web-search"
    }
}

#[async_trait::async_trait]
impl ToolCapability for WebSearchCapability {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse args: "query" or "query max_results"
        let args_str = call.arguments.as_str().unwrap_or("");
        let parts: Vec<&str> = args_str.splitn(2, ' ').collect();
        let query = parts[0];
        let max_results: usize = parts.get(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        
        match self.search(query, max_results).await {
            Ok(results) => {
                let output = results.iter()
                    .map(|r| format!("{}\n{}\n{}\n", r.title, r.url, r.snippet))
                    .collect::<Vec<_>>()
                    .join("\n");
                
                Ok(ToolResult::Success {
                    output,
                    structured: None,
                })
            }
            Err(e) => Ok(ToolResult::Error {
                message: format!("Search error: {}", e),
                code: Some("SEARCH_ERROR".to_string()),
                retryable: true,
            }),
        }
    }
}

/// Search result
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Stub web search for testing
pub struct StubWebSearch;

impl Capability for StubWebSearch {
    fn name(&self) -> &'static str {
        "stub-web-search"
    }
}

#[async_trait::async_trait]
impl ToolCapability for StubWebSearch {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        let _ = call.name; // use name
        Ok(ToolResult::Success {
            output: format!("Stub search results for: {}", call.arguments),
            structured: None,
        })
    }
}
