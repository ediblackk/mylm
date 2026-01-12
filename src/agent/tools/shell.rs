use crate::agent::tool::Tool;
use crate::executor::CommandExecutor;
use crate::context::TerminalContext;
use crate::memory::VectorStore;
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
    memory_store: Option<Arc<VectorStore>>,
    categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
    session_id: Option<String>,
}

impl ShellTool {
    /// Create a new ShellTool
    pub fn new(
        executor: Arc<CommandExecutor>,
        context: TerminalContext,
        event_tx: mpsc::UnboundedSender<TuiEvent>,
        memory_store: Option<Arc<VectorStore>>,
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

        // 5. Auto-record to memory if enabled
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
