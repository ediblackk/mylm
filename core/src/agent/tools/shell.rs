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
    memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
    session_id: Option<String>,
}

impl ShellTool {
    /// Create a new ShellTool
    pub fn new(
        executor: Arc<CommandExecutor>,
        context: TerminalContext,
        event_tx: mpsc::UnboundedSender<TuiEvent>,
        memory_store: Option<Arc<crate::memory::store::VectorStore>>,
        categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
        session_id: Option<String>,
    ) -> Self {
        Self { executor, _context: context, event_tx, memory_store, categorizer, session_id }
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

    fn kind(&self) -> crate::agent::tool::ToolKind {
        crate::agent::tool::ToolKind::Terminal
    }

    async fn call(&self, args: &str) -> Result<String> {
        crate::info_log!("ShellTool::call execution started: {}", args);
        // 1. Safety Check (performed by executor)
        if let Err(e) = self.executor.check_safety(args) {
            crate::error_log!("Safety check failed for command: {}. Error: {}", args, e);
            return Err(e);
        }

        // 2. Read Terminal Screen Context
        let (screen_tx, screen_rx) = oneshot::channel::<String>();
        if let Err(e) = self.event_tx.send(TuiEvent::GetTerminalScreen(screen_tx)) {
            crate::error_log!("Failed to send GetTerminalScreen event: {}", e);
            return Err(anyhow::anyhow!("Internal error: Failed to communicate with TUI"));
        }
        
        let mut screen_content = match screen_rx.await {
            Ok(content) => content,
            Err(e) => {
                crate::error_log!("Failed to receive terminal screen: {}", e);
                String::new()
            }
        };
        
        // Hard limit at ~50k tokens (heuristic: 1 token ~= 4 chars) -> 200,000 chars
        let char_limit = 200_000;
        if screen_content.len() > char_limit {
            screen_content = screen_content.chars().rev().take(char_limit).collect::<String>().chars().rev().collect();
        }
        
        // 3. Request execution via TUI Event
        let (tx, rx) = oneshot::channel::<String>();
        if let Err(e) = self.event_tx.send(TuiEvent::ExecuteTerminalCommand(args.to_string(), tx)) {
            crate::error_log!("Failed to send ExecuteTerminalCommand event: {}", e);
            return Err(anyhow::anyhow!("Internal error: Failed to communicate with TUI"));
        }
        
        // 4. Await result from PTY capture
        crate::debug_log!("Awaiting output from TUI for command: {}", args);
        let output = match rx.await {
            Ok(out) => {
                crate::info_log!("Received output for command: {} ({} bytes)", args, out.len());
                out
            }
            Err(e) => {
                let err_msg = format!("Failed to receive command output from TUI: {}", e);
                crate::error_log!("{}", err_msg);
                return Err(anyhow::anyhow!(err_msg));
            }
        };

        // 5. Auto-record to memory if enabled
        if let Some(store) = &self.memory_store {
            if let Ok(memory_id) = store.record_command(args, &output, 0, self.session_id.clone()).await {
                if let Some(categorizer) = &self.categorizer {
                    let content = format!("Command: {}\nOutput: {}", args, output);
                    if let Ok(category_id) = categorizer.categorize_memory(&content).await {
                        let _ = store.update_memory_category(memory_id, category_id.clone()).await;
                        let _ = categorizer.update_category_summary(&category_id).await;
                    }
                }
            }
        }
        
        // Combine screen context with command output
        let combined = format!(
            "--- TERMINAL CONTEXT ---\n{}\nCMD_OUTPUT:\n{}",
            screen_content,
            output
        );
        
        Ok(combined)
    }
}
