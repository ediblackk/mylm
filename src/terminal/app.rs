use crate::terminal::pty::PtyManager;
use mylm_core::config::ConfigUiExt;
use mylm_core::memory::graph::MemoryGraph;
use mylm_core::executor::{CommandExecutor, allowlist::CommandAllowlist, safety::SafetyChecker};
use mylm_core::llm::chat::{ChatMessage, MessageRole};
use mylm_core::llm::TokenUsage;
use mylm_core::agent::{Agent, AgentDecision, ToolKind};
use mylm_core::agent::v2::jobs::{JobRegistry, JobStatus};
use crate::terminal::session::SessionMonitor;
use crate::terminal::session_manager::SessionManager;
use mylm_core::context::pack::ContextBuilder;
use mylm_core::context::{ContextConfig, ContextManager};
use vt100::Parser;
use mylm_core::pacore::exp::Exp;
use futures::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use std::sync::RwLock;
use tokio::sync::Mutex;

use tokio::sync::mpsc;
pub use mylm_core::terminal::app::{AppState, TuiEvent};

#[derive(Debug, Clone)]
pub struct ActivityEntry {
    #[allow(dead_code)]
    pub at: Instant,
    #[allow(dead_code)]
    pub summary: String,
    #[allow(dead_code)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PendingStream {
    #[allow(dead_code)]
    pub started_at: Instant,
    pub chars: Vec<char>,
    pub rendered: usize,
    pub msg_index: usize,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Terminal,
    Chat,
}

pub struct App {
    pub terminal_parser: Parser,
    pub pty_manager: PtyManager,
    pub config: mylm_core::config::Config,
    pub agent: Arc<Mutex<Agent>>,
    pub chat_input: String,
    pub cursor_position: usize,
    pub chat_history: Vec<ChatMessage>,
    pub focus: Focus,
    pub state: AppState,
    pub should_quit: bool,
    pub return_to_hub: bool,
    pub chat_scroll: usize,
    pub chat_auto_scroll: bool,
    pub input_scroll: usize,
    pub session_monitor: SessionMonitor,
    pub terminal_scroll: usize,
    pub terminal_auto_scroll: bool,
    pub terminal_size: (u16, u16),
    pub status_message: Option<String>,
    pub state_started_at: Instant,
    pub activity_log: Vec<ActivityEntry>,
    pub pending_stream: Option<PendingStream>,
    pub interrupt_flag: Arc<AtomicBool>,
    pub verbose_mode: bool,
    pub show_thoughts: bool,
    pub auto_approve: Arc<AtomicBool>,
    pub active_task: Option<tokio::task::JoinHandle<()>>,
    pub capturing_command_output: bool,
    pub command_output_buffer: String,
    pub pending_command_tx: Option<tokio::sync::oneshot::Sender<String>>,
    pub input_price: f64,
    pub output_price: f64,
    pub tick_count: u64,
    pub terminal_history: Vec<String>,
    pub pending_echo_suppression: String,
    pub pending_clean_command: Option<String>,
    pub raw_buffer: Vec<u8>,
    pub session_id: String,
    pub show_memory_view: bool,
    pub memory_graph: MemoryGraph,
    pub memory_graph_scroll: usize,
    pub last_total_chat_lines: Option<usize>,
    pub show_help_view: bool,
    pub help_scroll: usize,
    pub update_available: bool,
    pub exit_name_input: String,
    // Job Panel state
    pub show_jobs_panel: bool,
    pub selected_job_index: Option<usize>,
    pub job_registry: JobRegistry,
    pub show_job_detail: bool,
    pub job_scroll: usize,
    // UI Layout state
    pub chat_width_percent: u16,
    pub show_terminal: bool,
    // Mouse selection state
    pub selection_start: Option<(u16, u16)>, // (x, y) in screen coordinates
    pub selection_end: Option<(u16, u16)>,
    pub selection_pane: Option<Focus>, // which pane the selection started in
    pub is_selecting: bool,
    // Pane offsets for coordinate translation
    pub terminal_area_offset: Option<(u16, u16)>, // (x, y) offset of terminal pane
    pub chat_area_offset: Option<(u16, u16)>,     // (x, y) offset of chat pane
    pub clipboard: Option<arboard::Clipboard>,
    pub scratchpad: Arc<RwLock<String>>,
    // PaCoRe (Parallel Consistency Reasoning) engine
            #[allow(dead_code)]
            pub pacore_engine: Option<Arc<Mutex<Option<Exp>>>>,
            pub pacore_enabled: bool,
            pub pacore_rounds: String,
            // PaCoRe progress tracking
            pub pacore_progress: Option<(usize, usize)>, // (completed, total)
            pub pacore_current_round: Option<(usize, usize)>, // (current, total)
            // Context manager for token tracking and condensation
            pub context_manager: ContextManager,
            // Session manager for automatic persistence
            pub session_manager: SessionManager,
            // Incognito mode flag
            pub incognito: bool,
    }

impl App {
    pub fn new(
        pty_manager: PtyManager,
        agent: Agent,
        config: mylm_core::config::Config,
        scratchpad: Arc<RwLock<String>>,
        job_registry: JobRegistry,
        incognito: bool,
    ) -> Self {
        // In V2, context limits and prices are not stored in config
        // Use sensible defaults
        let max_ctx = 128000_usize;
        let input_price = 0.0;
        let output_price = 0.0;
        
        let mut session_monitor = SessionMonitor::new();
        session_monitor.set_max_context(max_ctx as u32);
        let verbose_mode = false; // V2 doesn't have verbose_mode config
        let auto_approve = Arc::new(AtomicBool::new(false)); // V2 doesn't have allow_execution config
        let clipboard = arboard::Clipboard::new().ok();

        let session_id = agent.session_id.clone();
        
        // Extract PaCoRe config before moving config
        let pacore_enabled = config.features.pacore.enabled;
        let pacore_rounds = config.features.pacore.rounds.clone();

        // Initialize context manager with default config
        // Will be updated with proper config from agent's LLM client when available
        let context_manager = ContextManager::new(ContextConfig::new(max_ctx as usize));

        Self {
            terminal_parser: Parser::new(24, 80, 0), // Standard size, history handled separately
            pty_manager,
            config,
            agent: Arc::new(Mutex::new(agent)),
            chat_input: String::new(),
            cursor_position: 0,
            chat_history: Vec::new(),
            focus: Focus::Terminal,
            state: AppState::Idle,
            should_quit: false,
            return_to_hub: false,
            chat_scroll: 0,
            chat_auto_scroll: true,
            input_scroll: 0,
            session_monitor,
            terminal_scroll: 0,
            terminal_auto_scroll: true,
            terminal_size: (24, 80),
            status_message: None,
            state_started_at: Instant::now(),
            activity_log: Vec::new(),
            pending_stream: None,
            interrupt_flag: Arc::new(AtomicBool::new(false)),
            verbose_mode,
            show_thoughts: true,
            auto_approve,
            active_task: None,
            capturing_command_output: false,
            command_output_buffer: String::new(),
            pending_command_tx: None,
            input_price,
            output_price,
            tick_count: 0,
            terminal_history: Vec::new(),
            pending_echo_suppression: String::new(),
            pending_clean_command: None,
            raw_buffer: Vec::new(),
            session_id,
            show_memory_view: false,
            memory_graph: MemoryGraph::default(),
            memory_graph_scroll: 0,
            last_total_chat_lines: None,
            show_help_view: false,
            help_scroll: 0,
            update_available: false,
            exit_name_input: String::new(),
            // Job Panel state
            show_jobs_panel: false,
            selected_job_index: None,
            job_registry,
            show_job_detail: false,
            job_scroll: 0,
            // UI Layout state
            chat_width_percent: 30,
            show_terminal: true,
            // Mouse selection state
            selection_start: None,
            selection_end: None,
            selection_pane: None,
            is_selecting: false,
            terminal_area_offset: None,
            chat_area_offset: None,
            clipboard,
            scratchpad,
            // Initialize PaCoRe as None (lazy load on first use if enabled)
            pacore_engine: None,
            pacore_enabled,
            pacore_rounds,
            pacore_progress: None,
            pacore_current_round: None,
            context_manager,
            session_manager: SessionManager::new(),
            incognito,
        }
    }

