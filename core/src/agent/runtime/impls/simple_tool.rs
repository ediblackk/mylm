//! Simple tool executor for common commands
//!
//! Executes shell commands, file operations, etc.

use crate::agent::runtime::{
    capability::{Capability, ToolCapability},
    context::RuntimeContext,
    error::ToolError,
};
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;


/// Simple tool executor for shell commands
pub struct SimpleToolExecutor;

impl SimpleToolExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SimpleToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl Capability for SimpleToolExecutor {
    fn name(&self) -> &'static str {
        "simple-tools"
    }
}

#[async_trait::async_trait]
impl ToolCapability for SimpleToolExecutor {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        match call.name.as_str() {
            "shell" => execute_shell(&call.arguments.to_string()).await,
            "cat" | "read_file" => read_file(&call.arguments.to_string()).await,
            "ls" | "list_dir" => list_dir(&call.arguments.to_string()).await,
            "pwd" => Ok(ToolResult::Success {
                output: std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| ".".to_string()),
                structured: None,
            }),
            _ => {
                Ok(ToolResult::Error {
                    message: format!("Unknown tool: {}", call.name),
                    code: Some("UNKNOWN_TOOL".to_string()),
                    retryable: false,
                })
            }
        }
    }
}

async fn execute_shell(args: &str) -> Result<ToolResult, ToolError> {
    // SECURITY: This should check approval policy before execution
    // For now, we return what WOULD be executed
    Ok(ToolResult::Success {
        output: format!("Would execute: {}", args),
        structured: None,
    })
}

async fn read_file(path: &str) -> Result<ToolResult, ToolError> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => Ok(ToolResult::Success {
            output: content,
            structured: None,
        }),
        Err(e) => Ok(ToolResult::Error {
            message: format!("Error reading file: {}", e),
            code: Some("READ_ERROR".to_string()),
            retryable: false,
        }),
    }
}

async fn list_dir(path: &str) -> Result<ToolResult, ToolError> {
    let path = if path.is_empty() { "." } else { path };
    
    match tokio::fs::read_dir(path).await {
        Ok(mut entries) => {
            let mut output = String::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                output.push_str(&format!("{}\n", name.to_string_lossy()));
            }
            Ok(ToolResult::Success {
                output,
                structured: None,
            })
        }
        Err(e) => Ok(ToolResult::Error {
            message: format!("Error listing directory: {}", e),
            code: Some("LIST_ERROR".to_string()),
            retryable: false,
        }),
    }
}
