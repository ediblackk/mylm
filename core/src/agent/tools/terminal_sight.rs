use crate::agent::tool::{Tool, ToolKind};
use crate::terminal::app::TuiEvent;
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

/// A tool for peeking at the current terminal screen buffer.
/// Useful for monitoring long-running tasks or checking terminal state without executing commands.
pub struct TerminalSightTool {
    event_tx: mpsc::UnboundedSender<TuiEvent>,
}

impl TerminalSightTool {
    pub fn new(event_tx: mpsc::UnboundedSender<TuiEvent>) -> Self {
        Self { event_tx }
    }
}

#[async_trait]
impl Tool for TerminalSightTool {
    fn name(&self) -> &str {
        "terminal_sight"
    }

    fn description(&self) -> &str {
        "Get a snapshot of the current terminal screen content. Use this to see what is currently displayed, including TUI apps, progress bars, or the result of previously executed commands."
    }

    fn usage(&self) -> &str {
        "terminal_sight"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }

    async fn call(&self, _args: &str) -> Result<String> {
        let (tx, rx) = oneshot::channel::<String>();
        self.event_tx.send(TuiEvent::GetTerminalScreen(tx))
            .map_err(|_| anyhow::anyhow!("Failed to contact UI for terminal sight"))?;

        let screen = rx.await.map_err(|_| anyhow::anyhow!("UI failed to provide terminal sight"))?;
        
        if screen.trim().is_empty() {
            Ok("Terminal is currently empty or has no visible text.".to_string())
        } else {
            Ok(format!("## Terminal Screen Snapshot:\n\n{}", screen))
        }
    }
}