    pub fn process_terminal_data(&mut self, data: &[u8]) {
        self.terminal_parser.process(data);
        self.raw_buffer.extend_from_slice(data);
    }

    pub fn resize_pty(&mut self, width: u16, height: u16) {
        self.terminal_size = (height, width);
        let _ = self.pty_manager.resize(height, width);
        
        // Re-create parser with new dimensions and replay history to fix wrapping
        let mut new_parser = Parser::new(height, width, 0);
        new_parser.process(&self.raw_buffer);
        self.terminal_parser = new_parser;
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Terminal => Focus::Chat,
            Focus::Chat => Focus::Terminal,
        };
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor_position = self.cursor_position.saturating_sub(1);
    }

    pub fn move_cursor_right(&mut self) {
        let char_count = self.chat_input.chars().count();
        if self.cursor_position < char_count {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.chat_input.chars().count();
    }

    pub fn enter_char(&mut self, new_char: char) {
        if new_char == '\r' { return; }
        
        if self.cursor_position >= self.chat_input.chars().count() {
            self.chat_input.push(new_char);
        } else {
            let byte_idx = self.chat_input.char_indices().nth(self.cursor_position).map(|(i, _)| i).unwrap_or(self.chat_input.len());
            self.chat_input.insert(byte_idx, new_char);
        }
        self.cursor_position += 1;
    }

    pub fn enter_string(&mut self, text: &str) {
        let clean_text = text.replace('\r', "");
        if clean_text.is_empty() { return; }

        // Large paste warning
        if clean_text.len() > 10_000 {
            self.status_message = Some("⚠️ Large paste detected. Consider using /read or asking AI to read the file for efficiency.".to_string());
        }

        if self.cursor_position >= self.chat_input.chars().count() {
            self.chat_input.push_str(&clean_text);
        } else {
            let byte_idx = self.chat_input.char_indices().nth(self.cursor_position).map(|(i, _)| i).unwrap_or(self.chat_input.len());
            self.chat_input.insert_str(byte_idx, &clean_text);
        }
        
        self.cursor_position += clean_text.chars().count();
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            let mut chars: Vec<char> = self.chat_input.chars().collect();
            chars.remove(self.cursor_position - 1);
            self.chat_input = chars.into_iter().collect();
            self.move_cursor_left();
        }
    }

    pub fn delete_at_cursor(&mut self) {
        let char_count = self.chat_input.chars().count();
        if self.cursor_position < char_count {
            let mut chars: Vec<char> = self.chat_input.chars().collect();
            chars.remove(self.cursor_position);
            self.chat_input = chars.into_iter().collect();
        }
    }

    pub fn reset_cursor(&mut self) {
        self.cursor_position = 0;
    }

    pub fn trigger_manual_condensation(&mut self, event_tx: mpsc::UnboundedSender<TuiEvent>) {
        if self.state == AppState::Idle {
            let agent = self.agent.clone();
            let history = self.chat_history.clone();
            tokio::spawn(async move {
                let agent = agent.lock().await;
                if let Ok(new_history) = agent.condense_history(&history).await {
                    let _ = event_tx.send(TuiEvent::CondensedHistory(new_history));
                }
            });
        }
    }

    pub async fn submit_message(&mut self, event_tx: mpsc::UnboundedSender<TuiEvent>) {
        if !self.chat_input.is_empty() {
            // Abort any existing task before starting a new one
            self.abort_current_task();
            self.status_message = None;
            
            let input = self.chat_input.clone();

            // Handle Slash Commands
            if input.starts_with('/') {
                self.handle_slash_command(&input, event_tx);
                self.chat_input.clear();
                self.reset_cursor();
                return;
            }

            // Build prompt with context packs
            let history_height = 5000; // Capture more for the builder to decide
            let width = self.terminal_size.1;
            let mut temp_parser = Parser::new(history_height, width, 0);
            temp_parser.process(&self.raw_buffer);
            let terminal_content = temp_parser.screen().contents();

            // V2 doesn't have context_profile, use Balanced as default
            let builder = ContextBuilder::new(mylm_core::config::ContextProfile::Balanced);
            let mut final_message = input.clone();

            // 1. Terminal Pack
            if let Some(pack) = builder.build_terminal_pack(&terminal_content) {
                final_message.push_str(&pack.render());
            }

            self.chat_history.push(ChatMessage::user(&final_message));

            // Auto-save session after user message (fire-and-forget)
            if !self.incognito {
                let session = self.build_current_session().await;
                self.session_manager.set_current_session(session);
            }

            self.chat_input.clear();
            self.reset_cursor();
            self.input_scroll = 0;
            self.state = AppState::Thinking("...".to_string());
            // Reset scroll to bottom on new message
            self.chat_scroll = 0;
            self.chat_auto_scroll = true;

            // Check if PaCoRe is enabled
            if self.pacore_enabled {
                // Spawn task to run PaCoRe reasoning
                let agent = self.agent.clone();
                let history = self.chat_history.clone();
                let event_tx_clone = event_tx.clone();
                let interrupt_flag = self.interrupt_flag.clone();
                let pacore_rounds = self.pacore_rounds.clone();
                let config = self.config.clone();
                
                interrupt_flag.store(false, Ordering::SeqCst);
                
                let task = tokio::spawn(async move {
                    run_pacore_task(
                        agent,
                        history,
                        event_tx_clone,
                        interrupt_flag,
                        &pacore_rounds,
                        config,
                    ).await;
                });
                
                self.active_task = Some(task);
            } else {
                // Standard agent loop
                let agent = self.agent.clone();
                let history = self.chat_history.clone();
                // Update context manager with current history for ratio calculation
                self.context_manager.set_history(&history);
                let context_ratio = self.context_manager.get_context_ratio();
                let event_tx_clone = event_tx.clone();
                let interrupt_flag = self.interrupt_flag.clone();
                let auto_approve = self.auto_approve.clone();
                // V2 doesn't have max_driver_loops, use default
                let max_driver_loops = 30;
                interrupt_flag.store(false, Ordering::SeqCst);

                let task = tokio::spawn(async move {
                    {
                        let mut agent_lock = agent.lock().await;
                        
                        // Automatic condensation check
                        let final_history = if context_ratio > agent_lock.llm_client.config().condense_threshold {
                            match agent_lock.condense_history(&history).await {
                                Ok(new_history) => new_history,
                                Err(_) => history,
                            }
                        } else {
                            history
                        };

                        agent_lock.reset(final_history).await;
                    }
                    
                    run_agent_loop(
                        agent,
                        event_tx_clone,
                        interrupt_flag,
                        auto_approve,
                        max_driver_loops,
                        None
                    ).await;
                });
                
                self.active_task = Some(task);
            }
        }
    }

    pub fn abort_current_task(&mut self) {
        mylm_core::info_log!("App: Aborting current task");
        if let Some(task) = self.active_task.take() {
            if !task.is_finished() {
                task.abort();
                self.status_message = Some("⛔ Task interrupted by user.".to_string());
                self.interrupt_flag.store(true, Ordering::SeqCst);
            }
        }
        
        if let Some(tx) = self.pending_command_tx.take() {
            mylm_core::debug_log!("App: Aborting pending terminal command");
            let _ = tx.send("Error: Command aborted by user".to_string());
            
            // Try to recover terminal state if we were capturing
            if self.capturing_command_output {
                let _ = self.pty_manager.write_all(&[3, 13]); // Ctrl+C, Enter
                let _ = self.pty_manager.write_all(b"([ -t 0 ] && stty echo) 2>/dev/null\r");
            }
        }
        self.capturing_command_output = false;
        self.state = AppState::Idle;
    }

    fn handle_slash_command(&mut self, input: &str, event_tx: mpsc::UnboundedSender<TuiEvent>) {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts[0];

        match cmd {
            "/profile" => {
                if parts.len() < 2 {
                    let profiles: Vec<String> = self.config.profile_names();
                    self.chat_history.push(ChatMessage::assistant("Usage: /profile <name>\nAvailable profiles: ".to_string() +
                        &profiles.join(", ")));
                    return;
                }
                let name = parts[1];
                if self.config.profiles.contains_key(name) {
                    self.config.profile = name.to_string();
                    let _ = event_tx.send(TuiEvent::ConfigUpdate(self.config.clone()));
                    self.chat_history.push(ChatMessage::assistant(format!("Switched to profile: {}", name)));
                } else {
                    self.chat_history.push(ChatMessage::assistant(format!("Profile '{}' not found", name)));
                }
            }
            "/config" => {
                if parts.len() < 3 {
                    self.chat_history.push(ChatMessage::assistant("Usage: /config <key> <value>\nKeys: model, max_iterations".to_string()));
                    return;
                }
                let key = parts[1];
                let value = parts[2];
                
                let mut updated = false;
                let active_profile_name = self.config.profile.clone();
                
                match key {
                    "model" => {
                        // Set model override for active profile
                        if let Err(e) = self.config.set_profile_model_override(&active_profile_name, Some(value.to_string())) {
                            self.chat_history.push(ChatMessage::assistant(format!("Error: {}", e)));
                        } else {
                            updated = true;
                        }
                    }
                    "max_iterations" => {
                        if let Ok(iters) = value.parse::<usize>() {
                            if let Err(e) = self.config.set_profile_max_iterations(&active_profile_name, Some(iters)) {
                                self.chat_history.push(ChatMessage::assistant(format!("Error: {}", e)));
                            } else {
                                updated = true;
                            }
                        } else {
                            self.chat_history.push(ChatMessage::assistant("max_iterations must be a number".to_string()));
                        }
                    }
                    _ => {
                        self.chat_history.push(ChatMessage::assistant(format!("Unknown config key: {}", key)));
                    }
                }
                
                if updated {
                    let _ = event_tx.send(TuiEvent::ConfigUpdate(self.config.clone()));
                    self.chat_history.push(ChatMessage::assistant(format!("Updated {} to {}", key, value)));
                }
            }
            "/exec" => {
                if parts.len() < 2 {
                    self.chat_history.push(ChatMessage::assistant("Usage: /exec <command>".to_string()));
                    return;
                }
                let command = parts[1..].join(" ");
                
                self.state = AppState::ExecutingTool(command.clone());
                let agent = self.agent.clone();
                let event_tx_clone = event_tx.clone();
                let interrupt_flag = self.interrupt_flag.clone();
                interrupt_flag.store(false, Ordering::SeqCst);

                let auto_approve = self.auto_approve.clone();
                let max_driver_loops = 30; // V2 doesn't have max_driver_loops
                let task = tokio::spawn(async move {
                    // Manual execution via /exec
                    // Safety check
                    let allowlist = CommandAllowlist::new();
                    let executor = CommandExecutor::new(allowlist, SafetyChecker::new());
                    let last_obs = if let Err(e) = executor.check_safety(&command) {
                        let err_msg = format!("Error: Safety Check Failed: {}", e);
                        // Log failure to terminal
                        let err_log = format!("\r\n\x1b[31m[Safety Check Failed]:\x1b[0m {}\r\n", e);
                        let _ = event_tx_clone.send(TuiEvent::PtyWrite(err_log.into_bytes()));
                        Some(err_msg)
                    } else {
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        let _ = event_tx_clone.send(TuiEvent::ExecuteTerminalCommand(command, tx));
                        
                        match rx.await {
                            Ok(out) => Some(out),
                            Err(_) => Some("Error: Execution failed".to_string()),
                        }
                    };

                    // Resume agent loop with this observation
                    run_agent_loop(
                        agent,
                        event_tx_clone,
                        interrupt_flag,
                        auto_approve, // Now correctly respects auto_approve for future steps!
                        max_driver_loops,
                        last_obs
                    ).await;
                });
                self.active_task = Some(task);
            }
            "/help" => {
                self.chat_history.push(ChatMessage::assistant(
                    "Available commands:\n\
                    /profile <name> - Switch profile\n\
                    /model <name> - Set model for active profile\n\
                    /config <key> <value> - Update active profile\n\
                    /exec <command> - Execute shell command\n\
                    /verbose - Toggle verbose mode\n\
                    /help - Show this help\n\n\
                    Input Shortcuts:\n\
                    Ctrl+a / Home - Start of line\n\
                    Ctrl+e / End - End of line\n\
                    Ctrl+k - Kill to end\n\
                    Ctrl+u - Kill to start\n\
                    Arrows - Navigate lines/history"
                    .to_string()
                ));
            }
            "/model" => {
                if parts.len() < 2 {
                    // Show current model info
                    let active_profile = self.config.profile.clone();
                    let effective = self.config.get_effective_endpoint_info();
                    let base = self.config.get_endpoint_info();
                    let profile_info = self.config.get_profile_info(&active_profile);
                    
                    let model_source = if profile_info.as_ref().and_then(|p| p.model_override.as_ref()).is_some() {
                        "profile override"
                    } else {
                        "base endpoint"
                    };
                    
                    self.chat_history.push(ChatMessage::assistant(
                        format!("Current model: {} ({} via {})\nBase endpoint model: {}\n\nUsage: /model <model-name> to set profile model override, or /model clear to use base endpoint model.",
                            effective.model, model_source, active_profile, base.model)
                    ));
                    return;
                }
                
                let value = parts[1];
                let active_profile_name = self.config.profile.clone();
                
                if value == "clear" {
                    // Clear profile model override
                    if let Err(e) = self.config.set_profile_model_override(&active_profile_name, None) {
                        self.chat_history.push(ChatMessage::assistant(format!("Error: {}", e)));
                    } else {
                        let _ = event_tx.send(TuiEvent::ConfigUpdate(self.config.clone()));
                        self.chat_history.push(ChatMessage::assistant(
                            format!("Model override cleared for profile '{}'. Using base endpoint model.", active_profile_name)
                        ));
                    }
                } else {
                    // Set profile model override
                    if let Err(e) = self.config.set_profile_model_override(&active_profile_name, Some(value.to_string())) {
                        self.chat_history.push(ChatMessage::assistant(format!("Error: {}", e)));
                    } else {
                        let _ = event_tx.send(TuiEvent::ConfigUpdate(self.config.clone()));
                        self.chat_history.push(ChatMessage::assistant(
                            format!("Model set to '{}' for profile '{}' (profile override)", value, active_profile_name)
                        ));
                    }
                }
            }
            "/verbose" => {
                self.verbose_mode = !self.verbose_mode;
                let status = if self.verbose_mode { "ON" } else { "OFF" };
                self.chat_history.push(ChatMessage::assistant(format!("Verbose mode: {}", status)));
            }
            "/logs" => {
                let n = parts.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(20);
                let logs = mylm_core::agent::logger::get_recent_logs(n);
                let log_text = if logs.is_empty() {
                    "No logs found.".to_string()
                } else {
                    logs.join("\n")
                };
                self.chat_history.push(ChatMessage::assistant(format!("Recent Logs (last {}):\n{}", n, log_text)));
            }
            "/pacore" => {
                if parts.len() < 2 {
                    // Show current PaCoRe status
                    let status = if self.pacore_enabled { "ON" } else { "OFF" };
                    self.chat_history.push(ChatMessage::assistant(
                        format!("PaCoRe Status:\n  Enabled: {}\n  Rounds: {}\n\nCommands:\n  /pacore on - Enable PaCoRe\n  /pacore off - Disable PaCoRe\n  /pacore rounds <n,n> - Set rounds (e.g., '4,1')\n  /pacore status - Show this status\n  /pacore save - Save config to disk",
                            status, self.pacore_rounds)
                    ));
                    return;
                }
                let subcmd = parts[1];
                match subcmd {
                    "on" => {
                        self.pacore_enabled = true;
                        self.chat_history.push(ChatMessage::assistant("PaCoRe enabled. New messages will use parallel reasoning.".to_string()));
                    }
                    "off" => {
                        self.pacore_enabled = false;
                        self.chat_history.push(ChatMessage::assistant("PaCoRe disabled. Using standard agent loop.".to_string()));
                    }
                    "rounds" => {
                        if parts.len() < 3 {
                            self.chat_history.push(ChatMessage::assistant("Usage: /pacore rounds <comma-separated numbers> (e.g., 4,1)".to_string()));
                        } else {
                            let new_rounds = parts[2..].join("");
                            // Validate format
                            if new_rounds.split(',').all(|s| s.trim().parse::<usize>().is_ok()) {
                                // Clone before moving for display and config update
                                let rounds_clone = new_rounds.clone();
                                self.pacore_rounds = new_rounds;
                                // Update config
                                self.config.features.pacore.rounds = rounds_clone.clone();
                                let _ = self.config.save(None);
                                self.chat_history.push(ChatMessage::assistant(format!("PaCoRe rounds set to: {}", rounds_clone)));
                            } else {
                                self.chat_history.push(ChatMessage::assistant("Invalid rounds format. Use comma-separated numbers (e.g., 4,1)".to_string()));
                            }
                        }
                    }
                    "status" => {
                        let status = if self.pacore_enabled { "ON" } else { "OFF" };
                        self.chat_history.push(ChatMessage::assistant(
                            format!("PaCoRe Status:\n  Enabled: {}\n  Rounds: {}", status, self.pacore_rounds)
                        ));
                    }
                    "save" => {
                        self.config.features.pacore.rounds = self.pacore_rounds.clone();
                        match self.config.save(None) {
                            Ok(_) => {
                                self.chat_history.push(ChatMessage::assistant("PaCoRe configuration saved.".to_string()));
                            }
                            Err(e) => {
                                self.chat_history.push(ChatMessage::assistant(format!("Error saving config: {}", e)));
                            }
                        }
                    }
                    _ => {
                        self.chat_history.push(ChatMessage::assistant(format!("Unknown pacore command: {}. Use 'on', 'off', 'rounds', 'status', or 'save'", subcmd)));
                    }
                }
            }
            _ => {
                self.chat_history.push(ChatMessage::assistant(format!("Unknown command: {}", cmd)));
            }
        }
    }

    pub fn scroll_chat_up(&mut self) {
        self.chat_scroll = self.chat_scroll.saturating_add(1);
        self.chat_auto_scroll = false;
    }

    pub fn scroll_chat_down(&mut self) {
        self.chat_scroll = self.chat_scroll.saturating_sub(1);
        if self.chat_scroll == 0 {
            self.chat_auto_scroll = true;
        }
    }

    pub fn scroll_terminal_up(&mut self) {
        self.terminal_scroll = self.terminal_scroll.saturating_add(1);
        self.terminal_auto_scroll = false;
    }

    pub fn scroll_terminal_down(&mut self) {
        self.terminal_scroll = self.terminal_scroll.saturating_sub(1);
        if self.terminal_scroll == 0 {
            self.terminal_auto_scroll = true;
        }
    }

    pub fn set_history(&mut self, history: Vec<ChatMessage>) {
        self.chat_history = history;
    }

    pub fn add_assistant_message(&mut self, content: String, usage: TokenUsage) {
        self.chat_history.push(ChatMessage::assistant(content));
        
        let input_price = self.input_price;
        let output_price = self.output_price;

        self.session_monitor.add_usage(&usage, input_price, output_price);
        // CRITICAL BUG: This was setting state to Idle even if the agent loop was still running (e.g. after a Thought)
        // We should probably NOT set Idle here, but rely on run_agent_loop to send AppStateUpdate(Idle)
        // self.state = AppState::Idle; 
        
        // Reset scroll to bottom when response arrives if we were already at bottom
        if self.chat_auto_scroll {
            self.chat_scroll = 0;
        }
    }

    /// Build a Session object from current state
    async fn build_current_session(&self) -> crate::terminal::session::Session {
        let stats = self.session_monitor.get_stats();
        let preview = self.chat_history.iter()
            .rev()
            .find(|m| m.role == mylm_core::llm::chat::MessageRole::Assistant)
            .map(|m| m.content.chars().take(100).collect::<String>())
            .unwrap_or_else(|| "New Session".to_string());

        let agent = self.agent.lock().await;

        crate::terminal::session::Session {
            id: self.session_id.clone(),
            timestamp: chrono::Utc::now(),
            history: self.chat_history.clone(),
            metadata: crate::terminal::session::SessionMetadata {
                last_message_preview: preview,
                message_count: self.chat_history.len(),
                total_tokens: stats.total_tokens,
                input_tokens: stats.input_tokens,
                output_tokens: stats.output_tokens,
                cost: stats.cost,
                elapsed_seconds: self.session_monitor.duration().as_secs(),
            },
            terminal_history: self.raw_buffer.clone(),
            agent_session_id: agent.session_id.clone(),
            agent_history: agent.history.clone(),
        }
    }

    pub async fn save_session(&self, custom_name: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
        if self.incognito {
            return Ok(());
        }
        
        let stats = self.session_monitor.get_stats();
        let preview = self.chat_history.iter()
            .rev()
            .find(|m| m.role == mylm_core::llm::chat::MessageRole::Assistant)
            .map(|m| m.content.chars().take(100).collect::<String>())
            .unwrap_or_else(|| "New Session".to_string());

        let now = chrono::Utc::now();
        let agent = self.agent.lock().await;
        let session = crate::terminal::session::Session {
            id: custom_name.clone().unwrap_or_else(|| self.session_id.clone()),
            timestamp: now,
            history: self.chat_history.clone(),
            metadata: crate::terminal::session::SessionMetadata {
                last_message_preview: preview,
                message_count: self.chat_history.len(),
                total_tokens: stats.total_tokens,
                input_tokens: stats.input_tokens,
                output_tokens: stats.output_tokens,
                cost: stats.cost,
                elapsed_seconds: self.session_monitor.duration().as_secs(),
            },
            terminal_history: self.raw_buffer.clone(),
            agent_session_id: agent.session_id.clone(),
            agent_history: agent.history.clone(),
        };

        // Delegate to SessionManager for async persistence
        self.session_manager.set_current_session(session);
        
        Ok(())
    }

    pub async fn load_session(id: Option<&str>) -> Result<crate::terminal::session::Session, Box<dyn std::error::Error>> {
        // Handle None case (load latest) separately
        if id.is_none() {
            if let Some(latest) = crate::terminal::session_manager::SessionManager::load_latest().await {
                return Ok(latest);
            }
            return Err("No latest session found".into());
        }
        
        let target_id = id.unwrap();
        let target_clean = if target_id.ends_with(".json") {
            target_id.trim_end_matches(".json")
        } else {
            target_id
        };
        
        // Use SessionManager to load all sessions
        let sessions = crate::terminal::session_manager::SessionManager::load_sessions();
        
        // Find matching session in a single pass
        for session in sessions {
            if session.id == target_clean || 
               session.id.ends_with(&format!("_{}", target_clean)) ||
               session.id.contains(target_clean) {
                return Ok(session);
            }
        }
        
        Err("Session not found".into())
    }

    pub fn handle_terminal_input(&mut self, bytes: &[u8]) {
        let _ = self.pty_manager.write_all(bytes);
        // Reset terminal scroll on input if auto-scroll is enabled or just to be helpful
        self.terminal_scroll = 0;
        self.terminal_auto_scroll = true;
    }

    pub fn copy_text_to_clipboard(&mut self, text: String) {
        // Try arboard first
        if let Some(clipboard) = &mut self.clipboard {
            if clipboard.set_text(text.clone()).is_ok() {
                self.status_message = Some("Copied to clipboard".into());
                return;
            }
        }

        // Fallback to file
        let path = "/tmp/mylm-clipboard.txt";
        match std::fs::write(path, &text) {
            Ok(_) => {
                self.status_message = Some(format!("Clipboard unavailable; wrote to {}", path));
            }
            Err(e) => {
                self.status_message = Some(format!("Clipboard error & file write failed: {}", e));
            }
        }
    }

    pub fn copy_last_ai_response_to_clipboard(&mut self) {
        if let Some(msg) = self.chat_history.iter().rev().find(|m| m.role == MessageRole::Assistant) {
            self.copy_text_to_clipboard(msg.content.clone());
        } else {
            self.status_message = Some("⚠️ No AI response to copy".to_string());
        }
    }

    pub fn copy_terminal_buffer_to_clipboard(&mut self) {
         let history_height = 5000;
         let width = self.terminal_size.1;
         let mut temp_parser = Parser::new(history_height, width, 0);
         temp_parser.process(&self.raw_buffer);
         let content = temp_parser.screen().contents();
         self.copy_text_to_clipboard(content);
    }

    pub fn copy_visible_conversation_to_clipboard(&mut self) {
        let mut transcript = String::new();
        for msg in &self.chat_history {
            match msg.role {
                MessageRole::User => {
                    if !transcript.is_empty() {
                        transcript.push_str("\n\n");
                    }
                    transcript.push_str("User: ");
                    transcript.push_str(&msg.content);
                }
                MessageRole::Assistant => {
                    if !transcript.is_empty() {
                        transcript.push_str("\n\n");
                    }
                    transcript.push_str("AI: ");
                    transcript.push_str(&msg.content);
                }
                _ => {
                    // Skip System, Tool, and any other non-visible roles
                }
            }
        }
        if transcript.is_empty() {
            self.status_message = Some("⚠️ No conversation to copy".to_string());
        } else {
            self.copy_text_to_clipboard(transcript);
        }
    }

    pub fn set_state(&mut self, state: AppState) {
        // Reset PaCoRe progress when going idle
        let is_idle = matches!(state, AppState::Idle);
        
        self.state = state;
        self.state_started_at = Instant::now();
        
        if is_idle {
            self.pacore_progress = None;
            self.pacore_current_round = None;
        }
    }

    pub fn push_activity(&mut self, summary: impl Into<String>, detail: Option<String>) {
        self.activity_log.push(ActivityEntry {
            at: Instant::now(),
            summary: summary.into(),
            detail,
        });
        if self.activity_log.len() > 200 {
            let overflow = self.activity_log.len() - 200;
            self.activity_log.drain(0..overflow);
        }
    }

    pub async fn start_streaming_final_answer(&mut self, content: String, usage: TokenUsage) {
        // Insert a placeholder assistant message and fill it incrementally from ticks.
        self.chat_history.push(ChatMessage::assistant(String::new()));

        // Auto-save session after assistant message (fire-and-forget)
        if !self.incognito {
            let session = self.build_current_session().await;
            self.session_manager.set_current_session(session);
        }

        let msg_index = self.chat_history.len().saturating_sub(1);
        self.pending_stream = Some(PendingStream {
            started_at: Instant::now(),
            chars: content.chars().collect(),
            rendered: 0,
            msg_index,
            usage,
        });
        self.set_state(AppState::Streaming("Answer".to_string()));
    }

    // UI Layout helpers
    pub fn adjust_chat_width(&mut self, delta: i16) {
        let new_width = self.chat_width_percent as i16 + delta;
        self.chat_width_percent = new_width.clamp(20, 100) as u16;
    }

    // Selection helpers
    pub fn start_selection(&mut self, x: u16, y: u16, pane: Focus) {
        self.selection_start = Some((x, y));
        self.selection_end = Some((x, y));
        self.selection_pane = Some(pane);
        self.is_selecting = true;
    }

    pub fn update_selection(&mut self, x: u16, y: u16) {
        if self.is_selecting {
            self.selection_end = Some((x, y));
        }
    }

    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let result = self.get_selected_text();
        // Clear selection after copying
        self.selection_start = None;
        self.selection_end = None;
        self.selection_pane = None;
        result
    }

    fn get_selected_text(&self) -> Option<String> {
        let (start, end, pane) = match (self.selection_start, self.selection_end, self.selection_pane) {
            (Some(s), Some(e), Some(p)) => (s, e, p),
            _ => return None,
        };

        // Normalize selection (start should be top-left, end bottom-right)
        let (x1, y1) = start;
        let (x2, y2) = end;
        let ((start_x, start_y), (end_x, end_y)) = if y1 < y2 || (y1 == y2 && x1 <= x2) {
            ((x1, y1), (x2, y2))
        } else {
            ((x2, y2), (x1, y1))
        };

        match pane {
            Focus::Terminal => self.get_terminal_selected_text(start_x, start_y, end_x, end_y),
            Focus::Chat => self.get_chat_selected_text(start_x, start_y, end_x, end_y),
        }
    }

    fn get_terminal_selected_text(&self, start_x: u16, start_y: u16, end_x: u16, end_y: u16) -> Option<String> {
        let (offset_x, offset_y) = self.terminal_area_offset.unwrap_or((0, 0));
        
        // Screen dimensions (inner terminal area)
        let (screen_rows, _screen_cols) = self.terminal_parser.screen().size();
        
        // Reconstruct all lines including history
        let mut all_lines = Vec::new();
        for h in &self.terminal_history {
            all_lines.push(h.clone());
        }
        let screen_contents = self.terminal_parser.screen().contents();
        for s in screen_contents.split('\n') {
            all_lines.push(s.to_string());
        }

        let total_lines = all_lines.len();
        let height = screen_rows as usize;
        
        // If we're not auto-scrolling, we're showing a window into the history
        let start_idx = if self.terminal_auto_scroll {
            total_lines.saturating_sub(height)
        } else {
            let max_scroll = total_lines.saturating_sub(height);
            let clamped_scroll = self.terminal_scroll.min(max_scroll);
            total_lines.saturating_sub(clamped_scroll).saturating_sub(height)
        };

        let mut lines = Vec::new();
        for y in start_y..=end_y {
            let abs_y = start_idx + (y.saturating_sub(offset_y) as usize).saturating_sub(1);
            if let Some(line) = all_lines.get(abs_y) {
                let col_start = if y == start_y { start_x.saturating_sub(offset_x).saturating_sub(1) as usize } else { 0 };
                let col_end = if y == end_y { (end_x.saturating_sub(offset_x).saturating_sub(1) as usize).min(line.chars().count()) } else { line.chars().count() };
                
                let chars: Vec<char> = line.chars().collect();
                if col_start < chars.len() {
                    let part: String = chars[col_start..col_end.min(chars.len())].iter().collect();
                    lines.push(part);
                } else if col_start == 0 && chars.is_empty() {
                    lines.push(String::new());
                }
            }
        }

        Some(lines.join("\n"))
    }

    fn get_chat_selected_text(&self, _start_x: u16, _start_y: u16, _end_x: u16, _end_y: u16) -> Option<String> {
        // Reconstruct visible chat lines (same logic as ui.rs)
        let mut all_lines = Vec::new();
        for m in &self.chat_history {
            let prefix = match m.role {
                MessageRole::User => "You: ",
                MessageRole::Assistant => "AI: ",
                MessageRole::System => "Sys: ",
                _ => "AI: ",
            };
            all_lines.push(format!("{}{}", prefix, m.content));
        }

        if all_lines.is_empty() {
            return None;
        }

        Some(all_lines.join("\n\n"))
    }

    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.selection_pane = None;
        self.is_selecting = false;
    }

    pub fn is_in_selection(&self, x: u16, y: u16, pane: Focus) -> bool {
        let (start, end, p) = match (self.selection_start, self.selection_end, self.selection_pane) {
            (Some(s), Some(e), Some(p)) => (s, e, p),
            _ => return false,
        };

        if p != pane {
            return false;
        }

        let (x1, y1) = start;
        let (x2, y2) = end;
        let ((min_x, min_y), (max_x, max_y)) = if y1 < y2 || (y1 == y2 && x1 <= x2) {
            ((x1, y1), (x2, y2))
        } else {
            ((x2, y2), (x1, y1))
        };

        if y < min_y || y > max_y {
            return false;
        }

        if y == min_y && y == max_y {
            return x >= min_x && x <= max_x;
        }

        if y == min_y {
            return x >= min_x;
        }

        if y == max_y {
            return x <= max_x;
        }

        true
    }
}

