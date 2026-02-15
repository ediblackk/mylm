//! Memory Tool
//!
//! Provides agent access to long-term memory storage.
//!
//! # Usage
//!
//! Add a memory:
//! - `memory("add: User prefers dark mode")`
//! - `memory({"add": "User prefers dark mode"})`
//!
//! Search memories:
//! - `memory("search: dark mode preference")`
//! - `memory({"search": "dark mode"})`

use std::sync::Arc;
use crate::agent::runtime::capability::{Capability, ToolCapability};
use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::ToolError;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use crate::memory::store::{VectorStore, MemoryType};
use serde::Deserialize;

/// Memory tool for storing and retrieving memories
pub struct MemoryTool {
    store: Arc<VectorStore>,
}

impl MemoryTool {
    /// Create a new memory tool
    pub fn new(store: Arc<VectorStore>) -> Self {
        Self { store }
    }
    
    /// Parse and execute memory command
    async fn execute_command(&self, input: &str) -> Result<ToolResult, ToolError> {
        let input = input.trim();
        
        // Try to parse as JSON first
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(input) {
            return self.process_json(json).await;
        }
        
        // Try legacy string format: "add: content" or "search: query"
        if let Some(content) = input.strip_prefix("add:") {
            return self.add_memory(content.trim()).await;
        }
        
        if let Some(query) = input.strip_prefix("search:") {
            return self.search_memories(query.trim()).await;
        }
        
        // Try simple formats without prefixes
        if input.starts_with("add ") {
            return self.add_memory(&input[4..].trim()).await;
        }
        
        if input.starts_with("search ") {
            return self.search_memories(&input[7..].trim()).await;
        }
        
        Ok(ToolResult::Error {
            message: format!(
                "Invalid memory command. Use:\n\
                - 'add: <content>' or {{\"add\": \"<content>\"}} to save\n\
                - 'search: <query>' or {{\"search\": \"<query>\"}} to find\n\
                You sent: '{}'",
                input
            ),
            code: Some("INVALID_COMMAND".to_string()),
            retryable: false,
        })
    }
    
    /// Process JSON format (non-recursive)
    async fn process_json(&self, json: serde_json::Value) -> Result<ToolResult, ToolError> {
        // Handle wrapped args (from some LLM outputs)
        let json = if let Some(inner) = json.get("args").or_else(|| json.get("arguments")) {
            if let Some(s) = inner.as_str() {
                // Try to parse inner as JSON, otherwise treat as raw command
                match serde_json::from_str::<serde_json::Value>(s) {
                    Ok(parsed) => parsed,
                    Err(_) => return self.process_raw_command(s).await,
                }
            } else {
                inner.clone()
            }
        } else {
            json
        };
        
        self.process_parsed_json(json).await
    }
    
    /// Process a raw command string (non-recursive)
    async fn process_raw_command(&self, raw: &str) -> Result<ToolResult, ToolError> {
        let raw = raw.trim();
        
        if let Some(content) = raw.strip_prefix("add:") {
            self.add_memory(content.trim()).await
        } else if let Some(query) = raw.strip_prefix("search:") {
            self.search_memories(query.trim()).await
        } else {
            Ok(ToolResult::Error {
                message: format!(
                    "Invalid wrapped command. Expected 'add:' or 'search:' prefix. Got: '{}'",
                    raw
                ),
                code: Some("INVALID_WRAPPED_COMMAND".to_string()),
                retryable: false,
            })
        }
    }
    
    /// Process already-parsed JSON (non-recursive)
    async fn process_parsed_json(&self, json: serde_json::Value) -> Result<ToolResult, ToolError> {
        // Handle "add" key
        if let Some(add_val) = json.get("add") {
            if let Some(content) = add_val.as_str() {
                return self.add_memory(content).await;
            }
            
            // Handle structured add: {"add": {"content": "...", "type": "..."}}
            if let Some(obj) = add_val.as_object() {
                if let Some(content) = obj.get("content").and_then(|v| v.as_str()) {
                    let memory_type = obj.get("type")
                        .and_then(|v| v.as_str())
                        .map(MemoryType::from)
                        .unwrap_or(MemoryType::UserNote);
                    return self.add_memory_typed(content, memory_type).await;
                }
            }
        }
        
        // Handle "search" or "query" key
        let search_val = json.get("search").or_else(|| json.get("query"));
        if let Some(search_val) = search_val {
            let query = if let Some(q) = search_val.as_str() {
                q.to_string()
            } else if let Some(obj) = search_val.as_object() {
                obj.get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            };
            
            if !query.is_empty() {
                return self.search_memories(&query).await;
            }
        }
        
        Ok(ToolResult::Error {
            message: format!(
                "Invalid JSON format. Expected {{\"add\": \"content\"}} or {{\"search\": \"query\"}}. Got: {}",
                json
            ),
            code: Some("INVALID_JSON".to_string()),
            retryable: false,
        })
    }
    
