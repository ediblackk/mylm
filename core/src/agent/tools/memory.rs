use crate::agent::tool::{Tool, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use std::error::Error as StdError;
use std::sync::Arc;

/// A tool for interacting with the long-term vector memory.
pub struct MemoryTool {
    store: Arc<crate::memory::store::VectorStore>,
}

impl MemoryTool {
    /// Create a new MemoryTool
    pub fn new(store: Arc<crate::memory::store::VectorStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Interact with long-term memory. You can 'add' new information or 'search' for existing context."
    }

    fn usage(&self) -> &str {
        "Use 'add: <content>' to remember something, or 'search: <query>' to find something. IMPORTANT: Do not use any other format. Example: 'add: The user likes Rust' or 'search: what are the user's preferences?'"
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let args = args.trim();

        // Helper to extract content from JSON wrapper
        fn extract_from_json(args: &str) -> String {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
                // Check for wrapper keys like "args" or "arguments"
                if let Some(inner) = v.get("args").or_else(|| v.get("arguments")) {
                    if let Some(s) = inner.as_str() {
                        return extract_from_json(s);
                    }
                }

                // Check for direct command keys
                if let Some(content) = v.get("add").and_then(|s| s.as_str()) {
                    return format!("add: {}", content);
                }
                if let Some(query) = v.get("search").and_then(|s| s.as_str()) {
                    return format!("search: {}", query);
                }
                if let Some(query) = v.get("query").and_then(|s| s.as_str()) {
                    return format!("search: {}", query);
                }
            }
            args.to_string()
        }

        let cleaned_args = if args.starts_with('{') {
            extract_from_json(args)
        } else {
            args.to_string()
        };

        let args = cleaned_args.as_str();

        if let Some(content) = args.strip_prefix("add:") {
            let content = content.trim();
            if content.is_empty() {
                return Err(anyhow::anyhow!("Content for 'add' cannot be empty").into());
            }
            self.store.add_memory(content).await?;
            Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Successfully added to memory: '{}'",
                content
            ))))
        } else if let Some(query) = args.strip_prefix("search:") {
            let query = query.trim();
            if query.is_empty() {
                return Err(anyhow::anyhow!("Query for 'search' cannot be empty").into());
            }
            let results = self.store.search_memory(query, 5).await?;
            if results.is_empty() {
                Ok(ToolOutput::Immediate(serde_json::Value::String(
                    "No relevant memories found.".to_string(),
                )))
            } else {
                let mut output = String::from("Found relevant memories:\n");
                for (i, res) in results.iter().enumerate() {
                    output.push_str(&format!("{}. {}\n", i + 1, res));
                }
                Ok(ToolOutput::Immediate(serde_json::Value::String(output)))
            }
        } else {
            Err(anyhow::anyhow!("Invalid memory command. Use 'add: <content>' or 'search: <query>'. You sent: '{}'", args).into())
        }
    }
}
