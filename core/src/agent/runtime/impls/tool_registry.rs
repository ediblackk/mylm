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
