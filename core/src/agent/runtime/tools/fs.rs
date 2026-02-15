//! Filesystem tools - read and write files

use crate::agent::runtime::capability::{Capability, ToolCapability};
use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::ToolError;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use serde::Deserialize;
use std::path::Path;

const MAX_FILE_SIZE: usize = 10_000_000; // 10MB max read

/// Tool for reading files
#[derive(Debug, Default)]
pub struct ReadFileTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self
    }

    async fn read_file(&self, path: &str) -> Result<ToolResult, ToolError> {
        let path = Path::new(path);
        
        // Validate path
        if path.is_dir() {
            return Ok(ToolResult::Error {
                message: format!("'{}' is a directory, not a file", path.display()),
                code: Some("IS_DIRECTORY".to_string()),
                retryable: false,
            });
        }

        // Check file size before reading
        match tokio::fs::metadata(path).await {
            Ok(metadata) => {
                if metadata.len() > MAX_FILE_SIZE as u64 {
                    return Ok(ToolResult::Error {
                        message: format!(
                            "File too large: {} bytes (max {})",
                            metadata.len(),
                            MAX_FILE_SIZE
                        ),
                        code: Some("FILE_TOO_LARGE".to_string()),
                        retryable: false,
                    });
                }
            }
            Err(e) => {
                return Ok(ToolResult::Error {
                    message: format!("Cannot access file: {}", e),
                    code: Some("ACCESS_ERROR".to_string()),
                    retryable: false,
                });
            }
        }

        // Read file
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
}

impl Capability for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }
}

#[async_trait::async_trait]
impl ToolCapability for ReadFileTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        let path = call.arguments.as_str()
            .map(|s| s.to_string())
            .or_else(|| {
                call.arguments.get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .ok_or_else(|| ToolError::new(
                "Expected path string or {\"path\": \"...\"}"
            ))?;

        self.read_file(&path).await
    }
}

/// Tool for writing files
#[derive(Debug, Default)]
pub struct WriteFileTool;

impl WriteFileTool {
    pub fn new() -> Self {
        Self
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<ToolResult, ToolError> {
        let path = Path::new(path);
        
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
                structured: None,
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
                "Expected 'path content' or {\"path\": \"...\", \"content\": \"...\"}"
            ))?;

        let parts: Vec<&str> = args_str.splitn(2, ' ').collect();
        if parts.len() != 2 {
            return Err(ToolError::new(
                "Expected 'path content' format"
            ));
        }

        self.write_file(parts[0], parts[1]).await
    }
}

#[derive(Debug, Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.txt");
        tokio::fs::write(&file_path, "hello world").await.unwrap();

        let tool = ReadFileTool::new();
        let call = ToolCall {
            name: "read_file".to_string(),
            arguments: serde_json::json!(file_path.to_str().unwrap()),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.execute(&RuntimeContext::new(), call).await;
        assert!(result.is_ok());
        
        match result.unwrap() {
            ToolResult::Success { output, .. } => {
                assert_eq!(output, "hello world");
            }
            _ => panic!("Expected success"),
        }
    }

    #[tokio::test]
    async fn test_write_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("output.txt");

        let tool = WriteFileTool::new();
        let call = ToolCall {
            name: "write_file".to_string(),
            arguments: serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "content": "test content"
            }),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.execute(&RuntimeContext::new(), call).await;
        assert!(result.is_ok());

        // Verify file was written
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "test content");
    }
}
