//! List directory contents tool

use crate::agent::runtime::capability::{Capability, ToolCapability};
use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::ToolError;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;

use std::path::Path;

/// Tool for listing directory contents
#[derive(Debug, Default)]
pub struct ListFilesTool;

impl ListFilesTool {
    pub fn new() -> Self {
        Self
    }

    async fn list_files(&self, path: &str) -> Result<ToolResult, ToolError> {
        let path = if path.is_empty() { "." } else { path };
        let dir_path = Path::new(path);

        // Validate it's a directory
        if !dir_path.is_dir() {
            return Ok(ToolResult::Error {
                message: format!("'{}' is not a directory", path),
                code: Some("NOT_A_DIRECTORY".to_string()),
                retryable: false,
            });
        }

        match tokio::fs::read_dir(dir_path).await {
            Ok(mut entries) => {
                let mut files = Vec::new();
                let mut dirs = Vec::new();

                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    
                    let metadata = entry.metadata().await.ok();
                    let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                    let size = metadata.as_ref().map(|m| m.len());

                    if is_dir {
                        dirs.push(format!("{}/", name_str));
                    } else {
                        if let Some(s) = size {
                            files.push(format!("{} ({})", name_str, format_size(s)));
                        } else {
                            files.push(name_str.to_string());
                        }
                    }
                }

                // Sort directories first, then files
                dirs.sort();
                files.sort();

                let mut output = String::new();
                
                if !dirs.is_empty() {
                    output.push_str("Directories:\n");
                    for d in &dirs {
                        output.push_str(&format!("  {}\n", d));
                    }
                }

                if !files.is_empty() {
                    if !dirs.is_empty() {
                        output.push('\n');
                    }
                    output.push_str("Files:\n");
                    for f in files {
                        output.push_str(&format!("  {}\n", f));
                    }
                }

                if output.is_empty() {
                    output = "(empty directory)".to_string();
                }

                Ok(ToolResult::Success {
                    output,
                    structured: None,
                })
            }
            Err(e) => Ok(ToolResult::Error {
                message: format!("Error reading directory: {}", e),
                code: Some("READ_ERROR".to_string()),
                retryable: false,
            }),
        }
    }
}

impl Capability for ListFilesTool {
    fn name(&self) -> &'static str {
        "list_files"
    }
}

#[async_trait::async_trait]
impl ToolCapability for ListFilesTool {
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
            .unwrap_or_default();

        self.list_files(&path).await
    }
}

/// Format file size in human-readable form
fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = size as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{:.0} {}", size, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_list_files() {
        let temp = TempDir::new().unwrap();
        
        // Create test files
        tokio::fs::write(temp.path().join("file1.txt"), "content1").await.unwrap();
        tokio::fs::write(temp.path().join("file2.txt"), "content2").await.unwrap();
        tokio::fs::create_dir(temp.path().join("subdir")).await.unwrap();

        let tool = ListFilesTool::new();
        let call = ToolCall {
            name: "list_files".to_string(),
            arguments: serde_json::json!(temp.path().to_str().unwrap()),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.execute(&RuntimeContext::new(), call).await;
        assert!(result.is_ok());
        
        match result.unwrap() {
            ToolResult::Success { output, .. } => {
                assert!(output.contains("file1.txt"));
                assert!(output.contains("file2.txt"));
                assert!(output.contains("subdir/"));
            }
            _ => panic!("Expected success"),
        }
    }

    #[tokio::test]
    async fn test_format_size() {
        assert_eq!(format_size(100), "100 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
    }
}
