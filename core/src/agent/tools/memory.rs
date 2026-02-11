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
        r#"Add to memory: "add: your content here" or {"add": "your content"}
Search memory: "search: your query" or {"search": "your query"}

Examples:
  memory("add: User prefers dark mode")
  memory({"add": "API key is in ~/.config/api"})
  memory("search: dark mode preference")
  memory({"search": "API key location"})"#
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let args = args.trim();

        // Try to parse args as JSON first
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
            // Check for wrapped args (e.g. from some LLM outputs like {"args": "search: ..."})
            let root = if let Some(inner) = v.get("args").or_else(|| v.get("arguments")) {
                if let Some(s) = inner.as_str() {
                    // Try to parse inner as JSON, if that fails, treat it as a command string
                    serde_json::from_str(s).unwrap_or_else(|_| {
                        // Inner is not JSON - could be "search: ..." or "add: ..."
                        // Return it as a JSON string value that we can check later
                        serde_json::json!({"_raw_command": s})
                    })
                } else {
                    inner.clone()
                }
            } else {
                v
            };

            // Handle raw command from wrapped args (e.g., {"args": "search: memory"})
            if let Some(cmd) = root.get("_raw_command").and_then(|c| c.as_str()) {
                let cmd = cmd.trim();
                if let Some(query) = cmd.strip_prefix("search:") {
                    let query = query.trim();
                    if !query.is_empty() {
                        let results = self.store.search_memory(query, 5).await?;
                        if results.is_empty() {
                            return Ok(ToolOutput::Immediate(serde_json::Value::String(
                                "No relevant memories found.".to_string(),
                            )));
                        } else {
                            let mut output = String::from("Found relevant memories:\n");
                            for (i, res) in results.iter().enumerate() {
                                output.push_str(&format!("{}. {}\n", i + 1, res));
                            }
                            return Ok(ToolOutput::Immediate(serde_json::Value::String(output)));
                        }
                    }
                } else if let Some(content) = cmd.strip_prefix("add:") {
                    let content = content.trim();
                    if !content.is_empty() {
                        self.store.add_memory(content).await?;
                        return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                            "Successfully added to memory: '{}'",
                            content
                        ))));
                    }
                }
            }

            // Case 1: "add" key
            if let Some(add_val) = root.get("add") {
                if let Some(content) = add_val.as_str() {
                    // add: "content"
                    self.store.add_memory(content).await?;
                    return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                        "Successfully added to memory: '{}'",
                        content
                    ))));
                } else if let Some(obj) = add_val.as_object() {
                    // add: { "content": "...", "summary": "..." }
                    if let Some(content) = obj.get("content").and_then(|c| c.as_str()) {
                        let summary = obj.get("summary").and_then(|s| s.as_str()).map(|s| s.to_string());
                        self.store.add_memory_typed(
                            content, 
                            crate::memory::store::MemoryType::UserNote, 
                            None, 
                            None, 
                            None, 
                            summary.clone()
                        ).await?;
                        return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                            "Successfully added to memory (with summary '{}'): '{}'",
                            summary.unwrap_or_default(),
                            content
                        ))));
                    }
                }
            }

            // Case 2: "search" or "query" key
            let search_val = root.get("search").or_else(|| root.get("query"));
            if let Some(search_val) = search_val {
                let query = if let Some(q) = search_val.as_str() {
                    q.to_string()
                } else if let Some(obj) = search_val.as_object() {
                    obj.get("query").and_then(|q| q.as_str()).unwrap_or("").to_string()
                } else {
                    String::new()
                };

                if !query.is_empty() {
                    let results = self.store.search_memory(&query, 5).await?;
                    if results.is_empty() {
                        return Ok(ToolOutput::Immediate(serde_json::Value::String(
                            "No relevant memories found.".to_string(),
                        )));
                    } else {
                        let mut output = String::from("Found relevant memories:\n");
                        for (i, res) in results.iter().enumerate() {
                            output.push_str(&format!("{}. {}\n", i + 1, res));
                        }
                        return Ok(ToolOutput::Immediate(serde_json::Value::String(output)));
                    }
                }
            }
        }

        // Fallback: Legacy string parsing
        if let Some(content) = args.strip_prefix("add:") {
            let content = content.trim();
            if content.is_empty() {
                return Err(anyhow::anyhow!("Content for 'add' cannot be empty").into());
            }
            // For legacy add, we just use content (summary=None)
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
