//! Slash command handling for the terminal UI
use crate::tui::app::state::AppStateContainer;
use crate::tui::types::TuiEvent;
use mylm_core::llm::chat::ChatMessage;

use tokio::sync::mpsc::UnboundedSender;

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
        // Stub: System prompt dumping not available in new architecture
        let event_tx_clone = event_tx.clone();
        
        tokio::spawn(async move {
            let message = "System prompt dump not available in current architecture.".to_string();
            
            let _ = event_tx_clone.send(TuiEvent::AgentResponse(
                ChatMessage::assistant(message),
                mylm_core::llm::TokenUsage::default()
            ));
        });
    }

    fn handle_context_command(&mut self, event_tx: UnboundedSender<TuiEvent>) {
        let event_tx_clone = event_tx.clone();
        
        tokio::spawn(async move {
            let message = "Context dump not available in current architecture.".to_string();
            
            let _ = event_tx_clone.send(TuiEvent::AgentResponse(
                ChatMessage::assistant(message),
                mylm_core::llm::TokenUsage::default()
            ));
        });
    }

    fn handle_profile_command(&mut self, parts: &[&str], event_tx: UnboundedSender<TuiEvent>) {
        if parts.len() < 2 {
            let profiles: Vec<String> = self.config.profiles.keys().cloned().collect();
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
            self.config.active_profile = name.to_string();
            let _ = event_tx.send(TuiEvent::ConfigUpdate(name.to_string()));
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

        match key {
            "model" => {
                if let Some(profile) = self.config.profiles.get_mut(&self.config.active_profile) {
                    profile.model = Some(value.to_string());
                    updated = true;
                }
            }
            "max_iterations" => {
                if let Ok(iters) = value.parse::<usize>() {
                    if let Some(profile) = self.config.profiles.get_mut(&self.config.active_profile) {
                        profile.max_iterations = iters;
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
            let _ = event_tx.send(TuiEvent::ConfigUpdate(format!("{}={}", key, value)));
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

        self.state = crate::tui::app::state::AppState::ExecutingTool(command.clone());
        
        // /exec not yet implemented in new architecture
        self.chat_history.push(ChatMessage::assistant(
            format!("/exec not yet implemented in new architecture. Command: {}", command)
        ));
        self.state = crate::tui::app::state::AppState::Idle;
    }

    fn handle_help_command(&mut self) {
        self.chat_history.push(ChatMessage::assistant(
            "Available commands:\n\
            /profile <name> - Switch profile\n\
            /model <name> - Set model for active profile\n\
            /config <key> <value> - Update active profile\n\
            /exec <command> - Execute shell command (not yet implemented)\n\
            /jobs - List active jobs with metrics\n\
            /jobs cancel <id> - Cancel a specific job\n\
            /jobs cancel-all - Cancel all jobs\n\
            /jobs list - List all jobs\n\
            /prompt - Dump system prompt to file (not yet implemented)\n\
            /context - Dump LLM context to mylm/logs/ (not yet implemented)\n\
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
            let active_profile = &self.config.active_profile;
            let profile = self.config.active_profile();
            let provider = &profile.provider;
            let model = profile.model.as_deref().unwrap_or("default");

            self.chat_history.push(ChatMessage::assistant(format!(
                "Current profile: {}\nProvider: {}\nModel: {}\n\nUsage: /model <model-name> to set model, or /model clear to use default.",
                active_profile, provider, model
            )));
            return;
        }

        let value = parts[1];

        if value == "clear" {
            if let Some(profile) = self.config.profiles.get_mut(&self.config.active_profile) {
                profile.model = None;
                let _ = event_tx.send(TuiEvent::ConfigUpdate("model=clear".to_string()));
                self.chat_history.push(ChatMessage::assistant(format!(
                    "Model cleared for profile '{}'. Using default.",
                    self.config.active_profile
                )));
            }
        } else {
            if let Some(profile) = self.config.profiles.get_mut(&self.config.active_profile) {
                profile.model = Some(value.to_string());
                let _ = event_tx.send(TuiEvent::ConfigUpdate(format!("model={}", value)));
                self.chat_history.push(ChatMessage::assistant(format!(
                    "Model set to '{}' for profile '{}'",
                    value, self.config.active_profile
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
        // Logs not available in new architecture
        self.chat_history.push(ChatMessage::assistant(format!(
            "Recent Logs (last {}):\nLogs not available in current architecture.",
            n
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
                        self.pacore_rounds = rounds_clone.parse::<usize>().unwrap_or(3);
                        self.config.features.pacore.rounds = self.pacore_rounds;
                        let _ = self.config.save_default();
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
                self.config.features.pacore.rounds = self.pacore_rounds;
                match self.config.save_default() {
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
                        crate::tui::types::JobStatus::Running => "⏳",
                        crate::tui::types::JobStatus::Completed => "✅",
                        crate::tui::types::JobStatus::Failed => "❌",
                        crate::tui::types::JobStatus::Cancelled => "🛑",
                        crate::tui::types::JobStatus::TimeoutPending => "⏱",
                        crate::tui::types::JobStatus::Stalled => "⚠️",
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
