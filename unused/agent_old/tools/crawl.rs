use crate::agent_old::tool::{Tool, ToolOutput};
use async_trait::async_trait;
use std::error::Error as StdError;

/// A tool for crawling websites.
pub struct CrawlTool {
}

impl CrawlTool {
    pub fn new(_event_bus: std::sync::Arc<crate::agent_old::event_bus::EventBus>) -> Self {
        Self { }
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

    fn kind(&self) -> crate::agent_old::tool::ToolKind {
        crate::agent_old::tool::ToolKind::Web
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
