use crate::agent::tool::Tool;
use crate::executor::CommandExecutor;
use crate::context::TerminalContext;
use crate::terminal::app::TuiEvent;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

/// A tool for executing shell commands safely.
pub struct ShellTool {
    executor: Arc<CommandExecutor>,
    context: TerminalContext,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
}

impl ShellTool {
    /// Create a new ShellTool
    pub fn new(
        executor: Arc<CommandExecutor>,
        context: TerminalContext,
        event_tx: mpsc::UnboundedSender<TuiEvent>
    ) -> Self {
        Self { executor, context, event_tx }
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
        // Mirror the command to PTY for visibility before execution
        let mirror = format!("\x1b[33m$ {}\x1b[0m\r\n", args.trim());
        let _ = self.event_tx.send(TuiEvent::PtyWrite(mirror.into_bytes()));
        
        let result = self.executor.execute_from_llm(args, &self.context, false).await?;
        
        // Mirror the output to PTY for visibility
        let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", result.combined_output().trim());
        let _ = self.event_tx.send(TuiEvent::PtyWrite(obs_log.into_bytes()));
        
        if result.success {
            Ok(result.combined_output())
        } else {
            let error_msg = format!(
                "Command failed with exit code {:?}\nOutput:\n{}",
                result.exit_code,
                result.combined_output()
            );
            Err(anyhow::anyhow!(error_msg))
        }
    }
}
