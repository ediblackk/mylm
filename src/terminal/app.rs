use crate::terminal::pty::PtyManager;
use crate::executor::{CommandExecutor, allowlist::CommandAllowlist, safety::SafetyChecker};
use crate::llm::chat::ChatMessage;
use crate::llm::TokenUsage;
use crate::agent::{Agent, AgentDecision, ToolKind};
use crate::terminal::session::SessionMonitor;
use vt100::Parser;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

use tokio::sync::mpsc;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AppState {
    Idle,
    Thinking(String),      // Provider info
    #[allow(dead_code)]
    Streaming(String),     // Progress or partial content
    ExecutingTool(String), // Tool name
    WaitingForUser,        // Auto-approve off
    Error(String),
}

#[derive(Debug)]
pub enum TuiEvent {
    Input(crossterm::event::Event),
    Pty(Vec<u8>),
    PtyWrite(Vec<u8>),
    InternalObservation(Vec<u8>),
    AgentResponse(String, TokenUsage),
    StatusUpdate(String),
    CondensedHistory(Vec<ChatMessage>),
    ConfigUpdate(crate::config::Config),
    SuggestCommand(String),
    ExecuteTerminalCommand(String, tokio::sync::oneshot::Sender<String>),
    GetTerminalScreen(tokio::sync::oneshot::Sender<String>),
    AppStateUpdate(AppState),
    Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Terminal,
    Chat,
}

pub struct App {
    pub terminal_parser: Parser,
    pub pty_manager: PtyManager,
    pub config: crate::config::Config,
    pub agent: Arc<Mutex<Agent>>,
    pub chat_input: String,
    pub cursor_position: usize,
    pub chat_history: Vec<ChatMessage>,
    pub focus: Focus,
    pub state: AppState,
    pub should_quit: bool,
    pub chat_scroll: usize,
    pub chat_auto_scroll: bool,
    pub input_scroll: usize,
    pub session_monitor: SessionMonitor,
    pub terminal_scroll: usize,
    pub terminal_auto_scroll: bool,
    pub terminal_size: (u16, u16),
    pub status_message: Option<String>,
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
}