async fn run_agent_loop(
    agent: Arc<Mutex<Agent>>,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    interrupt_flag: Arc<AtomicBool>,
    auto_approve_flag: Arc<AtomicBool>,
    max_driver_loops: usize,
    mut last_observation: Option<String>,
) {
    let mut loop_iteration = 0;
    let mut retry_count = 0;
    let max_retries = 3;

    loop {
        loop_iteration += 1;
        if loop_iteration > max_driver_loops {
            let _ = event_tx.send(TuiEvent::AgentResponse(format!("Error: Driver-level safety limit reached ({} loops). Potential infinite loop detected.", max_driver_loops), TokenUsage::default()));
            let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
            break;
        }

        if interrupt_flag.load(Ordering::SeqCst) {
            let _ = event_tx.send(TuiEvent::AgentResponse("⛔ Task interrupted by user.".to_string(), TokenUsage::default()));
            let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
            break;
        }

        // Poll for completed background jobs and convert to observation
        let completed_jobs = {
            let agent_lock = agent.lock().await;
            agent_lock.job_registry.poll_updates()
        };
        if !completed_jobs.is_empty() {
            let mut observations = Vec::new();
            for job in completed_jobs {
                match job.status {
                    JobStatus::Completed => {
                        let result_str = job.result.as_ref()
                            .map(|r| r.to_string())
                            .unwrap_or_else(|| "Job completed successfully".to_string());
                        observations.push(format!("Background job '{}' result: {}", job.description, result_str));
                    }
                    JobStatus::Failed => {
                        let error_msg = job.error.as_ref()
                            .map(|e| e.as_str())
                            .unwrap_or("Unknown error");
                        observations.push(format!("Background job '{}' failed: {}", job.description, error_msg));
                    }
                    _ => {} // Running or other status, skip
                }
            }
            if !observations.is_empty() {
                if let Some(existing) = last_observation.take() {
                    last_observation = Some(format!("{}\n{}", existing, observations.join("\n")));
                } else {
                    last_observation = Some(observations.join("\n"));
                }
            }
        }

        let mut agent_lock = agent.lock().await;
        let provider = agent_lock.llm_client.config().provider.to_string();
        let model = agent_lock.llm_client.config().model.clone();
        let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Thinking(format!("{} ({})", model, provider))));
        let _ = event_tx.send(TuiEvent::ActivityUpdate {
            summary: "Thinking".to_string(),
            detail: Some(format!("Model: {} | Provider: {}", model, provider)),
        });
        
        let step_res = agent_lock.step(last_observation.take()).await;
        match step_res {
            Ok(AgentDecision::MalformedAction(error)) => {
                retry_count += 1;
                if retry_count > max_retries {
                    let fatal_error = format!("Fatal: Failed to parse agent response after {} attempts. Last error: {}", max_retries, error);
                    let _ = event_tx.send(TuiEvent::StatusUpdate(fatal_error.clone()));
                    let _ = event_tx.send(TuiEvent::AgentResponseFinal(fatal_error, TokenUsage::default()));
                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
                    break;
                }

                let _ = event_tx.send(TuiEvent::StatusUpdate(format!("⚠️ {} Retrying ({}/{})", error, retry_count, max_retries)));
                
                // Nudge the model to follow the format
                let nudge = format!(
                    "{}\n\n\
                    IMPORTANT: You must follow the ReAct format exactly:\n\
                    Thought: <your reasoning>\n\
                    Action: <tool name>\n\
                    Action Input: <tool arguments>\n\n\
                    Do not include any other text after Action Input.",
                    error
                );
                last_observation = Some(nudge);
                drop(agent_lock);
                continue;
            }
            Ok(AgentDecision::Message(msg, usage)) => {
                let has_pending = agent_lock.has_pending_decision();
                if has_pending {
                    retry_count = 0;
                    let _ = event_tx.send(TuiEvent::AgentResponse(msg, usage));
                    drop(agent_lock);
                    continue;
                }

                // If the model produced a message and no tool action is pending,
                // we treat it as a terminal conversational response.
                // This prevents "nudge" loops when the model answers without an explicit 'Final Answer:' tag.
                let _ = event_tx.send(TuiEvent::AgentResponseFinal(msg, usage));
                break;
            }
            Ok(AgentDecision::Action { tool, args, kind }) => {
                retry_count = 0;
                let _ = event_tx.send(TuiEvent::StatusUpdate(format!("Tool: '{}'", tool)));
                let _ = event_tx.send(TuiEvent::ActivityUpdate {
                    summary: format!("Tool: {}", tool),
                    detail: Some(args.clone()),
                });
                
                if kind == ToolKind::Terminal {
                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::ExecutingTool(tool.clone())));
                    
                    // Prepare command for execution
                    let cmd = if tool == "execute_command" {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                            v.get("command").and_then(|c| c.as_str())
                             .or_else(|| v.get("args").and_then(|c| c.as_str()))
                             .unwrap_or(&args).to_string()
                        } else {
                            args.clone()
                        }
                    } else {
                        format!("{} {}", tool, args)
                    };

                    if !auto_approve_flag.load(Ordering::SeqCst) {
                        let _ = event_tx.send(TuiEvent::SuggestCommand(cmd));
                        let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::WaitingForUser));
                        let _ = event_tx.send(TuiEvent::ActivityUpdate {
                            summary: "Waiting for approval".to_string(),
                            detail: Some("Auto-approve is OFF".to_string()),
                        });
                        break; // Stop and wait for manual approval
                    }

                    // Execute visibly
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = event_tx.send(TuiEvent::ExecuteTerminalCommand(cmd.clone(), tx));
                    
                    // Drop lock before awaiting
                    drop(agent_lock);

                    // Wait for result
                    match rx.await {
                        Ok(output) => {
                            last_observation = Some(output.clone());
                            // Record memory and categorize
                            let agent_lock = agent.lock().await;
                            if let Some(store) = &agent_lock.memory_store {
                                if let Ok(memory_id) = store.record_command(&cmd, &output, 0, Some(agent_lock.session_id.clone())).await {
                                    // Auto categorize is always enabled in V2
                                    let content = format!("Command: {}\nOutput: {}", cmd, output);
                                    let _ = agent_lock.auto_categorize(memory_id, &content).await;
                                }
                            }
                        }
                        Err(_) => {
                            mylm_core::error_log!("run_agent_loop: Terminal command execution failed (channel closed) for cmd: {}", cmd);
                            last_observation = Some("Error: Terminal command execution failed (channel closed)".to_string());
                        }
                    }
                } else {
                    // Internal tool
                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::ExecutingTool(tool.clone())));
                    
                    // Parse retry logic for internal tools
                    let mut observation = String::new();
                    let mut success = false;
                    let mut retry_count = 0;
                    
                    while !success && retry_count < 2 {
                        let t_args = if retry_count == 0 { args.clone() } else {
                            // If it failed once, it might be a JSON formatting issue.
                            // We can't easily "fix" the args here without a new LLM call,
                            // but ShellTool/others already handle basic ReAct fallback.
                            args.clone()
                        };

                        let call_res = match agent_lock.tool_registry.execute_tool(&tool, &t_args).await {
                            Ok(output) => Ok(output),
                            Err(e) => Err(Box::new(std::io::Error::other(e)) as Box<dyn std::error::Error + Send + Sync>),
                        };

                        match call_res {
                            Ok(out) => {
                                observation = out.as_string();
                                success = true;
                            }
                            Err(e) => {
                                // Check if it's a safety/allowlist error
                                let err_str: String = e.to_string();
                                if err_str.contains("allowlist") || err_str.contains("dangerous") {
                                    // STOP on safety/allowlist failure as requested
                                    let _ = event_tx.send(TuiEvent::AgentResponseFinal(format!("⛔ Terminal command blocked: {}", err_str), TokenUsage::default()));
                                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::WaitingForUser));
                                    let _ = event_tx.send(TuiEvent::ActivityUpdate {
                                        summary: "Action Blocked".to_string(),
                                        detail: Some(err_str),
                                    });
                                    return; // TERMINATE the loop
                                }
                                
                                // Check if it's a "tool not found" error - also stop the loop
                                if err_str.contains("not found in registry") || err_str.contains("not available") {
                                    let _ = event_tx.send(TuiEvent::AgentResponseFinal(format!("❌ Tool Error: {}", err_str), TokenUsage::default()));
                                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::WaitingForUser));
                                    let _ = event_tx.send(TuiEvent::ActivityUpdate {
                                        summary: "Tool Not Found".to_string(),
                                        detail: Some(err_str.clone()),
                                    });
                                    return; // TERMINATE the loop - no point retrying a missing tool
                                }
                                
                                observation = format!("Error: {}", e);
                                retry_count += 1;
                                
                                if retry_count == 1 {
                                    // TODO: Implement hidden LLM inquiry for "parser watcher" / "fix failed"
                                    // For now, we report the error and let it try again or fail.
                                }
                            }
                        }
                    }

                    // Show details (verbose mode will render detail, non-verbose just summary)
                    let detail = if observation.len() > 1200 {
                        Some(format!("{}… [truncated]", &observation[..1200]))
                    } else {
                        Some(observation.clone())
                    };
                    if tool == "web_search" {
                        let _ = event_tx.send(TuiEvent::ActivityUpdate {
                            summary: "Web search results".to_string(),
                            detail,
                        });
                    } else if tool == "crawl" {
                        let _ = event_tx.send(TuiEvent::ActivityUpdate {
                            summary: "Crawl results".to_string(),
                            detail,
                        });
                    }
                    
                    // Log to PTY - SUPPRESSED for Internal Tools
                    // Only ShellTool outputs (via ToolKind::Terminal path above) should appear in the Terminal Pane.
                    // Internal tool outputs are for the Agent's eyes only (via last_observation).
                    /*
                    let trimmed_obs = observation.trim();
                    let log_content = if trimmed_obs.len() > 200 {
                        format!("{}... [Content hidden, total length: {} chars]", &trimmed_obs[..100], trimmed_obs.len())
                    } else {
                        trimmed_obs.to_string()
                    };
                    let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", log_content);
                    let _ = event_tx.send(TuiEvent::InternalObservation(obs_log.into_bytes()));
                    */
                    
                    last_observation = Some(observation);
                    drop(agent_lock);
                }
            }
            Ok(AgentDecision::Error(e)) => {
                mylm_core::error_log!("Agent Decision Error: {}", e);
                let _ = event_tx.send(TuiEvent::AgentResponseFinal(format!("❌ Agent Error: {}", e), TokenUsage::default()));
                let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Error(e)));
                break;
            }
            Err(e) => {
                mylm_core::error_log!("Agent Loop Error: {}", e);
                let _ = event_tx.send(TuiEvent::AgentResponseFinal(format!("❌ System Error: {}", e), TokenUsage::default()));
                let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Error(e.to_string())));
                break;
            }
        }
    }
}

