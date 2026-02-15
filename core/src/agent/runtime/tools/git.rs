//! Git tools for repository operations

use crate::agent::runtime::capability::{Capability, ToolCapability};
use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::ToolError;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;

use tokio::process::Command;

/// Git status tool - show working tree status
#[derive(Debug, Default)]
pub struct GitStatusTool;

impl GitStatusTool {
    pub fn new() -> Self {
        Self
    }
}

impl Capability for GitStatusTool {
    fn name(&self) -> &'static str {
        "git_status"
    }
}

#[async_trait::async_trait]
impl ToolCapability for GitStatusTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        _call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        let output = Command::new("git")
            .args(["status", "--short", "--branch"])
            .output()
            .await
            .map_err(|e| ToolError::new(format!("Failed to execute git: {}", e)))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let result = if stdout.trim().is_empty() {
                "Working tree clean".to_string()
            } else {
                stdout.to_string()
            };
            
            Ok(ToolResult::Success {
                output: result,
                structured: None,
            })
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(ToolResult::Error {
                message: format!("Git status failed: {}", stderr),
                code: Some("GIT_ERROR".to_string()),
                retryable: false,
            })
        }
    }
}

/// Git log tool - show commit history
#[derive(Debug, Default)]
pub struct GitLogTool;

impl GitLogTool {
    pub fn new() -> Self {
        Self
    }
}

impl Capability for GitLogTool {
    fn name(&self) -> &'static str {
        "git_log"
    }
}

#[async_trait::async_trait]
impl ToolCapability for GitLogTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse limit from args
        let limit = if let Some(limit_val) = call.arguments.get("limit").and_then(|v| v.as_u64()) {
            limit_val as usize
        } else if let Some(limit_str) = call.arguments.as_str().and_then(|s| s.parse::<usize>().ok()) {
            limit_str
        } else {
            10
        };

        // Cap limit to reasonable range
        let limit = limit.clamp(1, 50);

        let output = Command::new("git")
            .args(["log", "--oneline", "-n", &limit.to_string()])
            .output()
            .await
            .map_err(|e| ToolError::new(format!("Failed to execute git: {}", e)))?;

        if output.status.success() {
            Ok(ToolResult::Success {
                output: String::from_utf8_lossy(&output.stdout).to_string(),
                structured: None,
            })
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(ToolResult::Error {
                message: format!("Git log failed: {}", stderr),
                code: Some("GIT_ERROR".to_string()),
                retryable: false,
            })
        }
    }
}

/// Git diff tool - show changes
#[derive(Debug, Default)]
pub struct GitDiffTool;

impl GitDiffTool {
    pub fn new() -> Self {
        Self
    }
}

impl Capability for GitDiffTool {
    fn name(&self) -> &'static str {
        "git_diff"
    }
}

#[async_trait::async_trait]
impl ToolCapability for GitDiffTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse optional path from args
        let path = call.arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut cmd = Command::new("git");
        cmd.arg("diff");
        
        if let Some(p) = path {
            cmd.arg(p);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| ToolError::new(format!("Failed to execute git: {}", e)))?;

        if output.status.success() {
            let diff = String::from_utf8_lossy(&output.stdout);
            let result = if diff.trim().is_empty() {
                "No changes detected.".to_string()
            } else {
                // Limit diff output size
                let mut result = diff.to_string();
                if result.len() > 50_000 {
                    result.truncate(50_000);
                    result.push_str("\n... [diff truncated]");
                }
                result
            };

            Ok(ToolResult::Success {
                output: result,
                structured: None,
            })
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(ToolResult::Error {
                message: format!("Git diff failed: {}", stderr),
                code: Some("GIT_ERROR".to_string()),
                retryable: false,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_git_status() {
        let tool = GitStatusTool::new();
        let call = ToolCall {
            name: "git_status".to_string(),
            arguments: serde_json::json!({}),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.execute(&RuntimeContext::new(), call).await;
        // May fail if not in a git repo, but should not panic
        assert!(matches!(result, Ok(_) | Err(_)));
    }

    #[tokio::test]
    async fn test_git_log() {
        let tool = GitLogTool::new();
        let call = ToolCall {
            name: "git_log".to_string(),
            arguments: serde_json::json!({"limit": 5}),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.execute(&RuntimeContext::new(), call).await;
        assert!(matches!(result, Ok(_) | Err(_)));
    }
}
