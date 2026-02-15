//! Shell command execution tool
//!
//! Executes shell commands with safety checks and timeout handling.

use crate::agent::runtime::capability::{Capability, ToolCapability};
use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::ToolError;
use crate::agent::runtime::terminal::TerminalExecutor;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;

use tokio::time::{timeout, Duration};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_OUTPUT_SIZE: usize = 100_000; // 100KB max output

/// Shell command execution tool
/// 
/// This tool executes shell commands using a TerminalExecutor.
/// When running in TUI mode, this uses the shared PTY so the agent
/// can see the terminal state and commands run in the same session.
#[derive(Debug, Default)]
pub struct ShellTool;

impl ShellTool {
    /// Create a new shell tool
    pub fn new() -> Self {
        Self
    }

    /// Execute a shell command
    async fn execute_shell(&self, command: &str, _background: bool) -> Result<ToolResult, ToolError> {
        // SECURITY: Basic command validation
        let dangerous_patterns = ["rm -rf /", "> /dev/sda", "dd if=/dev/zero"];
        for pattern in &dangerous_patterns {
            if command.contains(pattern) {
                return Ok(ToolResult::Error {
                    message: format!("Command blocked for safety: contains '{}'", pattern),
                    code: Some("SAFETY_BLOCK".to_string()),
                    retryable: false,
                });
            }
        }

        // Execute with timeout
        let result = timeout(
            Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            self.run_command(command),
        )
        .await;

        match result {
            Ok(Ok(output)) => Ok(ToolResult::Success {
                output,
                structured: None,
            }),
            Ok(Err(e)) => Ok(ToolResult::Error {
                message: format!("Command failed: {}", e),
                code: Some("EXEC_ERROR".to_string()),
                retryable: false,
            }),
            Err(_) => Ok(ToolResult::Error {
                message: format!("Command timed out after {} seconds", DEFAULT_TIMEOUT_SECS),
                code: Some("TIMEOUT".to_string()),
                retryable: true,
            }),
        }
    }

    /// Run the actual command
    async fn run_command(&self, command: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use tokio::process::Command;
        
        let output = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", command])
                .output()
                .await?
        } else {
            Command::new("sh")
                .args(["-c", command])
                .output()
                .await?
        };

        let mut result = String::new();

        // Add stdout
        if !output.stdout.is_empty() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            result.push_str(&stdout);
        }

        // Add stderr if present
        if !output.stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !result.is_empty() {
                result.push_str("\n\n[stderr]:\n");
            } else {
                result.push_str("[stderr]:\n");
            }
            result.push_str(&stderr);
        }

        // Truncate if too large
        if result.len() > MAX_OUTPUT_SIZE {
            result.truncate(MAX_OUTPUT_SIZE);
            result.push_str("\n... [output truncated]");
        }

        if output.status.success() {
            Ok(result)
        } else {
            let exit_code = output.status.code().unwrap_or(-1);
            Err(format!("Exit code {}: {}", exit_code, result).into())
        }
    }

    /// Execute shell command using a terminal executor
    /// 
    /// This runs the command through the shared PTY when running in TUI mode,
    /// allowing the agent to see the terminal state.
    async fn execute_shell_with_terminal(
        &self,
        terminal: &dyn TerminalExecutor,
        command: &str,
        _background: bool,
    ) -> Result<ToolResult, ToolError> {
        // SECURITY: Basic command validation
        let dangerous_patterns = ["rm -rf /", "> /dev/sda", "dd if=/dev/zero"];
        for pattern in &dangerous_patterns {
            if command.contains(pattern) {
                return Ok(ToolResult::Error {
                    message: format!("Command blocked for safety: contains '{}'", pattern),
                    code: Some("SAFETY_BLOCK".to_string()),
                    retryable: false,
                });
            }
        }

        // Get terminal screen before command (for context)
        let screen_before = terminal.get_screen().await.unwrap_or_default();

        // Execute with timeout
        let result = terminal.execute_command(
            command.to_string(),
            Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        ).await;

        match result {
            Ok(output) => {
                // Truncate if too large
                let output = if output.len() > MAX_OUTPUT_SIZE {
                    let mut truncated = output;
                    truncated.truncate(MAX_OUTPUT_SIZE);
                    truncated.push_str("\n... [output truncated]");
                    truncated
                } else {
                    output
                };

                // Combine screen context with output for the agent
                let combined = if screen_before.is_empty() {
                    output
                } else {
                    format!(
                        "--- TERMINAL CONTEXT ---\n{}\n--- COMMAND OUTPUT ---\n{}",
                        screen_before,
                        output
                    )
                };

                Ok(ToolResult::Success {
                    output: combined,
                    structured: None,
                })
            }
            Err(e) => Ok(ToolResult::Error {
                message: format!("Command failed: {}", e),
                code: Some("EXEC_ERROR".to_string()),
                retryable: false,
            }),
        }
    }
}

impl Capability for ShellTool {
    fn name(&self) -> &'static str {
        "shell"
    }
}

#[async_trait::async_trait]
impl ToolCapability for ShellTool {
    async fn execute(
        &self,
        ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse arguments - can be string or JSON object
        let args_str = call.arguments.as_str()
            .map(|s| s.to_string())
            .or_else(|| {
                call.arguments.get("command")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .ok_or_else(|| ToolError::new(
                "Expected command string or {\"command\": \"...\"}"
            ))?;

        let background = call.arguments
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Use terminal executor from context if available
        if let Some(terminal) = ctx.terminal() {
            self.execute_shell_with_terminal(terminal, &args_str, background).await
        } else {
            self.execute_shell(&args_str, background).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_echo() {
        let tool = ShellTool::new();
        let call = ToolCall {
            name: "shell".to_string(),
            arguments: serde_json::json!("echo hello world"),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.execute(&RuntimeContext::new(), call).await;
        assert!(result.is_ok());
        
        match result.unwrap() {
            ToolResult::Success { output, .. } => {
                assert!(output.contains("hello world"));
            }
            _ => panic!("Expected success"),
        }
    }

    #[tokio::test]
    async fn test_shell_json_args() {
        let tool = ShellTool::new();
        let call = ToolCall {
            name: "shell".to_string(),
            arguments: serde_json::json!({
                "command": "echo test"
            }),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.execute(&RuntimeContext::new(), call).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_shell_blocked_command() {
        let tool = ShellTool::new();
        let call = ToolCall {
            name: "shell".to_string(),
            arguments: serde_json::json!("rm -rf /"),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.execute(&RuntimeContext::new(), call).await;
        assert!(result.is_ok());
        
        match result.unwrap() {
            ToolResult::Error { code, .. } => {
                assert_eq!(code, Some("SAFETY_BLOCK".to_string()));
            }
            _ => panic!("Expected error for blocked command"),
        }
    }
}
