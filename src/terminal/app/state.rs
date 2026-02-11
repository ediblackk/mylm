//! Application state management
//!
//! This module provides the main `AppStateContainer`.
//! Phase 4 state split preparation: types defined here, split into
//! state_ui.rs and state_agent.rs in future iteration.

use crate::terminal::pty::PtyManager;
use crate::terminal::session::SessionMonitor;
use crate::terminal::session_manager::SessionManager;
use crate::terminal::delegate_impl::TerminalDelegate;
use mylm_core::agent::{Agent, AgentOrchestrator, ChatSessionHandle};
use mylm_core::agent::v2::jobs::JobRegistry;
use mylm_core::agent::{AgentWrapper, EventBus};
use mylm_core::context::ContextManager;
use mylm_core::llm::chat::ChatMessage;
use mylm_core::llm::TokenUsage;
use mylm_core::memory::graph::MemoryGraph;
use mylm_core::agent::tools::StructuredScratchpad;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock; // For scratchpad (async-compatible)
use std::time::Instant;
use ratatui::layout::Rect;

// Re-export from core for convenience
pub use mylm_core::terminal::app::AppState;

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
    Jobs,
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
    pub chat_input_area: Option<Rect>,

    // UI state
    pub state: AppState,
    pub state_started_at: Instant,
    pub status_message: Option<String>,
    pub should_quit: bool,
    pub return_to_hub: bool,
    pub show_memory_view: bool,
    pub memory_graph: MemoryGraph,
    pub memory_graph_scroll: usize,
    pub show_help_view: bool,
    pub help_scroll: usize,
    pub update_available: bool,
    pub exit_name_input: String,
    pub chat_width_percent: u16,
    pub show_terminal: bool,
    pub tick_count: u64,

    // Job panel state
    pub show_jobs_panel: bool,
    pub selected_job_index: Option<usize>,
    pub job_registry: JobRegistry,
    pub show_job_detail: bool,
    pub job_scroll: usize,

    // Mouse selection state
    pub selection_start: Option<(u16, u16)>,
    pub selection_end: Option<(u16, u16)>,
    pub selection_pane: Option<Focus>,
    pub is_selecting: bool,
    pub terminal_area_offset: Option<(u16, u16)>,
    pub chat_area_offset: Option<(u16, u16)>,

    // Agent and session state
    pub agent: AgentWrapper,  // Already contains Arc<Mutex<>> internally
    pub config: mylm_core::config::Config,
    pub session_monitor: SessionMonitor,
    pub session_id: String,
    pub session_manager: SessionManager,
    pub context_manager: ContextManager,

    // Execution state
    pub active_task: Option<tokio::task::JoinHandle<()>>,
    pub capturing_command_output: bool,
    pub command_output_buffer: String,
    pub pending_command_tx: Option<tokio::sync::oneshot::Sender<String>>,
    pub pending_approval_tx: Option<tokio::sync::oneshot::Sender<bool>>,
    pub pending_approval_rx: Option<tokio::sync::oneshot::Receiver<bool>>,
    pub pending_stream: Option<PendingStream>,
    pub pending_echo_suppression: String,
    pub pending_clean_command: Option<String>,
    pub activity_log: Vec<ActivityEntry>,

    // Flags and settings
    pub interrupt_flag: Arc<AtomicBool>,
    pub verbose_mode: bool,
    pub show_thoughts: bool,
    pub auto_approve: Arc<AtomicBool>,
    pub incognito: bool,

    // Pricing info
    pub input_price: f64,
    pub output_price: f64,

    // PaCoRe state
    pub pacore_enabled: bool,
    pub pacore_rounds: String,
    pub pacore_progress: Option<(usize, usize)>,
    pub pacore_current_round: Option<(usize, usize)>,

    // Utilities
    pub clipboard: Option<arboard::Clipboard>,
    pub scratchpad: Arc<RwLock<StructuredScratchpad>>,
    pub last_total_chat_lines: Option<usize>,
    
    // Terminal snapshot deduplication
    pub last_terminal_snapshot: Option<String>,
    
    /// Phase 4: Agent orchestrator for centralized execution
    pub orchestrator: Option<AgentOrchestrator>,
    
    /// Phase 4: Chat session handle for submitting messages
    pub chat_session_handle: Option<ChatSessionHandle>,
    
    /// Phase 4: Terminal delegate for core tools
    pub terminal_delegate: Option<Arc<TerminalDelegate>>,
    
    /// Phase 4: Event bus for core events
    pub event_bus: Option<Arc<EventBus>>,
}

