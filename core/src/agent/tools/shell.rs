use crate::agent::tool::{Tool, ToolOutput};
use crate::agent::v2::jobs::JobRegistry;
use crate::context::TerminalContext;
use crate::executor::CommandExecutor;
use crate::terminal::app::TuiEvent;
use async_trait::async_trait;
use serde::Deserialize;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

#[derive(Deserialize)]
struct ShellArgs {
    command: String,
    #[serde(default)]
    background: bool,
}

/// A tool for executing shell commands safely.
pub struct ShellTool {
    executor: Arc<CommandExecutor>,
    _context: TerminalContext,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
    session_id: Option<String>,
    job_registry: Option<JobRegistry>,
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
        job_registry: Option<JobRegistry>,
    ) -> Self {
        Self {
            executor,
            _context: context,
            event_tx,
            memory_store,
            categorizer,
            session_id,
            job_registry,
        }
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

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        crate::info_log!("ShellTool::call execution started: {}", args);

        // Parse args
        let shell_args = if let Ok(parsed) = serde_json::from_str::<ShellArgs>(args) {
            parsed
        } else {
            ShellArgs {
                command: args.trim().trim_matches('"').to_string(),
                background: false,
            }
        };

        let cmd = &shell_args.command;

        // 1. Safety Check (performed by executor)
        if let Err(e) = self.executor.check_safety(cmd) {
            crate::error_log!("Safety check failed for command: {}. Error: {}", cmd, e);
            return Err(e.into());
        }

        if shell_args.background {
            if let Some(registry) = &self.job_registry {
                let job_id = registry.create_job("shell", &format!("Executing: {}", cmd));
                let registry = registry.clone();
                let event_tx = self.event_tx.clone();
                let cmd_clone = cmd.clone();
                let job_id_for_task = job_id.clone();
                let description = format!("Running command: {}", cmd_clone);

                // Spawn background task
                tokio::spawn(async move {
                    let (tx, rx) = oneshot::channel::<String>();
                    if let Err(e) = event_tx.send(TuiEvent::ExecuteTerminalCommand(cmd_clone.clone(), tx))
                    {
                        registry.fail_job(&job_id_for_task, &format!("Failed to send command to TUI: {}", e));
                        return;
                    }

                    match rx.await {
                        Ok(output) => {
                            registry.update_job_output(&job_id_for_task, &output);
                            registry.complete_job(&job_id_for_task, serde_json::Value::String(output));
                        }
                        Err(e) => {
                            registry.fail_job(&job_id_for_task, &format!("Failed to receive output: {}", e));
                        }
                    }
                });

                return Ok(ToolOutput::Background {
                    job_id,
                    description,
                });
            } else {
                return Ok(ToolOutput::Immediate(serde_json::Value::String(
                    "Error: Background execution requested but no job registry available."
                        .to_string(),
                )));
            }
        }

        // 2. Read Terminal Screen Context
        let (screen_tx, screen_rx) = oneshot::channel::<String>();
        if let Err(e) = self.event_tx.send(TuiEvent::GetTerminalScreen(screen_tx)) {
            crate::error_log!("Failed to send GetTerminalScreen event: {}", e);
            return Err(format!("Internal error: Failed to communicate with TUI: {}", e).into());
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
            return Err(format!("Internal error: Failed to communicate with TUI: {}", e).into());
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
                return Err(err_msg.into());
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

        Ok(ToolOutput::Immediate(serde_json::Value::String(combined)))
    }
}
