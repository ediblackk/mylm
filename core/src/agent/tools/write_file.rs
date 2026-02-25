//! Write File Tool - Write content to files
//!
//! Provides file writing capabilities with automatic directory creation.

use crate::agent::runtime::core::{Capability, RuntimeContext, ToolCapability, ToolError};
use crate::agent::types::events::ToolResult;
use crate::agent::types::intents::ToolCall;
use crate::agent::tools::expand_tilde;
use serde::Deserialize;
use std::path::Path;

/// Tool for writing files
#[derive(Debug, Default)]
pub struct WriteFileTool;

impl WriteFileTool {
    /// Create a new write file tool
    pub fn new() -> Self {
        Self
    }
    
    /// Write content to a file
    /// 
    /// Creates parent directories if they don't exist.
    async fn write_file(&self, path: &str, content: &str) -> Result<ToolResult, ToolError> {
        let path = expand_tilde(path);
        let path = Path::new(&path);
        
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return Ok(ToolResult::Error {
                    message: format!("Cannot create directory: {}", e),
                    code: Some("MKDIR_ERROR".to_string()),
                    retryable: false,
                });
            }
        }
        
        // Write file
        match tokio::fs::write(path, content).await {
            Ok(()) => Ok(ToolResult::Success {
                output: format!("File written successfully: {}", path.display()),
                structured: Some(serde_json::json!({
                    "path": path.to_string_lossy(),
                    "bytes_written": content.len(),
                })),
            }),
            Err(e) => Ok(ToolResult::Error {
                message: format!("Error writing file: {}", e),
                code: Some("WRITE_ERROR".to_string()),
                retryable: false,
            }),
        }
    }
}

impl Capability for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }
}

/// Arguments for write_file tool
#[derive(Debug, Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[async_trait::async_trait]
impl ToolCapability for WriteFileTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Try to parse as structured JSON first
        if let Ok(args) = serde_json::from_value::<WriteArgs>(call.arguments.clone()) {
            return self.write_file(&args.path, &args.content).await;
        }
        
        // Fall back to positional args: path content
        let args_str = call.arguments.as_str()
            .ok_or_else(|| ToolError::new(
                "Expected '{\"path\": \"...\", \"content\": \"...\"}' or string arguments"
            ))?;
        
        // Try to parse as simple "path content" format
        let parts: Vec<&str> = args_str.splitn(2, ' ').collect();
        if parts.len() != 2 {
            return Err(ToolError::new(
                "Expected 'path content' format with space separator"
            ));
        }
        
        self.write_file(parts[0], parts[1]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_write_file_structured() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("output.txt");
        
        let tool = WriteFileTool::new();
        let call = ToolCall::new("write_file", serde_json::json!({
            "path": file_path.to_str().unwrap(),
            "content": "test content"
        }));
        
        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
        
        match result {
            ToolResult::Success { output, .. } => {
                assert!(output.contains("written successfully"));
            }
            _ => panic!("Expected success"),
        }
        
        // Verify file was written
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "test content");
    }
    
    #[tokio::test]
    async fn test_write_file_creates_directories() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("nested").join("dirs").join("file.txt");
        
        let tool = WriteFileTool::new();
        let call = ToolCall::new("write_file", serde_json::json!({
            "path": file_path.to_str().unwrap(),
            "content": "nested content"
        }));
        
        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
        
        match result {
            ToolResult::Success { .. } => {}
            _ => panic!("Expected success"),
        }
        
        assert!(file_path.exists());
    }
    
    #[tokio::test]
    async fn test_write_file_overwrite() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("existing.txt");
        
        tokio::fs::write(&file_path, "old content").await.unwrap();
        
        let tool = WriteFileTool::new();
        let call = ToolCall::new("write_file", serde_json::json!({
            "path": file_path.to_str().unwrap(),
            "content": "new content"
        }));
        
        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
        
        match result {
            ToolResult::Success { .. } => {}
            _ => panic!("Expected success"),
        }
        
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "new content");
    }
    
    #[tokio::test]
    async fn test_write_file_empty_content() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("empty.txt");
        
        let tool = WriteFileTool::new();
        let call = ToolCall::new("write_file", serde_json::json!({
            "path": file_path.to_str().unwrap(),
            "content": ""
        }));
        
        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
        
        match result {
            ToolResult::Success { .. } => {}
            _ => panic!("Expected success"),
        }
        
        assert!(file_path.exists());
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(content.is_empty());
    }
}
