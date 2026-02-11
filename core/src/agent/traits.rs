//! Agent traits - abstractions for core agent components
//!
//! This module defines traits that allow core agents to interact with external systems
//! without direct dependencies on terminal or UI components.

use std::time::Duration;
use async_trait::async_trait;

/// Trait for executing terminal commands and retrieving screen content.
///
/// This abstraction decouples core tools (ShellTool, TerminalSightTool) from the
/// terminal UI implementation, allowing the core to be independent and testable.
#[async_trait]
pub trait TerminalExecutor: Send + Sync {
    /// Execute a command in the terminal and return the output.
    ///
    /// # Arguments
    /// * `cmd` - The command to execute
    /// * `timeout` - Optional timeout after which the command should be cancelled.
    ///               If `None`, the command may wait indefinitely.
    ///
    /// # Returns
    /// * `Ok(String)` - The command output (stdout + stderr)
    /// * `Err(String)` - Error message if execution failed
    async fn execute_command(&self, cmd: String, timeout: Option<Duration>) -> Result<String, String>;

    /// Get the current terminal screen content.
    ///
    /// # Returns
    /// * `Ok(String)` - The visible terminal screen content as text
    /// * `Err(String)` - Error message if retrieval failed
    async fn get_screen(&self) -> Result<String, String>;
}
