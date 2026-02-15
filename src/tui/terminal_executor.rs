//! TUI Terminal Executor
//!
//! A TerminalExecutor implementation that uses the TUI's shared PTY.
//! 
//! Note: This is a simplified implementation. The full implementation would
//! integrate with the event loop to capture command output from the PTY.
//! For now, commands are executed via std::process::Command but the screen
//! content is retrieved from the App's vt100 parser.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use async_trait::async_trait;
use mylm_core::agent::runtime::terminal::TerminalExecutor;

use crate::tui::app::state::AppStateContainer;

/// A terminal executor for the TUI
/// 
/// This executor gets screen content from the App's vt100 parser.
/// Command execution falls back to std::process::Command for reliability.
pub struct TuiTerminalExecutor {
    /// Function to get current screen content from the App
    get_screen_fn: Arc<dyn Fn() -> Result<String, String> + Send + Sync>,
}

impl TuiTerminalExecutor {
    /// Create a new TUI terminal executor with a screen getter function
    pub fn new<F>(get_screen_fn: F) -> Self
    where
        F: Fn() -> Result<String, String> + Send + Sync + 'static,
    {
        Self {
            get_screen_fn: Arc::new(get_screen_fn),
        }
    }
    
    /// Create from a shared App reference
    /// 
    /// The App must remain alive for the lifetime of the executor.
    /// This uses a weak reference pattern to avoid circular dependencies.
    pub fn from_app(app: Arc<Mutex<AppStateContainer>>) -> Self {
        let screen_app = Arc::clone(&app);
        
        Self::new(move || {
            let app = screen_app.lock()
                .map_err(|_| "Failed to lock app")?;
            let screen = app.terminal_parser.screen();
            Ok(screen.contents())
        })
    }
}

#[async_trait]
impl TerminalExecutor for TuiTerminalExecutor {
    async fn execute_command(&self, command: String, timeout: Option<Duration>) -> Result<String, String> {
        // Get screen before command
        let screen_before = self.get_screen().await.unwrap_or_default();
        
        // Execute using std::process::Command for reliability
        // In the future, this could use the PTY for true shared session
        let output = tokio::process::Command::new("sh")
            .args(["-c", &command])
            .output()
            .await
            .map_err(|e| format!("Failed to execute command: {}", e))?;
        
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
        
        // Combine with screen context
        let combined = if screen_before.is_empty() {
            result
        } else {
            format!(
                "--- TERMINAL CONTEXT ---\n{}\n--- COMMAND OUTPUT ---\n{}",
                screen_before,
                result
            )
        };
        
        if output.status.success() {
            Ok(combined)
        } else {
            let exit_code = output.status.code().unwrap_or(-1);
            Err(format!("Exit code {}: {}", exit_code, combined))
        }
    }
    
    async fn get_screen(&self) -> Result<String, String> {
        (self.get_screen_fn)()
    }
}

impl Clone for TuiTerminalExecutor {
    fn clone(&self) -> Self {
        Self {
            get_screen_fn: Arc::clone(&self.get_screen_fn),
        }
    }
}

unsafe impl Send for TuiTerminalExecutor {}
unsafe impl Sync for TuiTerminalExecutor {}
