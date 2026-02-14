//! Application state management
//!
//! This module provides the main `AppStateContainer`.
//! Phase 4 state split preparation: types defined here, split into
//! state_ui.rs and state_agent.rs in future iteration.

// Re-export types from the types module (authoritative source)
pub use crate::tui::types::{
    PtyManager, StructuredScratchpad, JobRegistry,
    StreamState, AppState, Focus,
};
use mylm_core::agent::contract::session::{OutputEvent, UserInput};
use mylm_core::agent::factory::AgentSessionFactory;
use mylm_core::context::ContextManager;
use mylm_core::llm::chat::ChatMessage;
use mylm_core::memory::graph::MemoryGraph;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Instant;
use ratatui::layout::Rect;
use tokio::sync::mpsc;

// Import real Session types from session module
use crate::tui::session::{Session, SessionMonitor};

// Stub types for items not yet in contract
#[derive(Debug, Clone)]
pub struct SessionManager;

impl SessionManager {
    pub fn new() -> Self { Self }
    pub fn set_current_session(&mut self, _session: Session) {}
}

#[derive(Debug, Clone)]
pub struct TerminalDelegate;

impl TerminalDelegate {
    #[allow(dead_code)]
    pub fn new() -> Self { Self }
}

#[derive(Debug, Clone)]
pub struct ActivityEntry {
    #[allow(dead_code)]
    pub at: Instant,
    #[allow(dead_code)]
    pub summary: String,
    #[allow(dead_code)]
    pub detail: Option<String>,
}

/// Core application state container
/// 
/// Note: Phase 4 state split is prepared but not fully enforced.
/// The orchestrator field has been added for new architecture.
pub struct AppStateContainer {
    // Terminal state
    pub terminal_parser: vt100::Parser,
    pub pty_manager: PtyManager,
    pub terminal_size: (u16, u16),
    pub terminal_scroll: usize,
    pub terminal_auto_scroll: bool,
    pub terminal_history: Vec<String>,
    pub raw_buffer: Vec<u8>,

    // Chat state
    pub chat_input: String,
    pub cursor_position: usize,
    pub chat_history: Vec<ChatMessage>,
    pub chat_scroll: usize,
    pub chat_auto_scroll: bool,
    pub input_scroll: usize,
    pub chat_visual_lines: Vec<(String, usize)>,
    pub chat_history_start_col: Option<u16>,
    pub chat_visible_start_idx: usize,
    pub chat_visible_end_idx: usize,
    pub focus: Focus,
    #[allow(dead_code)]
    pub chat_input_area: Option<Rect>,

    // UI state
    pub state: AppState,
    pub state_started_at: Instant,
    pub status_message: Option<String>,
    pub should_quit: bool,
    pub return_to_hub: bool,
    pub show_memory_view: bool,
    #[allow(dead_code)]
    pub memory_graph: MemoryGraph,
    #[allow(dead_code)]
    pub memory_graph_scroll: usize,
    #[allow(dead_code)]
    pub show_help_view: bool,
    #[allow(dead_code)]
    pub help_scroll: usize,
    #[allow(dead_code)]
    pub update_available: bool,
    #[allow(dead_code)]
    pub exit_name_input: String,
    pub chat_width_percent: u16,
    #[allow(dead_code)]
    pub show_terminal: bool,
    #[allow(dead_code)]
    pub tick_count: u64,

    // Job panel state
    #[allow(dead_code)]
    pub show_jobs_panel: bool,
    #[allow(dead_code)]
    pub selected_job_index: Option<usize>,
    pub job_registry: JobRegistry,
    #[allow(dead_code)]
    pub show_job_detail: bool,
    #[allow(dead_code)]
    pub job_scroll: usize,

    // Mouse selection state
    pub selection_start: Option<(u16, u16)>,
    pub selection_end: Option<(u16, u16)>,
    pub selection_pane: Option<Focus>,
    pub is_selecting: bool,
    pub terminal_area_offset: Option<(u16, u16)>,
    pub chat_area_offset: Option<(u16, u16)>,

