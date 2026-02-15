//! Terminal Executor Trait
//!
//! Abstraction for executing commands in a terminal and retrieving screen content.
//! This allows the agent to interact with a shared PTY session when running in TUI mode.

use std::time::Duration;
use async_trait::async_trait;
use std::sync::Arc;

/// Trait for executing terminal commands and retrieving screen content.
///
/// This abstraction allows core tools (like ShellTool) to interact with a terminal
/// without direct dependencies on the TUI or PTY implementation.
///
/// When running in TUI mode, the TUI provides a TerminalExecutor implementation
/// that uses the shared PTY. When running in headless mode, a default implementation
/// using std::process::Command can be used.
#[async_trait]
pub trait TerminalExecutor: Send + Sync {
    /// Execute a command in the terminal and return the output.
    ///
    /// # Arguments
    /// * `command` - The command to execute
    /// * `timeout` - Optional timeout after which the command should be cancelled
    ///
    /// # Returns
    /// * `Ok(String)` - The command output (stdout + stderr)
    /// * `Err(String)` - Error message if execution failed
    async fn execute_command(&self, command: String, timeout: Option<Duration>) -> Result<String, String>;

    /// Get the current terminal screen content.
    ///
    /// This is used by the agent to see the current terminal state,
    /// including any output from previous commands.
    ///
    /// # Returns
    /// * `Ok(String)` - The visible terminal screen content as text
    /// * `Err(String)` - Error message if retrieval failed
    async fn get_screen(&self) -> Result<String, String>;
}

/// A default terminal executor that uses std::process::Command.
///
/// This is used when running in headless mode (no TUI/PTY available).
/// Each command runs in isolation without a shared shell session.
pub struct DefaultTerminalExecutor;

impl DefaultTerminalExecutor {
    /// Create a new default terminal executor
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultTerminalExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TerminalExecutor for DefaultTerminalExecutor {
    async fn execute_command(&self, command: String, timeout: Option<Duration>) -> Result<String, String> {
        use tokio::process::Command;
        use tokio::time::{timeout as tokio_timeout, Duration as TokioDuration};

        let output_result = if let Some(timeout_duration) = timeout {
            tokio_timeout(
                timeout_duration,
                Command::new("sh")
                    .args(["-c", &command])
                    .output()
            ).await
        } else {
            match Command::new("sh")
                .args(["-c", &command])
                .output()
                .await {
                Ok(output) => Ok(Ok(output)),
                Err(e) => Ok(Err(e)),
            }
        };

        match output_result {
            Ok(Ok(output)) => {
                let mut result = String::new();
                
                // Add stdout
                if !output.stdout.is_empty() {
                    result.push_str(&String::from_utf8_lossy(&output.stdout));
                }
                
                // Add stderr
                if !output.stderr.is_empty() {
                    if !result.is_empty() {
                        result.push_str("\n\n[stderr]:\n");
                    } else {
                        result.push_str("[stderr]:\n");
                    }
                    result.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                
                if output.status.success() {
                    Ok(result)
                } else {
                    let exit_code = output.status.code().unwrap_or(-1);
                    Err(format!("Exit code {}: {}", exit_code, result))
                }
            }
            Ok(Err(e)) => Err(format!("Command failed: {}", e)),
            Err(_) => Err("Command timed out".to_string()),
        }
    }

    async fn get_screen(&self) -> Result<String, String> {
        // Default implementation returns empty string
        // since there's no persistent terminal session
        Ok(String::new())
    }
}

/// A terminal executor that wraps an Arc<dyn TerminalExecutor>.
///
/// This allows sharing a single terminal executor instance across multiple tools.
pub struct SharedTerminalExecutor {
    inner: Arc<dyn TerminalExecutor>,
}

impl SharedTerminalExecutor {
    /// Create a new shared terminal executor
    pub fn new(executor: Arc<dyn TerminalExecutor>) -> Self {
        Self { inner: executor }
    }

    /// Get a reference to the inner executor
    pub fn inner(&self) -> &dyn TerminalExecutor {
        &*self.inner
    }
}

impl Clone for SharedTerminalExecutor {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[async_trait]
impl TerminalExecutor for SharedTerminalExecutor {
    async fn execute_command(&self, command: String, timeout: Option<Duration>) -> Result<String, String> {
        self.inner.execute_command(command, timeout).await
    }

    async fn get_screen(&self) -> Result<String, String> {
        self.inner.get_screen().await
    }
}

/// Type alias for a shared terminal executor reference
pub type TerminalExecutorRef = Arc<dyn TerminalExecutor>;
