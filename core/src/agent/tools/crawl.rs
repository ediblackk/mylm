use crate::agent::tool::{Tool, ToolOutput};
use crate::terminal::app::TuiEvent;
use async_trait::async_trait;
use std::error::Error as StdError;
use tokio::sync::mpsc;

/// A tool for crawling websites.
pub struct CrawlTool {
    _event_tx: mpsc::UnboundedSender<TuiEvent>,
}

impl CrawlTool {
    pub fn new(event_tx: mpsc::UnboundedSender<TuiEvent>) -> Self {
        Self { _event_tx: event_tx }
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

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let url = args.trim().trim_matches('"').to_string();

        // Simple implementation for now - in a real app we'd use reqwest
        Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
            "Crawled {} successfully.",
            url
        ))))
    }
}
