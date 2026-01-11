//! Command executor module
//!
//! Safely executes commands with allowlist verification

pub mod allowlist;
pub mod safety;

use crate::context::TerminalContext;
use allowlist::CommandAllowlist;
use anyhow::{Context as _, Result};
use safety::SafetyChecker;
use std::process::Output;
use tokio::process::Command;

/// Command executor with safety checks
#[allow(dead_code)]
pub struct CommandExecutor {
    allowlist: CommandAllowlist,
    safety_checker: SafetyChecker,
}

#[allow(dead_code)]
impl CommandExecutor {
    /// Create a new executor
    pub fn new(allowlist: CommandAllowlist, safety_checker: SafetyChecker) -> Self {
        CommandExecutor {
            allowlist,
            safety_checker,
        }
    }

    /// Execute a command from LLM suggestion
    pub async fn execute_from_llm(
        &self,
        command_str: &str,
        context: &TerminalContext,
        force: bool,
    ) -> Result<ExecutionResult> {
        // Parse the command
        let parsed = self.parse_command(command_str)?;

        // Check if command is in allowlist
        if !self.allowlist.is_allowed(&parsed.command) {
            return Err(anyhow::anyhow!(
                "Command '{}' is not in the allowlist",
                parsed.command
            ));
        }

        // Perform safety checks
        let safety_level = self.safety_checker.assess(command_str, &parsed.command, &parsed.args);

        if safety_level.is_dangerous() && !force {
            return Err(anyhow::anyhow!(
                "Command '{}' is marked as dangerous. Use --force to execute anyway.\n\
                 Reason: {}",
                command_str,
                safety_level.reason()
            ));
        }

        // Build and execute the command
        let output = self.execute(&parsed, context).await?;

        Ok(ExecutionResult {
            command: command_str.to_string(),
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
        })
    }

    /// Execute a command with safety bypass (for internal use)
    pub async fn execute_unsafe(
        &self,
        command_str: &str,
        context: &TerminalContext,
    ) -> Result<ExecutionResult> {
        let parsed = self.parse_command(command_str)?;
        let output = self.execute(&parsed, context).await?;

        Ok(ExecutionResult {
            command: command_str.to_string(),
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
        })
    }

    /// Parse a command string into components
    fn parse_command(&self, command_str: &str) -> Result<ParsedCommand> {
        // Use shell-words for proper parsing of quoted arguments
        let parts = shell_words::split(command_str.trim())
            .map_err(|e| anyhow::anyhow!("Failed to parse command: {}", e))?;

        if parts.is_empty() {
            return Err(anyhow::anyhow!("Empty command"));
        }

        Ok(ParsedCommand {
            command: parts[0].clone(),
            args: parts[1..].to_vec(),
        })
    }

    /// Execute the actual command
    async fn execute(
        &self,
        parsed: &ParsedCommand,
        context: &TerminalContext,
    ) -> Result<Output> {
        let mut cmd = Command::new(&parsed.command);

        // Add arguments
        cmd.args(&parsed.args);

        // Set working directory
        if let Some(cwd) = &context.cwd {
            cmd.current_dir(cwd);
        }

        // Execute
        let output = cmd
            .output()
            .await
            .with_context(|| format!("Failed to execute command: {}", parsed.command))?;

        Ok(output)
    }

    pub fn check_safety(&self, command_str: &str) -> Result<()> {
        let parsed = self.parse_command(command_str)?;

        // Check if command is in allowlist
        if !self.allowlist.is_allowed(&parsed.command) {
            return Err(anyhow::anyhow!(
                "Command '{}' is not in the allowlist",
                parsed.command
            ));
        }

        // Perform safety checks
        let safety_level = self.safety_checker.assess(command_str, &parsed.command, &parsed.args);

        if safety_level.is_dangerous() {
            return Err(anyhow::anyhow!(
                "Command '{}' is marked as dangerous.\nReason: {}",
                command_str,
                safety_level.reason()
            ));
        }

        Ok(())
    }
}

/// Parsed command structure
#[allow(dead_code)]
pub struct ParsedCommand {
    pub command: String,
    pub args: Vec<String>,
}

/// Result of command execution
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExecutionResult {
    /// The command that was executed
    pub command: String,
    /// Whether the command succeeded
    pub success: bool,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Exit code
    pub exit_code: Option<i32>,
}

#[allow(dead_code)]
impl ExecutionResult {
    /// Check if execution was successful
    pub fn is_success(&self) -> bool {
        self.success
    }

    /// Get combined output (stdout + stderr)
    pub fn combined_output(&self) -> String {
        let mut output = self.stdout.clone();
        if !self.stderr.is_empty() {
            output.push_str("\n--- stderr ---\n");
            output.push_str(&self.stderr);
        }
        output
    }
}
