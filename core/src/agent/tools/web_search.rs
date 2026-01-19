use crate::agent::tool::{Tool, ToolOutput};
use crate::config::WebSearchConfig;
use crate::llm::{
    chat::{ChatFunction, ChatMessage, ChatRequest, ChatTool},
    LlmClient, LlmConfig, LlmProvider,
};
use crate::terminal::app::TuiEvent;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::error::Error as StdError;
use tokio::sync::mpsc;

/// A tool for searching the web using various providers (Kimi, Google, etc.)
pub struct WebSearchTool {
    config: WebSearchConfig,
    client: reqwest::Client,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
}

impl WebSearchTool {
    pub fn new(config: WebSearchConfig, event_tx: mpsc::UnboundedSender<TuiEvent>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("mylm-assistant/0.1")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, client, event_tx }
    }

    async fn call_kimi(&self, query: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let _ = self.event_tx.send(TuiEvent::StatusUpdate(format!("Searching (Kimi): {}", query)));
        let base_url = "https://api.moonshot.ai/v1".to_string();
        let model = if self.config.model.is_empty() {
            "kimi-k2-turbo-preview".to_string()
        } else {
            self.config.model.clone()
        };

        let llm_config = LlmConfig::new(
            LlmProvider::MoonshotKimi,
            base_url,
            model,
            Some(self.config.api_key.clone()),
        );
        let client = LlmClient::new(llm_config)?;

        let web_search_tool = ChatTool {
            type_: "builtin_function".to_string(),
            function: ChatFunction {
                name: "$web_search".to_string(),
                description: None,
                parameters: None,
            },
        };

        // 1. Initial request to trigger tool call
        let mut messages = vec![
            ChatMessage::system("You are a helpful assistant with web search capabilities. When asked to search, use the $web_search tool."),
            ChatMessage::user(query),
        ];

        // 2. Handle the tool call loop (as per Kimi docs)
        for _ in 0..5 {
            // Limit turns to prevent infinite loops
            let request = ChatRequest::new(client.model().to_string(), messages.clone())
                .with_tools(vec![web_search_tool.clone()]);

            let response = client.chat(&request).await.context("Kimi API request failed")?;
            let choice = response.choices.first().context("Kimi returned no choices")?;

            if choice.finish_reason.as_deref() == Some("tool_calls") {
                if let Some(tool_calls) = &choice.message.tool_calls {
                    messages.push(choice.message.clone());
                    for tool_call in tool_calls {
                        if tool_call.function.name == "$web_search" {
                            // Echo back the arguments as the result for builtin_function.$web_search
                            messages.push(ChatMessage::tool(
                                tool_call.id.clone(),
                                tool_call.function.name.clone(),
                                tool_call.function.arguments.clone(),
                            ));
                        }
                    }
                    continue;
                }
            }

            let content = response.content().trim().to_string();
            if content.is_empty() {
                return Ok(ToolOutput::Immediate(serde_json::Value::String(
                    "No results found.".to_string(),
                )));
            }
            return Ok(ToolOutput::Immediate(serde_json::Value::String(content)));
        }

        Err(anyhow::anyhow!("Kimi search timed out (too many tool call turns)").into())
    }

    async fn call_serpapi(
        &self,
        query: &str,
    ) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let _ = self.event_tx.send(TuiEvent::StatusUpdate(format!("Searching (SerpAPI): {}", query)));
        let url = format!(
            "https://serpapi.com/search.json?q={}&api_key={}",
            urlencoding::encode(query),
            self.config.api_key
        );

        let resp_res = self.client.get(url).send().await;
        let resp = match resp_res {
            Ok(r) => match r.json::<serde_json::Value>().await {
                Ok(v) => v,
                Err(_) => {
                    return Ok(ToolOutput::Immediate(serde_json::Value::String(
                        "Error parsing response".to_string(),
                    )))
                }
            },
            Err(e) => return Err(anyhow::anyhow!("Failed to connect to SerpAPI: {}", e).into()),
        };

        if let Some(error) = resp.get("error").and_then(|e| e.as_str()) {
            return Err(anyhow::anyhow!("SerpAPI error: {}", error).into());
        }

        // Extract snippets from organic results
        let mut results = Vec::new();
        if let Some(organic) = resp.get("organic_results").and_then(|r| r.as_array()) {
            for res in organic.iter().take(5) {
                let title = res.get("title").and_then(|v| v.as_str()).unwrap_or("No Title");
                let snippet = res.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
                let link = res.get("link").and_then(|v| v.as_str()).unwrap_or("");
                results.push(format!("### {}\n{}\nSource: {}", title, snippet, link));
            }
        }

        if results.is_empty() {
            Ok(ToolOutput::Immediate(serde_json::Value::String(
                "No results found.".to_string(),
            )))
        } else {
            Ok(ToolOutput::Immediate(serde_json::Value::String(results.join("\n\n"))))
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for current information, news, or specific facts."
    }

    fn usage(&self) -> &str {
        "Provide a search query. Example: 'latest Rust version' or 'who won the Super Bowl 2025'."
    }

    fn kind(&self) -> crate::agent::tool::ToolKind {
        crate::agent::tool::ToolKind::Web
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        if !self.config.enabled {
            return Err(anyhow::anyhow!(
                "Web search is currently disabled. Please enable it in configuration."
            )
            .into());
        }

        match self.config.provider.as_str() {
            "kimi" => self.call_kimi(args).await,
            "serpapi" | "google" => self.call_serpapi(args).await,
            p => Err(anyhow::anyhow!("Unsupported web search provider: {}", p).into()),
        }
    }
}
