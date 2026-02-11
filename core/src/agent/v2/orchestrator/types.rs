//! Orchestrator Types
//!
//! Core types and configuration for the agent orchestrator.

use crate::agent::event_bus::EventBus;
use crate::agent::traits::TerminalExecutor;
use crate::agent::v2::jobs::JobRegistry;
use crate::agent::{Agent, AgentV2};
use crate::llm::chat::ChatMessage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;
use std::collections::VecDeque;

/// Unique identifier for a task
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskId(String);

impl TaskId {
    /// Create a new random task ID
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
    
    /// Get the string representation
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle to track and control a running task
#[derive(Debug, Clone)]
pub struct TaskHandle {
    pub id: TaskId,
    pub interrupt_flag: Arc<AtomicBool>,
}

impl TaskHandle {
    /// Create a new task handle
    pub fn new() -> Self {
        Self {
            id: TaskId::new(),
            interrupt_flag: Arc::new(AtomicBool::new(false)),
        }
    }
    
    /// Request graceful interruption of the task
    pub fn interrupt(&self) {
        self.interrupt_flag.store(true, Ordering::SeqCst);
    }
    
    /// Check if interruption has been requested
    pub fn is_interrupted(&self) -> bool {
        self.interrupt_flag.load(Ordering::SeqCst)
    }
}

impl Default for TaskHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks the execution state of the orchestrator
#[derive(Debug, Clone)]
pub enum ExecutionState {
    /// Idle, waiting for tasks
    Idle,
    /// Agent is thinking/processing
    Thinking { model: String, provider: String },
    /// Tool is being executed
    ExecutingTool { tool: String },
    /// Waiting for user approval
    WaitingForUser,
    /// Smart waiting for background workers
    SmartWaiting { active_workers: usize, iteration: usize },
    /// Task completed
    Completed,
    /// Task failed with error
    Error(String),
}

/// Message types for the chat session queue
#[derive(Debug, Clone)]
pub enum ChatSessionMessage {
    /// A user message to process
    UserMessage(ChatMessage),
    /// A worker event to process
    WorkerEvent(String),
    /// Request to interrupt the session
    Interrupt,
}

/// Handle for an active chat session - use this to submit messages
#[derive(Clone)]
pub struct ChatSessionHandle {
    pub(crate) sender: Sender<ChatSessionMessage>,
}

impl ChatSessionHandle {
    /// Create a new chat session handle (crate-internal)
    pub(crate) fn new(sender: Sender<ChatSessionMessage>) -> Self {
        Self { sender }
    }
    
    /// Submit a message to the chat session
    pub async fn send(&self, message: ChatSessionMessage) {
        crate::info_log!("ChatSessionHandle: Sending message to channel");
        match self.sender.send(message).await {
            Ok(_) => {
                crate::info_log!("ChatSessionHandle: Message sent successfully");
            }
            Err(e) => {
                crate::error_log!("ChatSessionHandle: Failed to send message: {:?}", e);
            }
        }
    }
}

/// Configuration for the AgentOrchestrator
#[derive(Clone)]
pub struct OrchestratorConfig {
    /// Maximum iterations for agent loops (safety limit)
    pub max_driver_loops: usize,
    /// Maximum retries for malformed actions
    pub max_retries: usize,
    /// Maximum smart wait iterations before returning to idle
    pub max_smart_wait_iterations: usize,
    /// Duration to wait between smart wait checks
    pub smart_wait_interval_secs: u64,
    /// Auto-approve terminal commands
    pub auto_approve: bool,
    /// Enable memory recording
    pub enable_memory: bool,
    /// Maximum tool execution failures before worker is stalled
    /// (configurable in Agent Settings)
    pub max_worker_tool_failures: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_driver_loops: 50,
            max_retries: 3,
            max_smart_wait_iterations: 5,
            smart_wait_interval_secs: 1,
            auto_approve: false,
            enable_memory: true,
            max_worker_tool_failures: 3, // Default: stall after 3 failures
        }
    }
}

/// The AgentOrchestrator centralizes all agent execution logic.
/// 
/// It manages:
/// - The main agent loop execution
/// - Background job lifecycle
/// - Tool execution coordination
/// - Event publishing to subscribers
/// - Worker escalation requests (for restricted command approval)
pub struct AgentOrchestrator {
    /// The V1 agent (legacy, for backward compatibility)
    pub agent_v1: Option<Arc<Mutex<Agent>>>,
    /// The V2 agent (preferred)
    pub agent_v2: Option<Arc<Mutex<AgentV2>>>,
    /// Event bus for publishing events to subscribers
    pub event_bus: Arc<EventBus>,
    /// Configuration for orchestration behavior
    pub config: OrchestratorConfig,
    /// Terminal executor delegate (for shell/terminal operations)
    pub terminal_delegate: Option<Arc<dyn TerminalExecutor>>,
    /// Job registry for background workers
    pub job_registry: JobRegistry,
    /// Task handles for active tasks
    pub active_tasks: Arc<Mutex<Vec<TaskHandle>>>,
    /// Message queue for chat session mode (user messages + worker events)
    pub chat_session_queue: Arc<Mutex<VecDeque<ChatSessionMessage>>>,
    /// Sender for submitting messages to chat session
    pub chat_session_sender: Option<Sender<ChatSessionMessage>>,
    /// Receiver for chat session to get messages
    pub chat_session_receiver: Arc<Mutex<Option<Receiver<ChatSessionMessage>>>>,
    /// Flag indicating if chat session is active
    pub chat_session_active: Arc<AtomicBool>,
    /// Sender for worker escalation requests (worker → main agent)
    pub escalation_tx: Option<Sender<(crate::agent::tools::worker_shell::EscalationRequest, tokio::sync::oneshot::Sender<crate::agent::tools::worker_shell::EscalationResponse>)>>,
    /// Receiver for worker escalation requests
    pub escalation_rx: Arc<Mutex<Option<Receiver<(crate::agent::tools::worker_shell::EscalationRequest, tokio::sync::oneshot::Sender<crate::agent::tools::worker_shell::EscalationResponse>)>>>>,
    /// Sender for tool approval responses (UI → orchestrator)
    pub approval_tx: Arc<Mutex<Option<Sender<bool>>>>,
}

impl Clone for AgentOrchestrator {
    fn clone(&self) -> Self {
        Self {
            agent_v1: self.agent_v1.clone(),
            agent_v2: self.agent_v2.clone(),
            event_bus: self.event_bus.clone(),
            config: self.config.clone(),
            terminal_delegate: self.terminal_delegate.clone(),
            job_registry: self.job_registry.clone(),
            active_tasks: self.active_tasks.clone(),
            chat_session_queue: self.chat_session_queue.clone(),
            chat_session_sender: None, // Can't clone sender - new session needs new channel
            chat_session_receiver: Arc::new(Mutex::new(None)),
            chat_session_active: self.chat_session_active.clone(),
            escalation_tx: self.escalation_tx.clone(),
            escalation_rx: Arc::new(Mutex::new(None)), // Can't clone receiver
            approval_tx: self.approval_tx.clone(),
        }
    }
}