impl App {
    pub fn new(pty_manager: PtyManager, agent: Agent, config: crate::config::Config) -> Self {
        let max_ctx = agent.llm_client.config().max_context_tokens;
        let input_price = agent.llm_client.config().input_price_per_1k;
        let output_price = agent.llm_client.config().output_price_per_1k;
        
        let mut session_monitor = SessionMonitor::new();
        session_monitor.set_max_context(max_ctx as u32);
        let verbose_mode = config.verbose_mode;
        let auto_approve = Arc::new(AtomicBool::new(config.commands.allow_execution));

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
            chat_scroll: 0,
            chat_auto_scroll: true,
            input_scroll: 0,
            session_monitor,
            terminal_scroll: 0,
            terminal_auto_scroll: true,
            terminal_size: (24, 80),
            status_message: None,
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
        let mut chars: Vec<char> = self.chat_input.chars().collect();
        chars.insert(self.cursor_position, new_char);
        self.chat_input = chars.into_iter().collect();
        self.move_cursor_right();
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

    pub fn submit_message(&mut self, event_tx: mpsc::UnboundedSender<TuiEvent>) {
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

            // Capture full terminal history for context
            let history_height = 2000;
            let width = self.terminal_size.1;
            let mut temp_parser = Parser::new(history_height, width, 0);
            temp_parser.process(&self.raw_buffer);
            let terminal_content = temp_parser.screen().contents();
            
            // Limit the content size if necessary (though 2000 lines is usually safe)
            // The prompt format is strict:
            let final_message = format!("{}\n\n---\n[TERMINAL STATE ATTACHMENT]\n{}", input, terminal_content);

            self.chat_history.push(ChatMessage::user(&final_message));
            self.chat_input.clear();
            self.reset_cursor();
            self.input_scroll = 0;
            self.state = AppState::Thinking("...".to_string());
            // Reset scroll to bottom on new message
            self.chat_scroll = 0;
            self.chat_auto_scroll = true;

            let agent = self.agent.clone();
            let history = self.chat_history.clone();
            let monitor_ratio = self.session_monitor.get_context_ratio();
            let event_tx_clone = event_tx.clone();
            let interrupt_flag = self.interrupt_flag.clone();
            let auto_approve = self.auto_approve.clone();
            let max_driver_loops = self.config.agent.max_driver_loops;
            interrupt_flag.store(false, Ordering::SeqCst);

            let task = tokio::spawn(async move {
                {
                    let mut agent_lock = agent.lock().await;
                    
                    // Automatic condensation check
                    let final_history = if monitor_ratio > agent_lock.llm_client.config().condense_threshold {
                        match agent_lock.condense_history(&history).await {
                            Ok(new_history) => new_history,
                            Err(_) => history,
                        }
                    } else {
                        history
                    };

                    agent_lock.reset(final_history);
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

    pub fn abort_current_task(&mut self) {
        if let Some(task) = self.active_task.take() {
            if !task.is_finished() {
                task.abort();
                self.status_message = Some("⛔ Task interrupted by user.".to_string());
                self.interrupt_flag.store(true, Ordering::SeqCst);
            }
            self.state = AppState::Idle;
        }
    }

    fn handle_slash_command(&mut self, input: &str, event_tx: mpsc::UnboundedSender<TuiEvent>) {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts[0];

        match cmd {
            "/profile" => {
                if parts.len() < 2 {
                    self.chat_history.push(ChatMessage::assistant("Usage: /profile <name>\nAvailable profiles: ".to_string() +
                        &self.config.profiles.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")));
                    return;
                }
                let name = parts[1];
                if self.config.profiles.iter().any(|p| p.name == name) {
                    self.config.active_profile = name.to_string();
                    let _ = event_tx.send(TuiEvent::ConfigUpdate(self.config.clone()));
                    self.chat_history.push(ChatMessage::assistant(format!("Switched to profile: {}", name)));
                } else {
                    self.chat_history.push(ChatMessage::assistant(format!("Profile '{}' not found", name)));
                }
            }
            "/config" => {
                if parts.len() < 3 {
                    self.chat_history.push(ChatMessage::assistant("Usage: /config <key> <value>\nKeys: model, endpoint, prompt".to_string()));
                    return;
                }
                let key = parts[1];
                let value = parts[2];
                
                let mut updated = false;
                let active_profile_name = self.config.active_profile.clone();
                if let Some(profile) = self.config.profiles.iter_mut().find(|p| p.name == active_profile_name) {
                    match key {
                        "model" => {
                            self.chat_history.push(ChatMessage::assistant("Model editing via /config is pending better endpoint management. Use /profile to switch.".to_string()));
                        }
                        "endpoint" => {
                            profile.endpoint = value.to_string();
                            updated = true;
                        }
                        "prompt" => {
                            profile.prompt = value.to_string();
                            updated = true;
                        }
                        _ => {
                            self.chat_history.push(ChatMessage::assistant(format!("Unknown config key: {}", key)));
                        }
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
                let max_driver_loops = self.config.agent.max_driver_loops;
                let task = tokio::spawn(async move {
                    // Manual execution via /exec
                    // Safety check
                    let executor = CommandExecutor::new(CommandAllowlist::new(), SafetyChecker::new());
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
                self.chat_history.push(ChatMessage::assistant("Available commands:\n/profile <name> - Switch profile\n/config <key> <value> - Update active profile\n/exec <command> - Execute shell command\n/verbose - Toggle verbose mode\n/help - Show this help".to_string()));
            }
            "/verbose" => {
                self.verbose_mode = !self.verbose_mode;
                let status = if self.verbose_mode { "ON" } else { "OFF" };
                self.chat_history.push(ChatMessage::assistant(format!("Verbose mode: {}", status)));
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

    pub fn save_session(&self) -> Result<(), Box<dyn std::error::Error>> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not find data directory")?
            .join("mylm")
            .join("sessions");
        
        std::fs::create_dir_all(&data_dir)?;
        let path = data_dir.join("latest.json");
        let content = serde_json::to_string(&self.chat_history)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn load_session() -> Result<Vec<ChatMessage>, Box<dyn std::error::Error>> {
        let path = dirs::data_dir()
            .ok_or("Could not find data directory")?
            .join("mylm")
            .join("sessions")
            .join("latest.json");
        
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(path)?;
        let history: Vec<ChatMessage> = serde_json::from_str(&content)?;
        Ok(history)
    }

    pub fn handle_terminal_input(&mut self, bytes: &[u8]) {
        let _ = self.pty_manager.write_all(bytes);
        // Reset terminal scroll on input if auto-scroll is enabled or just to be helpful
        self.terminal_scroll = 0;
        self.terminal_auto_scroll = true;
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

        let mut agent_lock = agent.lock().await;
        let provider = agent_lock.llm_client.config().provider.to_string();
        let model = agent_lock.llm_client.config().model.clone();
        let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Thinking(format!("{} ({})", model, provider))));
        
        let step_res = agent_lock.step(last_observation.take()).await;
        match step_res {
            Ok(AgentDecision::Message(msg, usage)) => {
                let has_pending = agent_lock.has_pending_decision();
                let _ = event_tx.send(TuiEvent::AgentResponse(msg, usage));
                if has_pending {
                    drop(agent_lock);
                    continue;
                }
                let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Idle));
                break;
            }
            Ok(AgentDecision::Action { tool, args, kind }) => {
                let _ = event_tx.send(TuiEvent::StatusUpdate(format!("Tool: '{}'", tool)));
                
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

                    // Safety check
                    let executor = CommandExecutor::new(CommandAllowlist::new(), SafetyChecker::new());
                    if let Err(e) = executor.check_safety(&cmd) {
                        last_observation = Some(format!("Error: Safety Check Failed: {}", e));
                        let err_log = format!("\r\n\x1b[31m[Safety Check Failed]:\x1b[0m {}\r\n", e);
                        let _ = event_tx.send(TuiEvent::PtyWrite(err_log.into_bytes()));
                        drop(agent_lock);
                        continue;
                    }

                    if !auto_approve_flag.load(Ordering::SeqCst) {
                        let _ = event_tx.send(TuiEvent::SuggestCommand(cmd));
                        let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::WaitingForUser));
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
                                    if agent_lock.llm_client.config().memory.auto_categorize {
                                        let content = format!("Command: {}\nOutput: {}", cmd, output);
                                        let _ = agent_lock.auto_categorize(memory_id, &content).await;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            last_observation = Some("Error: Terminal command execution failed (channel closed)".to_string());
                        }
                    }
                } else {
                    // Internal tool
                    let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::ExecutingTool(tool.clone())));
                    let observation = match agent_lock.tools.get(&tool) {
                        Some(t) => match t.call(&args).await {
                            Ok(output) => output,
                            Err(e) => format!("Error: {}", e),
                        },
                        None => format!("Error: Tool '{}' not found.", tool),
                    };
                    
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
                let _ = event_tx.send(TuiEvent::AgentResponse(format!("Error: {}", e), TokenUsage::default()));
                let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Error(e)));
                break;
            }
            Err(e) => {
                let _ = event_tx.send(TuiEvent::AgentResponse(format!("Error: {}", e), TokenUsage::default()));
                let _ = event_tx.send(TuiEvent::AppStateUpdate(AppState::Error(e.to_string())));
                break;
            }
        }
    }
}
