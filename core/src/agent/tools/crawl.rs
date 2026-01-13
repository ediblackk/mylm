use crate::agent::tool::Tool;
use crate::terminal::app::TuiEvent;
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// A tool for crawling websites.
pub struct CrawlTool {
    event_tx: mpsc::UnboundedSender<TuiEvent>,
}

impl CrawlTool {
    pub fn new(event_tx: mpsc::UnboundedSender<TuiEvent>) -> Self {
        Self { event_tx }
    }
}

#[async_trait]
impl Tool for CrawlTool {
    fn name(&self) -> &str {
        "crawl"
    }

    fn description(&self) -> &str {
        "Crawl a website to extract its content for analysis."
    }

    fn usage(&self) -> &str {
        "Pass the URL of the website to crawl. Example: 'https://rust-lang.org'."
    }

    fn kind(&self) -> crate::agent::tool::ToolKind {
        crate::agent::tool::ToolKind::Web
    }

    async fn call(&self, url: &str) -> Result<String> {
        let _ = self.event_tx.send(TuiEvent::StatusUpdate(format!("Crawling: {}", url)));

        let response = reqwest::get(url).await?;
        let body = response.text().await?;

        // Simple text extraction (heuristic)
        let text = body.lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(text)
    }
}
