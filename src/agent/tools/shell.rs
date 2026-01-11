use crate::agent::tool::Tool;
use crate::executor::CommandExecutor;
use crate::context::TerminalContext;
use crate::terminal::app::TuiEvent;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// A tool for executing shell commands safely.
pub struct ShellTool {
    executor: Arc<CommandExecutor>,
    _context: TerminalContext,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
}

impl ShellTool {
    /// Create a new ShellTool
    pub fn new(
        executor: Arc<CommandExecutor>,
        context: TerminalContext,
        event_tx: mpsc::UnboundedSender<TuiEvent>
    ) -> Self {
        Self { executor, _context: context, event_tx }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "execute_command"
    }

    fn description(&self) -> &str {
        "Execute a shell command safely. Use this to run system commands, list files, or check system status."
    }

    fn usage(&self) -> &str {
        "Pass the command string directly as arguments. Example: 'ls -la' or 'ps aux'."
    }

    async fn call(&self, args: &str) -> Result<String> {
        // 1. Safety Check (performed by executor)
        self.executor.check_safety(args)?;

        // 2. Read Terminal Screen Context
        let (screen_tx, screen_rx) = oneshot::channel();
        let _ = self.event_tx.send(TuiEvent::GetTerminalScreen(screen_tx));
        let mut screen_content = screen_rx.await.unwrap_or_default();
        
        // Hard limit at ~50k tokens (heuristic: 1 token ~= 4 chars) -> 200,000 chars
        let char_limit = 200_000;
        if screen_content.len() > char_limit {
            screen_content = screen_content.chars().rev().take(char_limit).collect::<String>().chars().rev().collect();
        }
        
        // 3. Request execution via TUI Event
        let (tx, rx) = oneshot::channel();
        let _ = self.event_tx.send(TuiEvent::ExecuteTerminalCommand(args.to_string(), tx));
        
        // 4. Await result from PTY capture
        let output = rx.await.map_err(|_| anyhow::anyhow!("Failed to receive command output from TUI"))?;
        
        // Combine screen context with command output
        let combined = format!(
            "--- TERMINAL CONTEXT ---\n{}\n--- COMMAND EXECUTION ---\n{}",
            screen_content,
            output
        );
        
        Ok(combined)
    }
}
