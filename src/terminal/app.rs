use crate::terminal::pty::PtyManager;
use crate::llm::chat::ChatMessage;
use crate::llm::TokenUsage;
use crate::agent::Agent;
use crate::terminal::session::SessionMonitor;
use vt100::Parser;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

use tokio::sync::mpsc;

#[derive(Debug, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Processing,
}

#[derive(Debug)]
pub enum TuiEvent {
    Input(crossterm::event::Event),
    Pty(Vec<u8>),
    PtyWrite(Vec<u8>),
    AgentResponse(String, TokenUsage),
    StatusUpdate(String),
    CondensedHistory(Vec<ChatMessage>),
    ConfigUpdate(crate::config::Config),
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
    pub active_task: Option<tokio::task::JoinHandle<()>>,
}

impl App {
    pub fn new(pty_manager: PtyManager, agent: Agent, config: crate::config::Config) -> Self {
        let max_ctx = agent.llm_client.config().max_context_tokens;
        let mut session_monitor = SessionMonitor::new();
        session_monitor.set_max_context(max_ctx as u32);
        let verbose_mode = config.verbose_mode;

        Self {
            terminal_parser: Parser::new(24, 80, 1000), // 1000 lines of scrollback
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
            active_task: None,
        }
    }

    pub fn resize_pty(&mut self, width: u16, height: u16) {
        self.terminal_size = (height, width);
        let _ = self.pty_manager.resize(height, width);
        self.terminal_parser.set_size(height, width);
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
        if !self.chat_input.is_empty() && self.state == AppState::Idle {
            let input = self.chat_input.clone();

            // Handle Slash Commands
            if input.starts_with('/') {
                self.handle_slash_command(&input, event_tx);
                self.chat_input.clear();
                self.reset_cursor();
                return;
            }

            self.chat_history.push(ChatMessage::user(&input));
            self.chat_input.clear();
            self.reset_cursor();
            self.input_scroll = 0;
            self.state = AppState::Processing;
            // Reset scroll to bottom on new message
            self.chat_scroll = 0;
            self.chat_auto_scroll = true;

            let agent = self.agent.clone();
            let history = self.chat_history.clone();
            let monitor_ratio = self.session_monitor.get_context_ratio();
            let event_tx_clone = event_tx.clone();
            let interrupt_flag = self.interrupt_flag.clone();
            interrupt_flag.store(false, Ordering::SeqCst);

            let task = tokio::spawn(async move {
                let mut agent = agent.lock().await;
                
                // Automatic condensation check
                let final_history = if monitor_ratio > agent.llm_client.config().condense_threshold {
                    match agent.condense_history(&history).await {
                        Ok(new_history) => new_history,
                        Err(_) => history,
                    }
                } else {
                    history
                };

                let result = agent.run(final_history, event_tx_clone, interrupt_flag).await;
                match result {
                    Ok((response, usage)) => {
                        let _ = event_tx.send(TuiEvent::AgentResponse(response, usage));
                    }
                    Err(e) => {
                        let _ = event_tx.send(TuiEvent::AgentResponse(format!("Error: {}", e), TokenUsage::default()));
                    }
                }
            });
            
            self.active_task = Some(task);
        }
    }

    pub fn abort_current_task(&mut self) {
        if let Some(task) = self.active_task.take() {
            task.abort();
            self.state = AppState::Idle;
            self.status_message = Some("⛔ Task interrupted by user.".to_string());
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
                            // Model editing via /config is tricky because endpoints are shared.
                            // For now, let's just not allow it here.
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
            "/help" => {
                self.chat_history.push(ChatMessage::assistant("Available commands:\n/profile <name> - Switch profile\n/config <key> <value> - Update active profile\n/help - Show this help".to_string()));
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
        // Extract only the Final Answer part if present, otherwise use the full content
        let clean_content = if let Some(pos) = content.find("Final Answer:") {
            content[pos + "Final Answer:".len()..].trim().to_string()
        } else {
            content
        };
        
        self.chat_history.push(ChatMessage::assistant(clean_content));
        
        // Get pricing from agent's LLM client config
        let (input_price, output_price) = {
            // We use a block to limit the borrow of agent
            let agent = futures::executor::block_on(self.agent.lock());
            let config = agent.llm_client.config();
            (config.input_price_per_1k, config.output_price_per_1k)
        };

        self.session_monitor.add_usage(&usage, input_price, output_price);
        self.state = AppState::Idle;
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