impl AppStateContainer {
    /// Legacy constructor - use `new_with_orchestrator` instead
    #[allow(dead_code)]
    pub fn new(
        pty_manager: PtyManager,
        agent: Agent,
        config: mylm_core::config::Config,
        scratchpad: Arc<RwLock<StructuredScratchpad>>,
        job_registry: JobRegistry,
        incognito: bool,
    ) -> Self {
        // Get actual config values from the LLM configuration
        let max_ctx = config.endpoint.max_context_tokens.unwrap_or(128000);
        let input_price = config.endpoint.input_price.unwrap_or(0.0);
        let output_price = config.endpoint.output_price.unwrap_or(0.0);

        let mut session_monitor = SessionMonitor::new(max_ctx as u32);
        session_monitor.set_pricing(input_price, output_price);
        let verbose_mode = false;
        let auto_approve = Arc::new(AtomicBool::new(false));
        let clipboard = arboard::Clipboard::new().ok();

        let session_id = agent.session_id.clone();
        let pacore_enabled = config.features.pacore.enabled;
        let pacore_rounds = config.features.pacore.rounds.clone();

        // Create context manager with actual config values and pricing
        let ctx_config = mylm_core::context::ContextConfig::new(max_ctx)
            .with_pricing(input_price, output_price);
        let context_manager = ContextManager::new(ctx_config);

        let app = Self {
            terminal_parser: vt100::Parser::new(24, 80, 0),
            pty_manager,
            config,
            agent: AgentWrapper::new_v1(agent),
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
            pending_stream: None,
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
            orchestrator: None,
            chat_session_handle: None,
            terminal_delegate: None,
            event_bus: None,
        };
        
        app
    }
    
    /// Create a new AppStateContainer with orchestrator (Phase 4 integrated)
    pub async fn new_with_orchestrator(
        pty_manager: PtyManager,
        agent: AgentWrapper,
        config: mylm_core::config::Config,
        scratchpad: Arc<RwLock<StructuredScratchpad>>,
        job_registry: JobRegistry,
        incognito: bool,
        orchestrator: AgentOrchestrator,
        terminal_delegate: Arc<TerminalDelegate>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        // Get actual config values from the LLM configuration
        let max_ctx = config.endpoint.max_context_tokens.unwrap_or(128000);
        let input_price = config.endpoint.input_price.unwrap_or(0.0);
        let output_price = config.endpoint.output_price.unwrap_or(0.0);

        let mut session_monitor = SessionMonitor::new(max_ctx as u32);
        session_monitor.set_pricing(input_price, output_price);
        let verbose_mode = false;
        let auto_approve = Arc::new(AtomicBool::new(false));
        let clipboard = arboard::Clipboard::new().ok();

        // Get session_id from the agent (using wrapper method)
        let session_id = agent.session_id().await;
        let pacore_enabled = config.features.pacore.enabled;
        let pacore_rounds = config.features.pacore.rounds.clone();

        // Create context manager with actual config values and pricing
        let ctx_config = mylm_core::context::ContextConfig::new(max_ctx)
            .with_pricing(input_price, output_price);
        let context_manager = ContextManager::new(ctx_config);

        let mut app = Self {
            terminal_parser: vt100::Parser::new(24, 80, 0),
            pty_manager,
            config,
            agent,
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
            pending_stream: None,
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
            // Phase 4 fields - now populated
            orchestrator: None, // Will be set after setting terminal delegate
            chat_session_handle: None,
            terminal_delegate: Some(terminal_delegate.clone()),
            event_bus: Some(event_bus),
        };
        
        // Set terminal delegate and auto_approve on orchestrator before storing
        let mut orchestrator = orchestrator;
        orchestrator.set_terminal_delegate(terminal_delegate);
        orchestrator.set_auto_approve(app.auto_approve.load(Ordering::SeqCst));
        app.orchestrator = Some(orchestrator);
        
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

    pub fn process_terminal_data(&mut self, data: &[u8]) {
        // Reduced logging to avoid I/O overhead
        // mylm_core::info_log!("process_terminal_data: processing {} bytes", data.len());
        self.terminal_parser.process(data);
        self.raw_buffer.extend_from_slice(data);
    }

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
    
    /// Initialize the orchestrator (Phase 4 - preparatory)
    #[allow(dead_code)]
    pub fn init_orchestrator(&mut self, mut orchestrator: AgentOrchestrator) {
        orchestrator.set_auto_approve(self.auto_approve.load(Ordering::SeqCst));
        self.orchestrator = Some(orchestrator);
    }
    
    /// Initialize the terminal delegate (Phase 4 - preparatory)
    #[allow(dead_code)]
    pub fn init_terminal_delegate(&mut self, delegate: Arc<TerminalDelegate>) {
        self.terminal_delegate = Some(delegate);
    }
    
    /// Initialize the event bus (Phase 4 - preparatory)
    #[allow(dead_code)]
    pub fn init_event_bus(&mut self, event_bus: Arc<EventBus>) {
        self.event_bus = Some(event_bus);
    }
}
