//! TerminalDelegate - Implementation of TerminalExecutor trait for the TUI
//!
//! This delegate bridges the core tools (ShellTool, TerminalSightTool) to the
//! terminal UI by translating TerminalExecutor calls into TuiEvent messages.

use crate::terminal::pty::PtyManager;
use mylm_core::agent::traits::TerminalExecutor;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
// async_trait imported but not used in this file

/// Terminal delegate that implements TerminalExecutor by routing commands through
/// the TUI event system. This maintains the existing PTY locking and event handling
/// while providing a clean trait-based interface for core tools.
pub struct TerminalDelegate {
    event_tx: mpsc::UnboundedSender<crate::terminal::app::TuiEvent>,
    // Mutex to ensure serialized access to PTY across all tools using this delegate
    _pty_lock: Arc<Mutex<()>>,
}

impl TerminalDelegate {
    /// Create a new TerminalDelegate
    pub fn new(
        _pty_manager: Arc<PtyManager>,
        event_tx: mpsc::UnboundedSender<crate::terminal::app::TuiEvent>,
    ) -> Self {
        Self {
            event_tx,
            _pty_lock: Arc::new(Mutex::new(())),
        }
    }
}

#[async_trait::async_trait]
impl TerminalExecutor for TerminalDelegate {
    async fn execute_command(&self, cmd: String, timeout: Option<std::time::Duration>) -> Result<String, String> {
        // Acquire lock to ensure serialized PTY access
        let _guard = self._pty_lock.lock().await;

        // Send ExecuteTerminalCommand event and wait for result
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        
        if let Err(e) = self.event_tx.send(crate::terminal::app::TuiEvent::ExecuteTerminalCommand(cmd.clone(), tx)) {
            return Err(format!("Failed to send ExecuteTerminalCommand event: {}", e));
        }

        // Wait for result with optional timeout
        match timeout {
            Some(duration) => {
                match tokio::time::timeout(duration, rx).await {
                    Ok(result) => match result {
                        Ok(output) => Ok(output),
                        Err(e) => Err(format!("Command execution failed: {}", e)),
                    },
                    Err(_) => Err(format!("Command timed out after {} seconds", duration.as_secs())),
                }
            }
            None => {
                match rx.await {
                    Ok(output) => Ok(output),
                    Err(e) => Err(format!("Command execution failed: {}", e)),
                }
            }
        }
    }

    async fn get_screen(&self) -> Result<String, String> {
        // Send GetTerminalScreen event and wait for result
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        
        if let Err(e) = self.event_tx.send(crate::terminal::app::TuiEvent::GetTerminalScreen(tx)) {
            return Err(format!("Failed to send GetTerminalScreen event: {}", e));
        }

        match rx.await {
            Ok(screen) => Ok(screen),
            Err(e) => Err(format!("Failed to get terminal screen: {}", e)),
        }
    }
}