    // Agent and session state
    // Using contract session for agent communication
    #[allow(dead_code)]
    pub agent_session_factory: Option<AgentSessionFactory>,
    pub config: mylm_core::config::Config,
    pub session_monitor: SessionMonitor,
    pub session_id: String,
    pub session_manager: SessionManager,
    pub context_manager: ContextManager,

    // Agent session channels (for streaming events)
    #[allow(dead_code)]
    pub output_rx: Option<mpsc::UnboundedReceiver<OutputEvent>>,
    #[allow(dead_code)]
    pub input_tx: Option<mpsc::Sender<UserInput>>,

    // Execution state
    #[allow(dead_code)]
    pub active_task: Option<tokio::task::JoinHandle<()>>,
    #[allow(dead_code)]
    pub capturing_command_output: bool,
    #[allow(dead_code)]
    pub command_output_buffer: String,
    #[allow(dead_code)]
    pub pending_command_tx: Option<tokio::sync::oneshot::Sender<String>>,
    #[allow(dead_code)]
    pub pending_approval_tx: Option<tokio::sync::oneshot::Sender<bool>>,
    #[allow(dead_code)]
    pub pending_approval_rx: Option<tokio::sync::oneshot::Receiver<bool>>,
    #[allow(dead_code)]
    pub stream_state: Option<StreamState>,
    
    // Streaming parser state fields
    #[allow(dead_code)]
    pub stream_escape_next: bool,
    #[allow(dead_code)]
    pub stream_key_buffer: String,
    #[allow(dead_code)]
    pub stream_lookback: String,
    #[allow(dead_code)]
    pub stream_thought: Option<String>,
    
    // Current response buffer for streaming
    #[allow(dead_code)]
    pub current_response: String,
    
    #[allow(dead_code)]
    pub pending_echo_suppression: String,
    #[allow(dead_code)]
    pub pending_clean_command: Option<String>,
    #[allow(dead_code)]
    pub activity_log: Vec<ActivityEntry>,

    // Flags and settings
    pub interrupt_flag: Arc<AtomicBool>,
    #[allow(dead_code)]
    pub verbose_mode: bool,
    #[allow(dead_code)]
    pub show_thoughts: bool,
    #[allow(dead_code)]
    pub auto_approve: Arc<AtomicBool>,
    pub incognito: bool,

    // Pricing info
    pub input_price: f64,
    pub output_price: f64,

    // PaCoRe state
    #[allow(dead_code)]
    pub pacore_enabled: bool,
    #[allow(dead_code)]
    pub pacore_rounds: usize,
    #[allow(dead_code)]
    pub pacore_progress: Option<(usize, usize)>,
    #[allow(dead_code)]
    pub pacore_current_round: Option<(usize, usize)>,

    // Utilities
    pub clipboard: Option<arboard::Clipboard>,
    #[allow(dead_code)]
    pub scratchpad: Arc<RwLock<StructuredScratchpad>>,
    #[allow(dead_code)]
    pub last_total_chat_lines: Option<usize>,
    
    // Terminal snapshot deduplication
    #[allow(dead_code)]
    pub last_terminal_snapshot: Option<String>,
    
    /// Phase 4: Agent session factory for creating sessions
    #[allow(dead_code)]
    pub session_factory: Option<AgentSessionFactory>,
    
    /// Phase 4: Chat session handle for submitting messages
    /// Using dyn trait object for session handle
    #[allow(dead_code)]
    pub chat_session_handle: Option<mpsc::Sender<UserInput>>,
    
    /// Phase 4: Terminal delegate for core tools
    #[allow(dead_code)]
    pub terminal_delegate: Option<Arc<TerminalDelegate>>,
    
    /// Pending approval for tool execution (intent_id, tool_name, args)
    pub pending_approval: Option<(u64, String, String)>,
    
    /// Flag to request session save
    pub save_session_request: bool,
    
