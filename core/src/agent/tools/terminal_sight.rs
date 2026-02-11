use crate::agent::tool::{Tool, ToolKind, ToolOutput};
use crate::agent::traits::TerminalExecutor;
use async_trait::async_trait;
use std::error::Error as StdError;
use std::sync::Arc;

/// A tool for peeking at the current terminal screen buffer.
/// Useful for monitoring long-running tasks or checking terminal state without executing commands.
pub struct TerminalSightTool {
    terminal: Arc<dyn TerminalExecutor>,
}

impl TerminalSightTool {
    pub fn new(terminal: Arc<dyn TerminalExecutor>) -> Self {
        Self { terminal }
    }
}

#[async_trait]
impl Tool for TerminalSightTool {
    fn name(&self) -> &str {
        "terminal_sight"
    }

    fn description(&self) -> &str {
        "Get a snapshot of the current terminal screen content. CRITICAL: You MUST use this tool after running execute_command to see the command output. You cannot see terminal output without using this tool."
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

    async fn call(&self, _args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let screen = self.terminal.get_screen().await?;

        crate::info_log!("TerminalSightTool: screen len={}, empty={}", screen.len(), screen.trim().is_empty());
        
        if screen.trim().is_empty() {
            Ok(ToolOutput::Immediate(serde_json::Value::String(
                "Terminal is currently empty or has no visible text.".to_string(),
            )))
        } else {
            crate::info_log!("TerminalSightTool: returning screen content ({} bytes)", screen.len());
            Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "## Terminal Screen Snapshot:\n\n{}",
                screen
            ))))
        }
    }
}
