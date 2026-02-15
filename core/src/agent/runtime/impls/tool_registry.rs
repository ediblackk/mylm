//! Tool Registry - Dynamic tool management
//!
//! Provides a registry pattern for tools with safety checks.

use crate::agent::runtime::{
    capability::{Capability, ToolCapability},
    context::RuntimeContext,
    error::ToolError,
};
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use crate::memory::store::{VectorStore, MemoryType};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;
use tokio::fs;

/// Tool function type
type ToolFn = Arc<
    dyn Fn(&RuntimeContext, &str) -> futures::future::BoxFuture<'static, Result<ToolResult, ToolError>>
        + Send
        + Sync,
>;

/// Tool registry with safety policies
pub struct ToolRegistry {
    tools: HashMap<String, ToolFn>,
    blocked_commands: Vec<String>,
    #[allow(dead_code)]
    allowlisted_paths: Vec<std::path::PathBuf>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
            blocked_commands: vec![
                "rm -rf /".to_string(),
                "sudo".to_string(),
                "chmod 777".to_string(),
            ],
            allowlisted_paths: vec![
                std::env::current_dir().unwrap_or_default(),
                dirs::home_dir().unwrap_or_default(),
            ],
        };
        
        registry.register_defaults();
        registry
    }
    
    /// Register default tools
    fn register_defaults(&mut self) {
        self.register("shell", Arc::new(|_ctx, args| {
            let args = args.to_string();
            Box::pin(async move {
                execute_shell(&args).await
            })
        }));
        
        self.register("read_file", Arc::new(|_ctx, args| {
            let args = args.to_string();
            Box::pin(async move {
                read_file(&args).await
            })
        }));
        
        self.register("write_file", Arc::new(|_ctx, args| {
            let args = args.to_string();
            Box::pin(async move {
                write_file(&args).await
            })
        }));
        
        self.register("list_dir", Arc::new(|_ctx, args| {
            let args = args.to_string();
            Box::pin(async move {
                list_dir(&args).await
            })
        }));
        
        self.register("search", Arc::new(|_ctx, args| {
            let args = args.to_string();
            Box::pin(async move {
                search_files(&args).await
            })
        }));
        
        self.register("pwd", Arc::new(|_ctx, _args| {
            Box::pin(async move {
                let output = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| ".".to_string());
                Ok(ToolResult::Success {
                    output,
                    structured: None,
                })
            })
        }));
        
        self.register("cat", Arc::new(|_ctx, args| {
            let args = args.to_string();
            Box::pin(async move {
                read_file(&args).await
            })
        }));
        
        self.register("ls", Arc::new(|_ctx, args| {
            let args = args.to_string();
            Box::pin(async move {
                list_dir(&args).await
            })
        }));
    }
    
    /// Register a custom tool
    pub fn register(&mut self, name: impl Into<String>, func: ToolFn) {
        self.tools.insert(name.into(), func);
    }
    
    /// Enable memory tool with a VectorStore
    pub fn with_memory(mut self, store: Arc<VectorStore>) -> Self {
        self.register("memory", Arc::new(move |_ctx, args| {
            let store = store.clone();
            let args = args.to_string();
            Box::pin(async move {
                execute_memory_tool(&store, &args).await
            })
        }));
        self
    }
    
    /// Check if tool exists
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
    
    /// Execute a tool
    pub async fn execute(&self, ctx: &RuntimeContext, call: &ToolCall) -> Result<ToolResult, ToolError> {
        // Convert arguments to string for safety check
        let args_str = call.arguments.to_string();
        
        // Safety check
        if self.is_blocked(&call.name, &args_str) {
            return Ok(ToolResult::Error {
                message: format!("ERROR: Command '{}' is blocked for safety", call.name),
                code: Some("BLOCKED".to_string()),
                retryable: false,
            });
        }
        
        if let Some(tool_fn) = self.tools.get(&call.name) {
            tool_fn(ctx, &args_str).await
        } else {
            Ok(ToolResult::Error {
                message: format!("Unknown tool: {}. Available: {:?}", 
                    call.name, 
                    self.tools.keys().collect::<Vec<_>>()),
                code: Some("UNKNOWN_TOOL".to_string()),
                retryable: false,
            })
        }
    }
    
    /// Check if command is blocked
    fn is_blocked(&self, tool: &str, args: &str) -> bool {
        let command = format!("{} {}", tool, args);
        self.blocked_commands.iter().any(|blocked| command.contains(blocked))
    }
    
    /// Get available tools
    pub fn available_tools(&self) -> Vec<&String> {
        self.tools.keys().collect()
    }
    
    /// Get tool descriptions for prompt generation
    pub fn descriptions(&self) -> Vec<ToolDescription> {
        let mut descriptions = Vec::new();
        
        for name in self.tools.keys() {
            let (desc, usage): (&str, String) = match name.as_str() {
                "shell" => ("Execute shell commands", "shell <command>".to_string()),
                "read_file" | "cat" => ("Read file contents", "read_file <path>".to_string()),
                "write_file" => ("Write content to file", "write_file <path> <content>".to_string()),
                "list_dir" | "ls" => ("List directory contents", "list_files <path>".to_string()),
                "search" => ("Search for pattern in files", "search <pattern> <path>".to_string()),
                "pwd" => ("Print working directory", "pwd".to_string()),
                _ => ("Execute tool", format!("{} <args>", name)),
            };
            descriptions.push(ToolDescription {
                name: name.clone(),
                description: desc.to_string(),
                usage,
            });
        }
        
        descriptions
    }
}