    /// Streaming state - currently in final answer
    pub stream_in_final: bool,
    
    /// Session active flag - false when session has halted
    pub session_active: bool,
}

impl AppStateContainer {
    /// Legacy constructor - simplified for new config structure
    #[allow(dead_code)]
    pub fn new(
        pty_manager: PtyManager,
        config: mylm_core::config::Config,
        scratchpad: Arc<RwLock<StructuredScratchpad>>,
        job_registry: JobRegistry,
        incognito: bool,
    ) -> Self {
        // Get actual config values from the LLM configuration
        let profile = config.active_profile();
        let max_ctx = profile.context_window;
        let input_price = profile.input_price.unwrap_or(0.0);
        let output_price = profile.output_price.unwrap_or(0.0);

        let mut session_monitor = SessionMonitor::new(max_ctx as u32);
        session_monitor.set_pricing(input_price, output_price);
        let verbose_mode = false;
        let auto_approve = Arc::new(AtomicBool::new(false));
        let clipboard = arboard::Clipboard::new().ok();

        let session_id = String::new();
        let pacore_enabled = config.features.pacore.enabled;
        let pacore_rounds = config.features.pacore.rounds;

        // Create context manager with actual config values and pricing
        let ctx_config = mylm_core::context::ContextConfig::new(max_ctx)
            .with_pricing(input_price, output_price);
        let context_manager = ContextManager::new(ctx_config);

        let app = Self {
            terminal_parser: vt100::Parser::new(24, 80, 0),
            pty_manager,
            config,
            agent_session_factory: None,
            chat_input: String::new(),
            cursor_position: 0,
            chat_history: Vec::new(),
            chat_visual_lines: Vec::new(),
            chat_history_start_col: None,
            chat_visible_start_idx: 0,
            chat_visible_end_idx: 0,
            focus: Focus::Terminal,
            chat_input_area: None,
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
            stream_state: None,
            stream_escape_next: false,
            stream_key_buffer: String::new(),
            stream_lookback: String::new(),
            stream_thought: None,
            current_response: String::new(),
            pending_approval_tx: None,
            pending_approval_rx: None,
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
            show_jobs_panel: false,
            selected_job_index: None,
            job_registry,
            show_job_detail: false,
            job_scroll: 0,
            chat_width_percent: 30,
            show_terminal: true,
            selection_start: None,
            selection_end: None,
            selection_pane: None,
            is_selecting: false,
            terminal_area_offset: None,
            chat_area_offset: None,
            clipboard,
            scratchpad,
            pacore_enabled,
            pacore_rounds,
            pacore_progress: None,
            pacore_current_round: None,
            context_manager,
            session_manager: SessionManager::new(),
            incognito,
            last_terminal_snapshot: None,
            // Phase 4 fields
            session_factory: None,
            chat_session_handle: None,
            terminal_delegate: None,
            output_rx: None,
            input_tx: None,
            // Missing fields
            pending_approval: None,
            save_session_request: false,
            stream_in_final: false,
            session_active: true,
        };
        
        app
    }
    