    /// Add a memory to the store
    async fn add_memory(&self, content: &str) -> Result<ToolResult, ToolError> {
        if content.is_empty() {
            return Ok(ToolResult::Error {
                message: "Memory content cannot be empty".to_string(),
                code: Some("EMPTY_CONTENT".to_string()),
                retryable: false,
            });
        }
        
        match self.store.add_memory(content).await {
            Ok(_) => Ok(ToolResult::Success {
                output: format!("✓ Memory saved: '{}'", content.chars().take(50).collect::<String>()),
                structured: Some(serde_json::json!({
                    "action": "add",
                    "content": content,
                    "status": "success"
                })),
            }),
            Err(e) => Ok(ToolResult::Error {
                message: format!("Failed to save memory: {}", e),
                code: Some("STORE_ERROR".to_string()),
                retryable: true,
            }),
        }
    }
    
    /// Add a typed memory
    async fn add_memory_typed(
        &self,
        content: &str,
        memory_type: MemoryType,
    ) -> Result<ToolResult, ToolError> {
        if content.is_empty() {
            return Ok(ToolResult::Error {
                message: "Memory content cannot be empty".to_string(),
                code: Some("EMPTY_CONTENT".to_string()),
                retryable: false,
            });
        }
        
        let type_str = format!("{:?}", memory_type);
        match self.store.add_memory_typed(content, memory_type, None, None, None, None).await {
            Ok(_) => Ok(ToolResult::Success {
                output: format!("✓ Memory saved as {}", type_str),
                structured: Some(serde_json::json!({
                    "action": "add",
                    "type": type_str,
                    "content": content,
                    "status": "success"
                })),
            }),
            Err(e) => Ok(ToolResult::Error {
                message: format!("Failed to save memory: {}", e),
                code: Some("STORE_ERROR".to_string()),
                retryable: true,
            }),
        }
    }
    
    /// Search memories
    async fn search_memories(&self, query: &str) -> Result<ToolResult, ToolError> {
        if query.is_empty() {
            return Ok(ToolResult::Error {
                message: "Search query cannot be empty".to_string(),
                code: Some("EMPTY_QUERY".to_string()),
                retryable: false,
            });
        }
        
        match self.store.search_memory(query, 5).await {
            Ok(memories) => {
                if memories.is_empty() {
                    Ok(ToolResult::Success {
                        output: "No relevant memories found.".to_string(),
                        structured: Some(serde_json::json!({
                            "action": "search",
                            "query": query,
                            "results": [],
                            "count": 0
                        })),
                    })
                } else {
                    let mut output = format!("Found {} relevant memories:\n\n", memories.len());
                    for (i, mem) in memories.iter().enumerate() {
                        let timestamp = chrono::DateTime::from_timestamp(mem.created_at, 0)
                            .map(|dt| dt.format("%Y-%m-%d").to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        
                        output.push_str(&format!(
                            "{}. [{} | {}] {}\n",
                            i + 1,
                            mem.r#type,
                            timestamp,
                            mem.content.lines().next().unwrap_or(&mem.content)
                        ));
                    }
                    
                    let results_json: Vec<_> = memories.iter().map(|m| {
                        serde_json::json!({
                            "id": m.id,
                            "type": format!("{:?}", m.r#type),
                            "content": m.content,
                            "created_at": m.created_at,
                        })
                    }).collect();
                    
                    Ok(ToolResult::Success {
                        output,
                        structured: Some(serde_json::json!({
                            "action": "search",
                            "query": query,
                            "results": results_json,
                            "count": memories.len()
                        })),
                    })
                }
            }
            Err(e) => Ok(ToolResult::Error {
                message: format!("Failed to search memories: {}", e),
                code: Some("SEARCH_ERROR".to_string()),
                retryable: true,
            }),
        }
    }
}

impl Capability for MemoryTool {
    fn name(&self) -> &'static str {
        "memory"
    }
}

#[async_trait::async_trait]
impl ToolCapability for MemoryTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Extract arguments - can be string or JSON object
        let args_str = if let Some(s) = call.arguments.as_str() {
            s.to_string()
        } else {
            call.arguments.to_string()
        };
        
        self.execute_command(&args_str).await
    }
}

/// Arguments for memory operations
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum MemoryArgs {
    /// Simple string format
    Simple(String),
    /// JSON object format
    Structured {
        #[serde(alias = "add")]
        add: Option<String>,
        #[serde(alias = "search")]
        search: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    
    // Note: These tests would need a real VectorStore instance
    // For unit tests, we'd typically mock the VectorStore
    
    #[test]
    fn test_memory_tool_name() {
        // Can't easily test without VectorStore, but we verify the structure compiles
    }
}
