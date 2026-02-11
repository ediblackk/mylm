//! Slash command handling for the terminal UI
use crate::terminal::app::state::AppStateContainer;
use mylm_core::config::ConfigUiExt;
use mylm_core::llm::chat::ChatMessage;
use std::sync::atomic::Ordering;

use tokio::sync::mpsc::UnboundedSender;

pub use mylm_core::terminal::app::TuiEvent;

impl AppStateContainer {
    pub fn handle_slash_command(&mut self, input: &str, event_tx: UnboundedSender<TuiEvent>) {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts[0];

        match cmd {
            "/profile" => self.handle_profile_command(&parts, event_tx),
            "/config" => self.handle_config_command(&parts, event_tx),
            "/exec" => self.handle_exec_command(&parts, event_tx),
            "/help" => self.handle_help_command(),
            "/model" => self.handle_model_command(&parts, event_tx),
            "/verbose" => self.handle_verbose_command(),
            "/logs" => self.handle_logs_command(&parts),
            "/pacore" => self.handle_pacore_command(&parts),
            "/jobs" => self.handle_jobs_command(&parts),
            "/prompt" => self.handle_prompt_command(event_tx),
            "/context" => self.handle_context_command(event_tx),
            _ => {
                self.chat_history.push(ChatMessage::assistant(format!(
                    "Unknown command: {}",
                    cmd
                )));
            }
        }
    }

    fn handle_prompt_command(&mut self, event_tx: UnboundedSender<TuiEvent>) {
        let agent = self.agent.clone();
        let event_tx_clone = event_tx.clone();
        
        tokio::spawn(async move {
            let tools_desc = agent.get_tools_description().await;
            let full_prompt = agent.get_system_prompt().await;
            
            // Save to logs directory
            let logs_dir = std::path::PathBuf::from("mylm/logs");
            let _ = std::fs::create_dir_all(&logs_dir);
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let log_path = logs_dir.join(format!("system_prompt_{}.txt", timestamp));
            
            let message = match std::fs::write(&log_path, &full_prompt) {
                Ok(_) => format!(
                    "## System Prompt Debug\n\n### Available Tools ({})\n\n{}\n\n---\n\nFull system prompt saved to: `{}` ({} chars)",
                    tools_desc.lines().filter(|l| l.starts_with("- ")).count(),
                    tools_desc,
                    log_path.display(),
                    full_prompt.len()
                ),
                Err(e) => format!(
                    "## System Prompt Debug (Error saving: {})\n\n### Available Tools ({})\n\n{}",
                    e,
                    tools_desc.lines().filter(|l| l.starts_with("- ")).count(),
                    tools_desc
                ),
            };
            
            let _ = event_tx_clone.send(TuiEvent::AgentResponse(message, mylm_core::llm::TokenUsage::default()));
        });
    }

