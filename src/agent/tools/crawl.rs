use crate::agent::tool::Tool;
use crate::terminal::app::TuiEvent;
use anyhow::Result;
use async_trait::async_trait;
use reqwest;
use tokio::sync::mpsc;

/// A tool for retrieving and cleaning web page content.
pub struct CrawlTool {
    client: reqwest::Client,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
}

impl CrawlTool {
    pub fn new(event_tx: mpsc::UnboundedSender<TuiEvent>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("mylm-assistant/0.1")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client, event_tx }
    }
}

#[async_trait]
impl Tool for CrawlTool {
    fn name(&self) -> &str {
        "crawl"
    }

    fn description(&self) -> &str {
        "Retrieve the text content of a web page from a URL."
    }

    fn usage(&self) -> &str {
        "Provide a URL. Example: 'https://rust-lang.org'."
    }

    async fn call(&self, url: &str) -> Result<String> {
        let _ = self.event_tx.send(TuiEvent::StatusUpdate(format!("Crawling: {}", url)));
        let response = self.client.get(url).send().await?.text().await?;
        
        // Very basic HTML to text conversion (just stripping tags for now)
        // In a production app, we'd use something like 'html2md' or 'readability'
        let cleaned = response
            .split('<')
            .map(|part| {
                if let Some(pos) = part.find('>') {
                    &part[pos + 1..]
                } else {
                    part
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        // Truncate to avoid blowing up the context window
        let truncated = if cleaned.len() > 10000 {
            format!("{}... [Content Truncated]", &cleaned[..10000])
        } else {
            cleaned
        };

        Ok(truncated)
    }
}