/// Tool description for prompt generation
#[derive(Debug, Clone)]
pub struct ToolDescription {
    pub name: String,
    pub description: String,
    pub usage: String,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Capability for ToolRegistry {
    fn name(&self) -> &'static str {
        "tool-registry"
    }
}

#[async_trait::async_trait]
impl ToolCapability for ToolRegistry {
    async fn execute(
        &self,
        ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        self.execute(ctx, &call).await
    }
}

// ===== Individual Tool Implementations =====

async fn execute_shell(args: &str) -> Result<ToolResult, ToolError> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(ToolResult::Error {
            message: "No command provided".to_string(),
            code: Some("NO_COMMAND".to_string()),
            retryable: false,
        });
    }
    
    let command = parts[0];
    let command_args = &parts[1..];
    
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        Command::new(command)
            .args(command_args)
            .current_dir(std::env::current_dir().unwrap_or_default())
            .output()
    ).await;
    
    match output {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            
            if output.status.success() {
                Ok(ToolResult::Success {
                    output: stdout.to_string(),
                    structured: None,
                })
            } else {
                Ok(ToolResult::Error {
                    message: format!("Exit code: {:?}\nStdout: {}\nStderr: {}",
                        output.status.code(), stdout, stderr),
                    code: output.status.code().map(|c| c.to_string()),
                    retryable: false,
                })
            }
        }
        Ok(Err(e)) => Ok(ToolResult::Error {
            message: format!("Failed to execute: {}", e),
            code: Some("EXEC_ERROR".to_string()),
            retryable: true,
        }),
        Err(_) => Ok(ToolResult::Error {
            message: "Command timed out after 30 seconds".to_string(),
            code: Some("TIMEOUT".to_string()),
            retryable: true,
        }),
    }
}

async fn read_file(args: &str) -> Result<ToolResult, ToolError> {
    let path = args.trim();
    if path.is_empty() {
        return Ok(ToolResult::Error {
            message: "No file path provided".to_string(),
            code: Some("NO_PATH".to_string()),
            retryable: false,
        });
    }
    
    match fs::read_to_string(path).await {
        Ok(content) => Ok(ToolResult::Success {
            output: content,
            structured: None,
        }),
        Err(e) => Ok(ToolResult::Error {
            message: format!("Error reading file '{}': {}", path, e),
            code: Some("READ_ERROR".to_string()),
            retryable: false,
        }),
    }
}

async fn write_file(args: &str) -> Result<ToolResult, ToolError> {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return Ok(ToolResult::Error {
            message: "Usage: write_file <path> <content>".to_string(),
            code: Some("USAGE_ERROR".to_string()),
            retryable: false,
        });
    }
    
    let path = parts[0];
    let content = parts[1].trim_matches(&['"', '\''][..]);
    
    match fs::write(path, content).await {
        Ok(_) => Ok(ToolResult::Success {
            output: format!("Successfully wrote to {}", path),
            structured: None,
        }),
        Err(e) => Ok(ToolResult::Error {
            message: format!("Error writing file '{}': {}", path, e),
            code: Some("WRITE_ERROR".to_string()),
            retryable: false,
        }),
    }
}

async fn list_dir(args: &str) -> Result<ToolResult, ToolError> {
    let path = if args.trim().is_empty() { "." } else { args.trim() };
    
    match fs::read_dir(path).await {
        Ok(mut entries) => {
            let mut output = String::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                let metadata = entry.metadata().await.ok();
                let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                
                let entry_type = if is_dir { "DIR " } else { "FILE" };
                output.push_str(&format!("{} {:>10} {}\n", 
                    entry_type, 
                    if is_dir { "-".to_string() } else { format!("{}B", size) },
                    name.to_string_lossy()));
            }
            Ok(ToolResult::Success {
                output,
                structured: None,
            })
        }
        Err(e) => Ok(ToolResult::Error {
            message: format!("Error listing directory '{}': {}", path, e),
            code: Some("LIST_ERROR".to_string()),
            retryable: false,
        }),
    }
}

