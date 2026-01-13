use crate::agent::tool::{Tool, ToolKind};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use tokio::process::Command;

/// A tool for checking the git status.
pub struct GitStatusTool;

#[async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }

    fn description(&self) -> &str {
        "Show the working tree status in short format with branch information."
    }

    fn usage(&self) -> &str {
        "Takes no arguments or empty JSON object."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn call(&self, _args: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["status", "--short", "--branch"])
            .output()
            .await?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Ok(format!(
                "Error executing git status: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}

/// A tool for viewing git commit history.
pub struct GitLogTool;

#[derive(Deserialize)]
struct LogArgs {
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

#[async_trait]
impl Tool for GitLogTool {
    fn name(&self) -> &str {
        "git_log"
    }

    fn description(&self) -> &str {
        "Show the git commit logs."
    }

    fn usage(&self) -> &str {
        "Provide a limit for the number of commits to show. Example: { \"limit\": 5 }"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "The number of commits to show (default: 10).",
                    "default": 10
                }
            }
        })
    }

    async fn call(&self, args: &str) -> Result<String> {
        let limit = if let Ok(parsed) = serde_json::from_str::<LogArgs>(args) {
            parsed.limit
        } else {
            10
        };

        let output = Command::new("git")
            .args(["log", "-n", &limit.to_string()])
            .output()
            .await?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Ok(format!(
                "Error executing git log: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}

/// A tool for viewing git diffs.
pub struct GitDiffTool;

#[derive(Deserialize)]
struct DiffArgs {
    path: Option<String>,
}

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn description(&self) -> &str {
        "Show changes between commits, commit and working tree, etc."
    }

    fn usage(&self) -> &str {
        "Optionally provide a file path to diff. Example: { \"path\": \"src/main.rs\" } or {} for all changes."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to diff."
                }
            }
        })
    }

    async fn call(&self, args: &str) -> Result<String> {
        let path = serde_json::from_str::<DiffArgs>(args).ok().and_then(|a| a.path);

        let mut cmd = Command::new("git");
        cmd.arg("diff");
        if let Some(p) = path {
            cmd.arg(p);
        }

        let output = cmd.output().await?;

        if output.status.success() {
            let diff = String::from_utf8_lossy(&output.stdout);
            if diff.is_empty() {
                Ok("No changes detected.".to_string())
            } else {
                Ok(diff.to_string())
            }
        } else {
            Ok(format!(
                "Error executing git diff: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}