async fn run_pacore_task(
    agent: Arc<Mutex<Agent>>,
    history: Vec<ChatMessage>,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    interrupt_flag: Arc<AtomicBool>,
    pacore_rounds: &str,
    config: mylm_core::config::Config,
) {
    // Parse rounds configuration
    let rounds_vec: Vec<usize> = pacore_rounds
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    
    if rounds_vec.is_empty() {
        let _ = event_tx.send(TuiEvent::AgentResponseFinal(
            "Error: No valid rounds configured. Use /pacore rounds <comma-separated numbers>".to_string(),
            TokenUsage::default()
        ));
        let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
        return;
    }
    
    // Get LLM client and config from agent
    let (llm_client, resolved_config) = {
        let agent_lock = agent.lock().await;
        (agent_lock.llm_client.clone(), config.resolve_profile())
    };
    
    // Extract endpoint info
    let base_url = resolved_config.base_url.unwrap_or_else(|| resolved_config.provider.default_url());
    let api_key = match resolved_config.api_key {
        Some(key) => key,
        None => {
            let _ = event_tx.send(TuiEvent::AgentResponseFinal(
                "Error: No API key configured for PaCoRe".to_string(),
                TokenUsage::default()
            ));
            let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
            return;
        }
    };
    
    // Create ChatClient for PaCoRe
    let chat_client = mylm_core::pacore::ChatClient::new(base_url, api_key);
    
    // Get model name
    let model_name = llm_client.config().model.clone();
    
    // Clone rounds for status message before moving into Exp
    let rounds_display = rounds_vec.clone();
    let total_calls: usize = rounds_vec.iter().sum();
    
    // Create progress channel
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel(100);
    let event_tx_clone = event_tx.clone();
    
    // Spawn progress handler
    let num_rounds = rounds_vec.len();
    let rounds_for_progress = rounds_vec.clone();
    tokio::spawn(async move {
        let mut completed_calls = 0usize;
        while let Some(event) = progress_rx.recv().await {
            use mylm_core::pacore::PaCoReProgressEvent;
            let status = match &event {
                PaCoReProgressEvent::RoundStarted { round, total_rounds, calls_in_round } => {
                    format!("PaCoRe Round {}/{} • {} calls starting...", 
                        round + 1, total_rounds, calls_in_round)
                }
                PaCoReProgressEvent::CallCompleted { round, call_index: _, total_calls } => {
                    completed_calls += 1;
                    // Calculate completed calls in current round
                    let prev_rounds_total: usize = (0..*round).map(|r| rounds_for_progress.get(r).copied().unwrap_or(0)).sum();
                    let current_round_completed = completed_calls.saturating_sub(prev_rounds_total);
                    format!("PaCoRe R{} • {}/{} ✓ [{}/{} total]", 
                        round + 1, current_round_completed, total_calls, completed_calls, total_calls)
                }
                PaCoReProgressEvent::SynthesisStarted { round } => {
                    format!("PaCoRe synthesizing round {}...", round + 1)
                }
                PaCoReProgressEvent::StreamingStarted => {
                    "PaCoRe streaming final response...".to_string()
                }
                PaCoReProgressEvent::RoundCompleted { round, responses_received } => {
                    format!("PaCoRe Round {} completed ({} responses)", round + 1, responses_received)
                }
                PaCoReProgressEvent::Error { round, error } => {
                    format!("PaCoRe error in round {}: {}", round + 1, error)
                }
                _ => continue,
            };
            
            // Send status update
            let _ = event_tx_clone.send(TuiEvent::StatusUpdate(status));
            
            // Send progress update for progress bar
            if let PaCoReProgressEvent::CallCompleted { round, .. } = event {
                let _ = event_tx_clone.send(TuiEvent::PaCoReProgress {
                    completed: completed_calls,
                    total: total_calls,
                    current_round: round + 1,
                    total_rounds: num_rounds,
                });
            }
        }
    });
    
    // Create PaCoRe engine with progress callback
    let exp = Exp::new(
        model_name,
        rounds_vec,
        10, // max_concurrent - using default from CLI
        chat_client,
    ).with_progress_callback(move |e| {
        let _ = progress_tx.try_send(e);
    });
    
    // Run PaCoRe reasoning
    let _ = event_tx.send(TuiEvent::StatusUpdate(
        format!("PaCoRe reasoning with rounds: {:?} ({} total calls)", rounds_display, total_calls)
    ));
    
    // Convert ChatMessage history to PaCoRe Message format
    let pacore_messages: Vec<mylm_core::pacore::model::Message> = history
        .iter()
        .map(|msg| mylm_core::pacore::model::Message {
            role: match msg.role {
                mylm_core::llm::chat::MessageRole::User => "user",
                mylm_core::llm::chat::MessageRole::Assistant => "assistant",
                mylm_core::llm::chat::MessageRole::System => "system",
                mylm_core::llm::chat::MessageRole::Tool => "tool",
            }.to_string(),
            content: msg.content.clone(),
            name: None,
            tool_calls: None,
        })
        .collect();
    
    match exp.process_single_stream(pacore_messages, "tui").await {
        Ok(mut stream) => {
            // Accumulate full response for smooth rendering via PendingStream
            let mut full_response = String::new();
            
            // Consume the stream and accumulate content
            while let Some(chunk_result) = stream.next().await {
                // Check for interrupt
                if interrupt_flag.load(Ordering::SeqCst) {
                    let _ = event_tx.send(TuiEvent::StatusUpdate("PaCoRe interrupted by user".to_string()));
                    break;
                }
                
                match chunk_result {
                    Ok(chunk) => {
                        if let Some(choice) = chunk.choices.first() {
                            if let Some(delta) = &choice.delta {
                                // Accumulate content instead of sending per-chunk
                                full_response.push_str(&delta.content);
                            } else if let Some(message) = &choice.message {
                                full_response.push_str(&message.content);
                            }
                        }
                    }
                    Err(e) => {
                        let _ = event_tx.send(TuiEvent::StatusUpdate(format!("PaCoRe stream error: {}", e)));
                    }
                }
            }
            
            // Send accumulated response as a single final event for smooth rendering
            if !full_response.is_empty() {
                let _ = event_tx.send(TuiEvent::AgentResponseFinal(full_response, TokenUsage::default()));
            }
        }
        Err(e) => {
            let _ = event_tx.send(TuiEvent::AgentResponseFinal(
                format!("PaCoRe error: {}", e),
                TokenUsage::default()
            ));
        }
    }
    
    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
}