    fn handle_context_command(&mut self, event_tx: UnboundedSender<TuiEvent>) {
        let agent = self.agent.clone();
        let event_tx_clone = event_tx.clone();
        
        tokio::spawn(async move {
            // Clone data from agent wrapper
            let history_clone = agent.history().await;
            let system_prompt = match &agent {
                mylm_core::agent::AgentWrapper::V2(a) => {
                    let guard = a.lock().await;
                    guard.get_system_prompt().await
                }
                mylm_core::agent::AgentWrapper::V1(_) => {
                    "V1 Agent system prompt not available".to_string()
                }
            };
            
            // Calculate token estimates for each message
            let mut context_dump = String::new();
            context_dump.push_str("# LLM Context Dump\n\n");
            context_dump.push_str(&format!("Generated: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
            context_dump.push_str(&format!("Total messages: {}\n\n", history_clone.len()));
            
            // System prompt section
            context_dump.push_str("## System Prompt\n\n");
            context_dump.push_str(&format!("```\n{}\n```\n\n", system_prompt));
            
            // Message history section
            context_dump.push_str("## Conversation History\n\n");
            let mut total_tokens = 0;
            
            for (idx, msg) in history_clone.iter().enumerate() {
                let tokens = mylm_core::context::TokenCounter::estimate(&msg.content);
                total_tokens += tokens;
                
                context_dump.push_str(&format!(
                    "### Message {} | Role: {:?} | Tokens: {}\n\n```\n{}\n```\n\n",
                    idx,
                    msg.role,
                    tokens,
                    msg.content
                ));
            }
            
            context_dump.push_str(&format!("\n## Summary\n\n"));
            context_dump.push_str(&format!("- Total messages: {}\n", history_clone.len()));
            context_dump.push_str(&format!("- Estimated tokens: {}\n", total_tokens));
            
            // Save to logs directory
            let logs_dir = std::path::PathBuf::from("mylm/logs");
            let _ = std::fs::create_dir_all(&logs_dir);
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let log_path = logs_dir.join(format!("context_dump_{}.md", timestamp));
            
            let message = match std::fs::write(&log_path, &context_dump) {
                Ok(_) => format!(
                    "## Context Dump\n\n- Total messages: {}\n- Estimated tokens: {}\n- Full context saved to: `{}`",
                    history_clone.len(),
                    total_tokens,
                    log_path.display()
                ),
                Err(e) => format!(
                    "## Context Dump (Error saving file: {})\n\n- Total messages: {}\n- Estimated tokens: {}\n\nContext preview (first 2000 chars):\n\n```\n{}...\n```",
                    e,
                    history_clone.len(),
                    total_tokens,
                    &context_dump[..2000.min(context_dump.len())]
                ),
            };
            
            let _ = event_tx_clone.send(TuiEvent::AgentResponse(message, mylm_core::llm::TokenUsage::default()));
        });
    }

    fn handle_profile_command(&mut self, parts: &[&str], event_tx: UnboundedSender<TuiEvent>) {
        if parts.len() < 2 {
            let profiles: Vec<String> = self.config.profile_names();
            self.chat_history.push(ChatMessage::assistant(
                format!(
                    "Usage: /profile <name>\nAvailable profiles: {}",
                    profiles.join(", ")
                ),
            ));
            return;
        }
        let name = parts[1];
        if self.config.profiles.contains_key(name) {
            self.config.profile = name.to_string();
            let _ = event_tx.send(TuiEvent::ConfigUpdate(self.config.clone()));
            self.chat_history.push(ChatMessage::assistant(format!(
                "Switched to profile: {}",
                name
            )));
        } else {
            self.chat_history.push(ChatMessage::assistant(format!(
                "Profile '{}' not found",
                name
            )));
        }
    }

    fn handle_config_command(&mut self, parts: &[&str], event_tx: UnboundedSender<TuiEvent>) {
        if parts.len() < 3 {
            self.chat_history.push(ChatMessage::assistant(
                "Usage: /config <key> <value>\nKeys: model, max_iterations".to_string(),
            ));
            return;
        }
        let key = parts[1];
        let value = parts[2];
        let mut updated = false;
        let active_profile_name = self.config.profile.clone();

        match key {
            "model" => {
                if let Err(e) = self
                    .config
                    .set_profile_model_override(&active_profile_name, Some(value.to_string()))
                {
                    self.chat_history.push(ChatMessage::assistant(format!("Error: {}", e)));
                } else {
                    updated = true;
                }
            }
            "max_iterations" => {
                if let Ok(iters) = value.parse::<usize>() {
                    if let Err(e) = self
                        .config
                        .set_profile_max_iterations(&active_profile_name, Some(iters))
                    {
                        self.chat_history.push(ChatMessage::assistant(format!("Error: {}", e)));
                    } else {
                        updated = true;
                    }
                } else {
                    self.chat_history
                        .push(ChatMessage::assistant("max_iterations must be a number".to_string()));
                }
            }
            _ => {
                self.chat_history
                    .push(ChatMessage::assistant(format!("Unknown config key: {}", key)));
            }
        }

        if updated {
            let _ = event_tx.send(TuiEvent::ConfigUpdate(self.config.clone()));
            self.chat_history
                .push(ChatMessage::assistant(format!("Updated {} to {}", key, value)));
        }
    }

    fn handle_exec_command(&mut self, parts: &[&str], _event_tx: UnboundedSender<TuiEvent>) {
        if parts.len() < 2 {
            self.chat_history
                .push(ChatMessage::assistant("Usage: /exec <command>".to_string()));
            return;
        }
        let command = parts[1..].join(" ");

        self.state = mylm_core::terminal::app::AppState::ExecutingTool(command.clone());
        
        // Use orchestrator for /exec command
        if let Some(orchestrator) = &self.orchestrator {
            let history = self.chat_history.clone();
            let auto_approve = self.auto_approve.load(Ordering::SeqCst);
            
            let mut orchestrator = orchestrator.clone();
            orchestrator.set_auto_approve(auto_approve);
            
            // Set terminal delegate for tool execution
            if let Some(ref delegate) = self.terminal_delegate {
                orchestrator.set_terminal_delegate(delegate.clone());
            }
            
            let task = tokio::spawn(async move {
                let _ = orchestrator.start_task(command, history).await;
            });
            self.active_task = Some(task);
        } else {
            self.chat_history.push(ChatMessage::assistant(
                "Error: Orchestrator not initialized".to_string()
            ));
            self.state = mylm_core::terminal::app::AppState::Idle;
        }
    }

    fn handle_help_command(&mut self) {
        self.chat_history.push(ChatMessage::assistant(
            "Available commands:\n\
            /profile <name> - Switch profile\n\
            /model <name> - Set model for active profile\n\
            /config <key> <value> - Update active profile\n\
            /exec <command> - Execute shell command\n\
            /jobs - List active jobs with metrics\n\
            /jobs cancel <id> - Cancel a specific job\n\
            /jobs cancel-all - Cancel all jobs\n\
            /jobs list - List all jobs\n\
            /prompt - Dump system prompt to file\n\
            /context - Dump LLM context to mylm/logs/\n\
            /verbose - Toggle verbose mode\n\
            /help - Show this help\n\n\
            Input Shortcuts:\n\
            Ctrl+a / Home - Start of line\n\
            Ctrl+e / End - End of line\n\
            Ctrl+k - Kill to end\n\
            Ctrl+u - Kill to start\n\
            Arrows - Navigate lines/history"
                .to_string(),
        ));
    }

    fn handle_model_command(&mut self, parts: &[&str], event_tx: UnboundedSender<TuiEvent>) {
        if parts.len() < 2 {
            let active_profile = self.config.profile.clone();
            let effective = self.config.get_effective_endpoint_info();
            let base = self.config.get_endpoint_info();
            let profile_info = self.config.get_profile_info(&active_profile);

            let model_source = if profile_info
                .as_ref()
                .and_then(|p| p.model_override.as_ref())
                .is_some()
            {
                "profile override"
            } else {
                "base endpoint"
            };

            self.chat_history.push(ChatMessage::assistant(format!(
                "Current model: {} ({} via {})\nBase endpoint model: {}\n\nUsage: /model <model-name> to set profile model override, or /model clear to use base endpoint model.",
                effective.model, model_source, active_profile, base.model
            )));
            return;
        }

        let value = parts[1];
        let active_profile_name = self.config.profile.clone();

        if value == "clear" {
            if let Err(e) = self
                .config
                .set_profile_model_override(&active_profile_name, None)
            {
                self.chat_history.push(ChatMessage::assistant(format!("Error: {}", e)));
            } else {
                let _ = event_tx.send(TuiEvent::ConfigUpdate(self.config.clone()));
                self.chat_history.push(ChatMessage::assistant(format!(
                    "Model override cleared for profile '{}'. Using base endpoint model.",
                    active_profile_name
                )));
            }
        } else {
            if let Err(e) = self
                .config
                .set_profile_model_override(&active_profile_name, Some(value.to_string()))
            {
                self.chat_history.push(ChatMessage::assistant(format!("Error: {}", e)));
            } else {
                let _ = event_tx.send(TuiEvent::ConfigUpdate(self.config.clone()));
                self.chat_history.push(ChatMessage::assistant(format!(
                    "Model set to '{}' for profile '{}' (profile override)",
                    value, active_profile_name
                )));
            }
        }
    }

    fn handle_verbose_command(&mut self) {
        self.verbose_mode = !self.verbose_mode;
        let status = if self.verbose_mode { "ON" } else { "OFF" };
        self.chat_history
            .push(ChatMessage::assistant(format!("Verbose mode: {}", status)));
    }

    fn handle_logs_command(&mut self, parts: &[&str]) {
        let n = parts
            .get(1)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(20);
        let logs = mylm_core::agent::logger::get_recent_logs(n);
        let log_text = if logs.is_empty() {
            "No logs found.".to_string()
        } else {
            logs.join("\n")
        };
        self.chat_history.push(ChatMessage::assistant(format!(
            "Recent Logs (last {}):\n{}",
            n, log_text
        )));
    }

    fn handle_pacore_command(&mut self, parts: &[&str]) {
        if parts.len() < 2 {
            let status = if self.pacore_enabled { "ON" } else { "OFF" };
            self.chat_history.push(ChatMessage::assistant(format!(
                "PaCoRe Status:\n  Enabled: {}\n  Rounds: {}\n\nCommands:\n  /pacore on - Enable PaCoRe\n  /pacore off - Disable PaCoRe\n  /pacore rounds <n,n> - Set rounds (e.g., '4,1')\n  /pacore status - Show this status\n  /pacore save - Save config to disk",
                status, self.pacore_rounds
            )));
            return;
        }
        let subcmd = parts[1];
        match subcmd {
            "on" => {
                self.pacore_enabled = true;
                self.chat_history.push(ChatMessage::assistant(
                    "PaCoRe enabled. New messages will use parallel reasoning.".to_string(),
                ));
            }
            "off" => {
                self.pacore_enabled = false;
                self.chat_history.push(ChatMessage::assistant(
                    "PaCoRe disabled. Using standard agent loop.".to_string(),
                ));
            }
            "rounds" => {
                if parts.len() < 3 {
                    self.chat_history.push(ChatMessage::assistant(
                        "Usage: /pacore rounds <comma-separated numbers> (e.g., 4,1)".to_string(),
                    ));
                } else {
                    let new_rounds = parts[2..].join("");
                    if new_rounds.split(',').all(|s| s.trim().parse::<usize>().is_ok()) {
                        let rounds_clone = new_rounds.clone();
                        self.pacore_rounds = new_rounds;
                        self.config.features.pacore.rounds = rounds_clone.clone();
                        let _ = self.config.save(None);
                        self.chat_history.push(ChatMessage::assistant(format!(
                            "PaCoRe rounds set to: {}",
                            rounds_clone
                        )));
                    } else {
                        self.chat_history.push(ChatMessage::assistant(
                            "Invalid rounds format. Use comma-separated numbers (e.g., 4,1)"
                                .to_string(),
                        ));
                    }
                }
            }
            "status" => {
                let status = if self.pacore_enabled { "ON" } else { "OFF" };
                self.chat_history.push(ChatMessage::assistant(format!(
                    "PaCoRe Status:\n  Enabled: {}\n  Rounds: {}",
                    status, self.pacore_rounds
                )));
            }
            "save" => {
                self.config.features.pacore.rounds = self.pacore_rounds.clone();
                match self.config.save(None) {
                    Ok(_) => {
                        self.chat_history
                            .push(ChatMessage::assistant("PaCoRe configuration saved.".to_string()));
                    }
                    Err(e) => {
                        self.chat_history.push(ChatMessage::assistant(format!(
                            "Error saving config: {}",
                            e
                        )));
                    }
                }
            }
            _ => {
                self.chat_history.push(ChatMessage::assistant(format!(
                    "Unknown pacore command: {}. Use 'on', 'off', 'rounds', 'status', or 'save'",
                    subcmd
                )));
            }
        }
    }

    fn handle_jobs_command(&mut self, parts: &[&str]) {
        if parts.len() < 2 {
            // List active jobs
            let jobs = self.job_registry.list_active_jobs();
            if jobs.is_empty() {
                self.chat_history.push(ChatMessage::assistant(
                    "No active jobs.\n\nUsage:\n  /jobs - List active jobs\n  /jobs cancel <id> - Cancel a job\n  /jobs cancel-all - Cancel all jobs\n  /jobs list - List all jobs".to_string()
                ));
            } else {
                let mut msg = format!("Active Jobs ({}):\n\n", jobs.len());
                for job in &jobs {
                    let duration = chrono::Utc::now().signed_duration_since(job.started_at);
                    let metrics = &job.metrics;
                    msg.push_str(&format!(
                        "🆔 {} | {}\n   Status: {:?} | Duration: {}s\n   Tokens: {} prompt / {} completion / {} total | Requests: {}\n   Errors: {} | Rate Limits: {}\n\n",
                        &job.id[..8.min(job.id.len())],
                        job.description,
                        job.status,
                        duration.num_seconds(),
                        metrics.prompt_tokens,
                        metrics.completion_tokens,
                        metrics.total_tokens,
                        metrics.request_count,
                        metrics.error_count,
                        metrics.rate_limit_hits
                    ));
                }
                msg.push_str("Use '/jobs cancel <id>' to cancel a specific job or '/jobs cancel-all' to cancel all.");
                self.chat_history.push(ChatMessage::assistant(msg));
            }
            return;
        }

        let subcmd = parts[1];
        match subcmd {
            "list" => {
                let jobs = self.job_registry.list_all_jobs();
                let mut msg = format!("All Jobs ({}):\n\n", jobs.len());
                for job in &jobs[..jobs.len().min(20)] { // Show last 20
                    let status_icon = match job.status {
                        mylm_core::agent::v2::jobs::JobStatus::Running => "⏳",
                        mylm_core::agent::v2::jobs::JobStatus::Completed => "✅",
                        mylm_core::agent::v2::jobs::JobStatus::Failed => "❌",
                        mylm_core::agent::v2::jobs::JobStatus::Cancelled => "🛑",
                        mylm_core::agent::v2::jobs::JobStatus::TimeoutPending => "⏱",
                        mylm_core::agent::v2::jobs::JobStatus::Stalled => "⚠️",
                    };
                    msg.push_str(&format!(
                        "{} {} | {} | {:?}\n",
                        status_icon,
                        &job.id[..8.min(job.id.len())],
                        job.description,
                        job.status
                    ));
                }
                if jobs.len() > 20 {
                    msg.push_str(&format!("\n... and {} more jobs", jobs.len() - 20));
                }
                self.chat_history.push(ChatMessage::assistant(msg));
            }
            "cancel" => {
                if parts.len() < 3 {
                    self.chat_history.push(ChatMessage::assistant(
                        "Usage: /jobs cancel <job-id>".to_string()
                    ));
                    return;
                }
                let job_id = parts[2];
                // Try to find job by partial ID
                let all_jobs = self.job_registry.list_all_jobs();
                let matched = all_jobs.iter().find(|j| {
                    j.id.starts_with(job_id) || j.id == job_id
                });
                
                if let Some(job) = matched {
                    if self.job_registry.cancel_job(&job.id) {
                        self.chat_history.push(ChatMessage::assistant(format!(
                            "🛑 Job '{}' ({}) cancelled successfully.",
                            &job.id[..8.min(job.id.len())],
                            job.description
                        )));
                    } else {
                        self.chat_history.push(ChatMessage::assistant(format!(
                            "Job '{}' is not running and cannot be cancelled.",
                            &job.id[..8.min(job.id.len())]
                        )));
                    }
                } else {
                    self.chat_history.push(ChatMessage::assistant(format!(
                        "Job '{}' not found. Use '/jobs list' to see available jobs.",
                        job_id
                    )));
                }
            }
            "cancel-all" => {
                let cancelled = self.job_registry.cancel_all_jobs();
                self.chat_history.push(ChatMessage::assistant(format!(
                    "🛑 Cancelled {} job(s).",
                    cancelled
                )));
            }
            _ => {
                self.chat_history.push(ChatMessage::assistant(format!(
                    "Unknown jobs command: {}. Use 'list', 'cancel <id>', or 'cancel-all'",
                    subcmd
                )));
            }
        }
    }
}
