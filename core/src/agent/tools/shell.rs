use crate::agent::tool::{Tool, ToolOutput};
use crate::agent::traits::TerminalExecutor;
use crate::agent::v2::jobs::JobRegistry;
use crate::context::TerminalContext;
use crate::executor::CommandExecutor;
use async_trait::async_trait;
use serde::Deserialize;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use crate::config::v2::types::AgentPermissions;

/// Configuration for creating a new ShellTool
#[derive(Clone)]
pub struct ShellToolConfig {
    pub executor: Arc<CommandExecutor>,
    pub context: TerminalContext,
    pub terminal: Arc<dyn TerminalExecutor>,
    pub memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    pub categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
    pub session_id: Option<String>,
    pub job_registry: Option<JobRegistry>,
    pub permissions: Option<AgentPermissions>,
}

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
    terminal: Arc<dyn TerminalExecutor>,
    memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
    session_id: Option<String>,
    job_registry: Option<JobRegistry>,
    permissions: Option<AgentPermissions>,
}

impl ShellTool {
    /// Create a new ShellTool with individual parameters (deprecated, use `new_with_config`)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        executor: Arc<CommandExecutor>,
        context: TerminalContext,
        terminal: Arc<dyn TerminalExecutor>,
        memory_store: Option<Arc<crate::memory::store::VectorStore>>,
        categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
        session_id: Option<String>,
        job_registry: Option<JobRegistry>,
        permissions: Option<AgentPermissions>,
    ) -> Self {
        Self::new_with_config(ShellToolConfig {
            executor,
            context,
            terminal,
            memory_store,
            categorizer,
            session_id,
            job_registry,
            permissions,
        })
    }

    /// Create a new ShellTool from a configuration struct
    pub fn new_with_config(config: ShellToolConfig) -> Self {
        Self {
            executor: config.executor,
            _context: config.context,
            terminal: config.terminal,
            memory_store: config.memory_store,
            categorizer: config.categorizer,
            session_id: config.session_id,
            job_registry: config.job_registry,
            permissions: config.permissions,
        }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "execute_command"
    }

    fn description(&self) -> &str {
        "Execute a shell command safely. Use this for system commands, git operations, or complex shell pipelines. For simple file/directory listing, prefer 'list_files' tool which gives structured JSON output."
    }

    fn usage(&self) -> &str {
        "Either pass a raw command string (e.g. 'ls -la') OR a JSON object: { \"command\": \"ls -la\", \"background\": false }."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute (e.g. ls -la)"
                },
                "background": {
                    "type": "boolean",
                    "description": "Run in background as a job",
                    "default": false
                }
            },
            "required": ["command"]
        })
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

        let cmd = shell_args.command.trim();
        crate::debug_log!("ShellTool: parsed command: '{}', background: {}", cmd, shell_args.background);

        // 1. Permission Check (if agent has permissions configured)
        if let Some(agent_permissions) = &self.permissions {
            // Check forbidden commands first (highest priority)
            if let Some(forbidden) = &agent_permissions.forbidden_commands {
                for pattern in forbidden {
                    if crate::agent::permissions::matches_pattern(cmd, pattern) {
                        crate::warn_log!("ShellTool: command '{}' matches forbidden pattern '{}'", cmd, pattern);
                        return Ok(ToolOutput::Immediate(serde_json::Value::String(
                            format!("Error: Command '{}' is forbidden by pattern '{}'", cmd, pattern)
                        )));
                    }
                }
            }

        }

        // 2. Safety Check (performed by executor)
        if let Err(e) = self.executor.check_safety(cmd) {
            crate::error_log!("Safety check failed for command: {}. Error: {}", cmd, e);
            return Err(e.into());
        }
        crate::debug_log!("ShellTool: safety check passed for command: {}", cmd);

        if shell_args.background {
            crate::info_log!("ShellTool: executing in background: {}", cmd);
            if let Some(registry) = &self.job_registry {
                let job_id = registry.create_job("shell", &format!("Executing: {}", cmd));
                crate::info_log!("ShellTool: created background job: {}", &job_id[..8.min(job_id.len())]);
                
                let registry = registry.clone();
                let terminal = self.terminal.clone();
                let cmd_clone = cmd.to_string();
                let job_id_for_task = job_id.clone();
                let description = format!("Running command: {}", cmd_clone);

                // Spawn background task
                tokio::spawn(async move {
                    crate::debug_log!("ShellTool background task: starting job {}", &job_id_for_task[..8.min(job_id_for_task.len())]);
                    match timeout(Duration::from_secs(30), terminal.execute_command(cmd_clone.clone(), Some(Duration::from_secs(30)))).await {
                        Ok(Ok(output)) => {
                            crate::info_log!("ShellTool background task: job {} completed, output length: {}", &job_id_for_task[..8.min(job_id_for_task.len())], output.len());
                            registry.update_job_output(&job_id_for_task, &output);
                            registry.complete_job(&job_id_for_task, serde_json::Value::String(output));
                        }
                        Ok(Err(e)) => {
                            crate::error_log!("ShellTool background task: job {} failed to receive output: {}", &job_id_for_task[..8.min(job_id_for_task.len())], e);
                            registry.fail_job(&job_id_for_task, &format!("Failed to receive output: {}", e));
                        }
                        Err(_) => {
                            // Timeout occurred - enter grace period before final failure
                            // This allows time for file I/O to flush and coordinator to read output
                            // The job will automatically transition to Failed after 15 seconds via heartbeat
                            crate::warn_log!("ShellTool background task: job {} timed out, entering grace period", &job_id_for_task[..8.min(job_id_for_task.len())]);
                            registry.set_timeout_pending(&job_id_for_task, "Command timed out after 30 seconds - waiting for output flush");
                            // Return immediately - no blocking sleep here
                        }
                    }
                });

                return Ok(ToolOutput::Background {
                    job_id,
                    description,
                });
            } else {
                crate::error_log!("ShellTool: background execution requested but no job registry available");
                return Ok(ToolOutput::Immediate(serde_json::Value::String(
                    "Error: Background execution requested but no job registry available."
                        .to_string(),
                )));
            }
        }

        crate::debug_log!("ShellTool: executing in foreground: {}", cmd);
        // 2. Read Terminal Screen Context
        let screen_content = match self.terminal.get_screen().await {
            Ok(content) => {
                crate::debug_log!("ShellTool: got terminal screen, length: {}", content.len());
                content
            }
            Err(e) => {
                crate::error_log!("Failed to receive terminal screen: {}", e);
                String::new()
            }
        };

        // Hard limit at ~50k tokens (heuristic: 1 token ~= 4 chars) -> 200,000 chars
        let char_limit = 200_000;
        let screen_content = if screen_content.len() > char_limit {
            let truncated: String = screen_content.chars().rev().take(char_limit).collect::<String>().chars().rev().collect();
            crate::debug_log!("ShellTool: screen content truncated to {} chars", char_limit);
            truncated
        } else {
            screen_content
        };

        // 3. Execute command via TerminalExecutor
        // PTY Locking: Sequential execution enforcement for Shell tools
        // This prevents parallel shell commands from colliding on the single session PTY.
        lazy_static::lazy_static! {
            static ref PTY_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        }
        let _guard = PTY_MUTEX.lock().await;
        crate::trace_log!("ShellTool: PTY lock acquired for command: {}", cmd);

        let output = match self.terminal.execute_command(cmd.to_string(), None).await {
            Ok(out) => {
                crate::info_log!("ShellTool: received output for command '{}' ({} bytes)", cmd, out.len());
                out
            }
            Err(e) => {
                let err_msg = format!("Failed to receive command output from terminal: {}", e);
                crate::error_log!("{}", err_msg);
                return Err(err_msg.into());
            }
        };

        // 4. Auto-record to memory if enabled
        if let Some(store) = &self.memory_store {
            if let Ok(memory_id) = store.record_command(cmd, &output, 0, self.session_id.clone()).await {
                crate::debug_log!("ShellTool: command recorded to memory with id: {}", memory_id);
                if let Some(categorizer) = &self.categorizer {
                    let content = format!("Command: {}\nOutput: {}", cmd, output);
                    if let Ok(category_id) = categorizer.categorize_memory(&content).await {
                        let _ = store.update_memory_category(memory_id, category_id.clone()).await;
                        let _ = categorizer.update_category_summary(&category_id).await;
                        crate::debug_log!("ShellTool: memory categorized as: {}", category_id);
                    }
                }
            } else {
                crate::warn_log!("ShellTool: failed to record command to memory");
            }
        }

        // Combine screen context with command output
        let combined = format!(
            "--- TERMINAL CONTEXT ---\n{}\nCMD_OUTPUT:\n{}",
            screen_content,
            output
        );

        crate::debug_log!("ShellTool: command '{}' completed successfully, total output length: {}", cmd, combined.len());
        Ok(ToolOutput::Immediate(serde_json::Value::String(combined)))
    }
}
