use anyhow::Result;
use async_trait::async_trait;
use crate::agent::tool::{Tool, ToolOutput, ToolKind};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::sync::RwLock;

#[derive(Clone)]
pub struct ConsolidateTool {
    scratchpad: Arc<RwLock<String>>,
    store: Arc<crate::memory::store::VectorStore>,
}

impl ConsolidateTool {
    pub fn new(scratchpad: Arc<RwLock<String>>, store: Arc<crate::memory::store::VectorStore>) -> Self {
        Self { scratchpad, store }
    }
}

#[derive(Debug, Deserialize)]
struct ConsolidateArgs {
    new_scratchpad: String,
    memories: Vec<Value>, // Can be string or object { "content": "...", "summary": "..." }
}

#[async_trait]
impl Tool for ConsolidateTool {
    fn name(&self) -> &str {
        "consolidate_memory"
    }

    fn description(&self) -> &str {
        "Save important facts to long-term memory and condense the scratchpad to free up space."
    }

    fn usage(&self) -> &str {
        r#"
        {
            "new_scratchpad": "New condensed content for the scratchpad...",
            "memories": [
                "Fact 1 to remember",
                { "content": "Fact 2 with details...", "summary": "Fact 2" }
            ]
        }
        "#
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "new_scratchpad": {
                    "type": "string",
                    "description": "The new, condensed content for the scratchpad."
                },
                "memories": {
                    "type": "array",
                    "description": "List of memories to save to long-term storage.",
                    "items": {
                        "anyOf": [
                            { "type": "string" },
                            { 
                                "type": "object",
                                "properties": {
                                    "content": { "type": "string" },
                                    "summary": { "type": "string" }
                                },
                                "required": ["content"]
                            }
                        ]
                    }
                }
            },
            "required": ["new_scratchpad", "memories"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn std::error::Error + Send + Sync>> {
        let args: ConsolidateArgs = serde_json::from_str(args)?;
        
        let mut added_count = 0;
        
        for mem in args.memories {
            if let Some(content) = mem.as_str() {
                self.store.add_memory(content).await?;
                added_count += 1;
            } else if let Some(obj) = mem.as_object() {
                if let Some(content) = obj.get("content").and_then(|c| c.as_str()) {
                    let summary = obj.get("summary").and_then(|s| s.as_str()).map(|s| s.to_string());
                     self.store.add_memory_typed(
                        content, 
                        crate::memory::store::MemoryType::UserNote, 
                        None, 
                        None, 
                        None, 
                        summary
                    ).await?;
                    added_count += 1;
                }
            }
        }

        // Update scratchpad
        {
            let mut scratchpad = self.scratchpad.write().map_err(|_| anyhow::anyhow!("Scratchpad lock poisoned"))?;
            *scratchpad = args.new_scratchpad;
        }

        Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
            "Consolidation complete. Saved {} memories and updated scratchpad.",
            added_count
        ))))
    }
    
    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}