async fn search_files(args: &str) -> Result<ToolResult, ToolError> {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return Ok(ToolResult::Error {
            message: "Usage: search <pattern> <path>".to_string(),
            code: Some("USAGE_ERROR".to_string()),
            retryable: false,
        });
    }
    
    let pattern = parts[0];
    let path = parts[1];
    
    let output = tokio::process::Command::new("grep")
        .args(["-r", "-n", "-I", "--", pattern, path])
        .output()
        .await;
    
    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = stdout.lines().collect();
            
            if lines.is_empty() {
                Ok(ToolResult::Success {
                    output: format!("No matches found for '{}' in {}", pattern, path),
                    structured: None,
                })
            } else {
                let limited: Vec<&str> = lines.iter().take(50).cloned().collect();
                let result = if lines.len() > 50 {
                    format!("{}\n... ({} more matches)", limited.join("\n"), lines.len() - 50)
                } else {
                    limited.join("\n")
                };
                
                Ok(ToolResult::Success {
                    output: result,
                    structured: None,
                })
            }
        }
        Err(_) => {
            simple_file_search(pattern, path).await
        }
    }
}

async fn simple_file_search(pattern: &str, path: &str) -> Result<ToolResult, ToolError> {
    let mut results = Vec::new();
    
    if let Ok(mut entries) = fs::read_dir(path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let entry_path = entry.path();
            
            if entry_path.is_file() {
                if let Ok(content) = fs::read_to_string(&entry_path).await {
                    for (line_num, line) in content.lines().enumerate() {
                        if line.contains(pattern) {
                            results.push(format!("{}:{}:{}", 
                                entry_path.display(), 
                                line_num + 1,
                                line.chars().take(100).collect::<String>()));
                            if results.len() >= 50 {
                                break;
                            }
                        }
                    }
                }
            }
            
            if results.len() >= 50 {
                break;
            }
        }
    }
    
    let output = if results.is_empty() {
        format!("No matches found for '{}' in {}", pattern, path)
    } else {
        results.join("\n")
    };
    
    Ok(ToolResult::Success {
        output,
        structured: None,
    })
}


// ===== Memory Tool Implementation =====

async fn execute_memory_tool(store: &VectorStore, input: &str) -> Result<ToolResult, ToolError> {
    let input = input.trim();
    
    // Try to parse as JSON first
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(input) {
        return execute_memory_json(store, json).await;
    }
    
    // Try legacy string format: "add: content" or "search: query"
    if let Some(content) = input.strip_prefix("add:") {
        return memory_add(store, content.trim()).await;
    }
    
    if let Some(query) = input.strip_prefix("search:") {
        return memory_search(store, query.trim()).await;
    }
    
    // Try simple formats without prefixes
    if input.starts_with("add ") {
        return memory_add(store, &input[4..].trim()).await;
    }
    
    if input.starts_with("search ") {
        return memory_search(store, &input[7..].trim()).await;
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

async fn execute_memory_json(store: &VectorStore, json: serde_json::Value) -> Result<ToolResult, ToolError> {
    // Handle wrapped args
    let json = if let Some(inner) = json.get("args").or_else(|| json.get("arguments")) {
        if let Some(s) = inner.as_str() {
            match serde_json::from_str::<serde_json::Value>(s) {
                Ok(parsed) => parsed,
                Err(_) => return memory_process_raw(store, s).await,
            }
        } else {
            inner.clone()
        }
    } else {
        json
    };
    
    // Handle "add" key
    if let Some(add_val) = json.get("add") {
        if let Some(content) = add_val.as_str() {
            return memory_add(store, content).await;
        }
        // Handle structured add
        if let Some(obj) = add_val.as_object() {
            if let Some(content) = obj.get("content").and_then(|v| v.as_str()) {
                let _memory_type = obj.get("type")
                    .and_then(|v| v.as_str())
                    .map(MemoryType::from)
                    .unwrap_or(MemoryType::UserNote);
                return memory_add(store, content).await;
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
            return memory_search(store, &query).await;
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

async fn memory_process_raw(store: &VectorStore, raw: &str) -> Result<ToolResult, ToolError> {
    let raw = raw.trim();
    
    if let Some(content) = raw.strip_prefix("add:") {
        memory_add(store, content.trim()).await
    } else if let Some(query) = raw.strip_prefix("search:") {
        memory_search(store, query.trim()).await
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

async fn memory_add(store: &VectorStore, content: &str) -> Result<ToolResult, ToolError> {
    if content.is_empty() {
        return Ok(ToolResult::Error {
            message: "Memory content cannot be empty".to_string(),
            code: Some("EMPTY_CONTENT".to_string()),
            retryable: false,
        });
    }
    
    match store.add_memory(content).await {
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

async fn memory_search(store: &VectorStore, query: &str) -> Result<ToolResult, ToolError> {
    if query.is_empty() {
        return Ok(ToolResult::Error {
            message: "Search query cannot be empty".to_string(),
            code: Some("EMPTY_QUERY".to_string()),
            retryable: false,
        });
    }
    
    match store.search_memory(query, 5).await {
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