    /// Create a new AppStateContainer with agent session factory
    #[allow(dead_code)]
    pub async fn new_with_factory(
        pty_manager: PtyManager,
        config: mylm_core::config::Config,
        scratchpad: Arc<RwLock<StructuredScratchpad>>,
        job_registry: JobRegistry,
        incognito: bool,
        session_factory: AgentSessionFactory,
    ) -> Self {
        // Get actual config values from the LLM configuration
        let profile = config.active_profile();
        let max_ctx = profile.context_window;
        let input_price = profile.input_price.unwrap_or(0.0);
        let output_price = profile.output_price.unwrap_or(0.0);

        let mut session_monitor = SessionMonitor::new(max_ctx as u32);
        session_monitor.set_pricing(input_price, output_price);
        let verbose_mode = false;
        let auto_approve = Arc::new(AtomicBool::new(false));
        let clipboard = arboard::Clipboard::new().ok();

        let session_id = String::new();
        let pacore_enabled = config.features.pacore.enabled;
        let pacore_rounds = config.features.pacore.rounds;

        // Create context manager with actual config values and pricing
        let ctx_config = mylm_core::context::ContextConfig::new(max_ctx)
            .with_pricing(input_price, output_price);
        let context_manager = ContextManager::new(ctx_config);

        let app = Self {
            terminal_parser: vt100::Parser::new(24, 80, 0),
            pty_manager,
            config,
            agent_session_factory: Some(session_factory),
            chat_input: String::new(),
            cursor_position: 0,
            chat_history: Vec::new(),
            chat_visual_lines: Vec::new(),
            chat_history_start_col: None,
            chat_visible_start_idx: 0,
            chat_visible_end_idx: 0,
            focus: Focus::Terminal,
            chat_input_area: None,
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
            stream_state: None,
            stream_escape_next: false,
            stream_key_buffer: String::new(),
            stream_lookback: String::new(),
            stream_thought: None,
            current_response: String::new(),
            pending_approval_tx: None,
            pending_approval_rx: None,
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
            show_jobs_panel: false,
            selected_job_index: None,
            job_registry,
            show_job_detail: false,
            job_scroll: 0,
            chat_width_percent: 30,
            show_terminal: true,
            selection_start: None,
            selection_end: None,
            selection_pane: None,
            is_selecting: false,
            terminal_area_offset: None,
            chat_area_offset: None,
            clipboard,
            scratchpad,
            pacore_enabled,
            pacore_rounds,
            pacore_progress: None,
            pacore_current_round: None,
            context_manager,
            session_manager: SessionManager::new(),
            incognito,
            last_terminal_snapshot: None,
            // Phase 4 fields - use agent_session_factory for both
            session_factory: None,
            chat_session_handle: None,
            terminal_delegate: None,
            output_rx: None,
            input_tx: None,
            // Missing fields
            pending_approval: None,
            save_session_request: false,
            stream_in_final: false,
            session_active: true,
        };
        
        app
    }

    pub fn set_state(&mut self, state: AppState) {
        let is_idle = matches!(state, AppState::Idle);
        self.state = state;
        self.state_started_at = Instant::now();
        if is_idle {
            self.pacore_progress = None;
            self.pacore_current_round = None;
        }
    }

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn process_terminal_data(&mut self, data: &[u8]) {
        // Reduced logging to avoid I/O overhead
        // mylm_core::info_log!("process_terminal_data: processing {} bytes", data.len());
        self.terminal_parser.process(data);
        self.raw_buffer.extend_from_slice(data);
    }

    #[allow(dead_code)]
    pub fn resize_pty(&mut self, width: u16, height: u16) {
        self.terminal_size = (height, width);
        let _ = self.pty_manager.resize(height, width);
        let mut new_parser = vt100::Parser::new(height, width, 0);
        new_parser.process(&self.raw_buffer);
        self.terminal_parser = new_parser;
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Terminal => Focus::Chat,
            Focus::Chat => {
                if self.show_jobs_panel {
                    Focus::Jobs
                } else {
                    Focus::Terminal
                }
            }
            Focus::Jobs => Focus::Terminal,
        };
    }

    pub fn abort_current_task(&mut self) {
        // Simplified: removed abort task log
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
            if self.capturing_command_output {
                let _ = self.pty_manager.write_all(&[3, 13]);
                let _ = self
                    .pty_manager
                    .write_all(b"([ -t 0 ] && stty echo) 2>/dev/null\r");
            }
        }
        self.capturing_command_output = false;
        self.state = AppState::Idle;
    }
    
    /// Initialize the terminal delegate (Phase 4 - preparatory)
    #[allow(dead_code)]
    pub fn init_terminal_delegate(&mut self, delegate: Arc<TerminalDelegate>) {
        self.terminal_delegate = Some(delegate);
    }
}
