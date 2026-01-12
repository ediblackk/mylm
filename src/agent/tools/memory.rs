use crate::agent::tool::Tool;
use crate::memory::VectorStore;
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::sync::Arc;

/// A tool for interacting with the long-term vector memory.
pub struct MemoryTool {
    store: Arc<VectorStore>,
}

impl MemoryTool {
    /// Create a new MemoryTool
    pub fn new(store: Arc<VectorStore>) -> Self {
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

    async fn call(&self, args: &str) -> Result<String> {
        let args = args.trim();
        
        if let Some(content) = args.strip_prefix("add:") {
            let content = content.trim();
            if content.is_empty() {
                bail!("Content for 'add' cannot be empty");
            }
            self.store.add_memory(content).await?;
            Ok(format!("Successfully added to memory: '{}'", content))
        } else if let Some(query) = args.strip_prefix("search:") {
            let query = query.trim();
            if query.is_empty() {
                bail!("Query for 'search' cannot be empty");
            }
            let results = self.store.search_memory(query, 5).await?;
            if results.is_empty() {
                Ok("No relevant memories found.".to_string())
            } else {
                let mut output = String::from("Found relevant memories:\n");
                for (i, res) in results.iter().enumerate() {
                    output.push_str(&format!("{}. {}\n", i + 1, res));
                }
                Ok(output)
            }
        } else {
            bail!("Invalid memory command. Use 'add: <content>' or 'search: <query>'");
        }
    }
}
