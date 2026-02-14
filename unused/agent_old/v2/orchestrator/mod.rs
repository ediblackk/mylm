//! Agent Orchestrator - Centralized agent execution management
//!
//! This module provides the `AgentOrchestrator` which centralizes all agent execution logic,
//! moving it from the terminal UI layer into the core. This creates a clean separation where:
//! - Core manages: agent loops, tool execution, job spawning, reasoning strategies
//! - Terminal manages: UI state, user input, event display
//!
//! The orchestrator uses the EventBus for unidirectional communication (Core → Terminal)
//! and accepts a TerminalExecutor delegate for terminal-specific operations.

mod event_bus;
mod helpers;
mod loops;
mod types;

// PaCoRe experimental module
pub mod pacore;

pub use types::{
    AgentOrchestrator, ChatSessionHandle, ChatSessionMessage, ExecutionState, OrchestratorConfig,
    TaskHandle, TaskId,
};

// Export event bus components from local module
pub use event_bus::{CoreEvent, EventBus};

// Re-export escalation types from worker_shell
pub use crate::agent_old::tools::worker_shell::{EscalationRequest, EscalationResponse};

// Note: EventBus is exported above, no need for alias
use crate::agent_old::traits::TerminalExecutor;
use crate::agent_old::v2::jobs::JobRegistry;
use crate::agent_old::v2::AgentV2;
use crate::agent_old::Agent;
use crate::llm::chat::ChatMessage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{channel, Sender, Receiver};
use tokio::sync::oneshot;

/// Type alias for escalation channel
pub type EscalationChannel = (Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>, Receiver<(EscalationRequest, oneshot::Sender<EscalationResponse>)>);

impl AgentOrchestrator {
    /// Create escalation channel for worker restricted command approval
    pub fn create_escalation_channel() -> EscalationChannel {
        channel(100)
    }

    /// Create a new orchestrator with an Agent (V1)
    pub async fn new_with_agent_v1(
        agent: Arc<Mutex<Agent>>,
        event_bus: Arc<EventBus>,
        config: OrchestratorConfig,
    ) -> Self {
        Self::new_with_agent_v1_and_escalation(agent, event_bus, config, None, None).await
    }

    /// Create a new orchestrator with an Agent (V1) and optional external escalation channel
    pub async fn new_with_agent_v1_and_escalation(
        agent: Arc<Mutex<Agent>>,
        event_bus: Arc<EventBus>,
        config: OrchestratorConfig,
        external_escalation_tx: Option<Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
        external_escalation_rx: Option<Receiver<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
    ) -> Self {
        let job_registry = {
            let agent_lock = agent.lock().await;
            Arc::new(agent_lock.job_registry.clone())
        };
        
        // Use external channel if both sender and receiver are provided, otherwise create new
        let (escalation_tx, escalation_rx) = if let (Some(tx), Some(rx)) = (external_escalation_tx, external_escalation_rx) {
            (tx, rx)
        } else {
            Self::create_escalation_channel()
        };
        
        let orchestrator = Self {
            agent_v1: Some(agent),
            agent_v2: None,
            event_bus,
            config,
            terminal_delegate: None,
            job_registry,
            active_tasks: Arc::new(Mutex::new(Vec::new())),
            chat_session_queue: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            chat_session_sender: None,
            chat_session_receiver: Arc::new(Mutex::new(None)),
            chat_session_active: Arc::new(AtomicBool::new(false)),
            escalation_tx: Some(escalation_tx),
            escalation_rx: Arc::new(Mutex::new(Some(escalation_rx))),
            approval_tx: Arc::new(Mutex::new(None)),
        };
        
        // Spawn background tasks
        orchestrator.spawn_escalation_handler();
        
        orchestrator
    }
    
    /// Create a new orchestrator with an AgentV2
    pub async fn new_with_agent_v2(
        agent: Arc<Mutex<AgentV2>>,
        event_bus: Arc<EventBus>,
        config: OrchestratorConfig,
    ) -> Self {
        Self::new_with_agent_v2_and_escalation(agent, event_bus, config, None, None).await
    }

    /// Create a new orchestrator with an AgentV2 and optional external escalation channel
    /// When external_escalation_tx and external_escalation_rx are provided, they will be used
    /// instead of creating a new channel. This allows the orchestrator to share the channel
    /// with other components (like DelegateTool).
    pub async fn new_with_agent_v2_and_escalation(
        agent: Arc<Mutex<AgentV2>>,
        event_bus: Arc<EventBus>,
        config: OrchestratorConfig,
        external_escalation_tx: Option<Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
        external_escalation_rx: Option<Receiver<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
    ) -> Self {
        let job_registry = {
            let agent_lock = agent.lock().await;
            agent_lock.job_registry.clone()
        };
        
        // Use external channel if both sender and receiver are provided, otherwise create new
        let (escalation_tx, escalation_rx) = if let (Some(tx), Some(rx)) = (external_escalation_tx, external_escalation_rx) {
            (tx, rx)
        } else {
            Self::create_escalation_channel()
        };
        
        let orchestrator = Self {
            agent_v1: None,
            agent_v2: Some(agent),
            event_bus,
            config,
            terminal_delegate: None,
            job_registry,
            active_tasks: Arc::new(Mutex::new(Vec::new())),
            chat_session_queue: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            chat_session_sender: None,
            chat_session_receiver: Arc::new(Mutex::new(None)),
            chat_session_active: Arc::new(AtomicBool::new(false)),
            escalation_tx: Some(escalation_tx),
            escalation_rx: Arc::new(Mutex::new(Some(escalation_rx))),
            approval_tx: Arc::new(Mutex::new(None)),
        };
        
        // Spawn background tasks
        orchestrator.spawn_escalation_handler();
        
        orchestrator
    }
    
    /// Set the terminal executor delegate
    pub fn set_terminal_delegate(&mut self, delegate: Arc<dyn TerminalExecutor>) {
        self.terminal_delegate = Some(delegate);
    }
    
    /// Set auto-approve for terminal commands
    pub fn set_auto_approve(&mut self, auto_approve: bool) {
        self.config.auto_approve = auto_approve;
    }
    
    /// Send approval response for a pending tool execution
    pub async fn send_approval(&self, approved: bool) -> Result<(), String> {
        let tx_guard = self.approval_tx.lock().await;
        if let Some(ref tx) = *tx_guard {
            tx.send(approved).await
                .map_err(|_| "Failed to send approval - channel closed".to_string())?;
            Ok(())
        } else {
            Err("No pending approval request".to_string())
        }
    }
    
    /// Get a reference to the event bus
    pub fn event_bus(&self) -> Arc<EventBus> {
        self.event_bus.clone()
    }
    
    /// Get a reference to the job registry
    pub fn job_registry(&self) -> &JobRegistry {
        &self.job_registry
    }
    
    /// Start a new task with the given input message
    /// Returns a TaskHandle that can be used to interrupt the task
    pub async fn start_task(&self, task: String, history: Vec<ChatMessage>) -> TaskHandle {
        let handle = TaskHandle::new();
        
        // Store the handle
        {
            let mut tasks = self.active_tasks.lock().await;
            tasks.push(handle.clone());
        }
        
        // Create a job for this task
        let job_id = self.job_registry.create_job("orchestrator", &task);
        crate::info_log!("Orchestrator: Created job {} for task", job_id);
        
        // Create approval channel for this task
        let (approval_tx, approval_rx) = channel::<bool>(5);
        {
            let mut tx_guard = self.approval_tx.lock().await;
            *tx_guard = Some(approval_tx);
        }
        
        // Clone necessary data for the spawned task
        let agent_v1 = self.agent_v1.clone();
        let agent_v2 = self.agent_v2.clone();
        let event_bus = self.event_bus.clone();
        let config = self.config.clone();
        let interrupt_flag = handle.interrupt_flag.clone();
        let job_registry = self.job_registry.clone();
        let terminal_delegate = self.terminal_delegate.clone();
        let active_tasks = self.active_tasks.clone();
        let handle_id = handle.id.clone();
        let job_id_clone = job_id.clone();
        
        // Spawn the task
        tokio::spawn(async move {
            event_bus.publish(CoreEvent::StatusUpdate {
                message: format!("Task started: {}", &task[..task.len().min(50)]),
            });
            
            // Run the main loop
            let result = if let Some(agent) = agent_v1 {
                loops::run_agent_loop_v1(
                    agent,
                    event_bus.clone(),
                    interrupt_flag.clone(),
                    config,
                    job_registry.clone(),
                    Some(job_id_clone.clone()),
                    terminal_delegate,
                    task,
                    history,
                    Some(approval_rx),
                ).await
            } else if let Some(agent) = agent_v2 {
                crate::info_log!("Orchestrator: Starting V2 agent loop for job {}", job_id_clone);
                loops::run_agent_loop_v2(
                    agent,
                    event_bus.clone(),
                    interrupt_flag.clone(),
                    config,
                    job_registry.clone(),
                    Some(job_id_clone.clone()),
                    terminal_delegate,
                    task,
                    history,
                    Some(approval_rx),
                ).await
            } else {
                Err("No agent available".to_string())
            };
            
            // Handle result and update job status
            match result {
                Ok(_) => {
                    job_registry.complete_job(&job_id_clone, serde_json::json!({"status": "completed"}));
                    event_bus.publish(CoreEvent::StatusUpdate {
                        message: "Task completed".to_string(),
                    });
                    crate::info_log!("Orchestrator: Job {} completed", job_id_clone);
                }
                Err(e) => {
                    job_registry.fail_job(&job_id_clone, &e);
                    event_bus.publish(CoreEvent::StatusUpdate {
                        message: format!("Task failed: {}", e),
                    });
                    crate::info_log!("Orchestrator: Job {} failed: {}", job_id_clone, e);
                }
            }
            
            // Remove handle from active tasks
            let mut tasks = active_tasks.lock().await;
            tasks.retain(|t| t.id != handle_id);
        });
        
        handle
    }
    
    /// Start a continuous chat session
    /// 
    /// Unlike `start_task`, this:
    /// - Does NOT create a job for the main chat (only workers create jobs)
    /// - Processes user messages from a channel
    /// - Polls for worker events and injects job status
    /// - Runs continuously until interrupted
    /// - Allows interleaving of user chat and background worker processing
    /// 
    /// Returns a TaskHandle for controlling the session and a ChatSessionHandle for sending messages
    pub async fn start_chat_session(&self, history: Vec<ChatMessage>) -> (TaskHandle, ChatSessionHandle) {
        let handle = TaskHandle::new();
        
        // Store the handle
        {
            let mut tasks = self.active_tasks.lock().await;
            tasks.push(handle.clone());
        }
        
        // Create channel for message passing
        let (sender, receiver) = channel::<ChatSessionMessage>(100);
        
        // Create handle for external use
        let session_handle = ChatSessionHandle::new(sender.clone());
        
        // Mark chat session as active
        self.chat_session_active.store(true, Ordering::SeqCst);
        
        // Clone necessary data for the spawned task
        let agent_v2 = self.agent_v2.clone();
        let event_bus = self.event_bus.clone();
        let config = self.config.clone();
        let interrupt_flag = handle.interrupt_flag.clone();
        let job_registry = self.job_registry.clone();
        let terminal_delegate = self.terminal_delegate.clone();
        let active_tasks = self.active_tasks.clone();
        let handle_id = handle.id.clone();
        let chat_session_active = self.chat_session_active.clone();
        
        // Spawn the chat session
        tokio::spawn(async move {
            event_bus.publish(CoreEvent::StatusUpdate {
                message: "Chat session started".to_string(),
            });
            
            let result = if let Some(agent) = agent_v2 {
                loops::run_chat_session_loop_v2(
                    agent,
                    event_bus.clone(),
                    interrupt_flag.clone(),
                    config,
                    job_registry.clone(),
                    terminal_delegate,
                    history,
                    receiver,
                ).await
            } else {
                Err("Chat session requires V2 agent".to_string())
            };
            
            // Handle result
            match result {
                Ok(_) => {
                    event_bus.publish(CoreEvent::StatusUpdate {
                        message: "Chat session ended".to_string(),
                    });
                }
                Err(e) => {
                    event_bus.publish(CoreEvent::StatusUpdate {
                        message: format!("Chat session error: {}", e),
                    });
                }
            }
            
            // Mark chat session as inactive
            chat_session_active.store(false, Ordering::SeqCst);
            
            // Remove handle from active tasks
            let mut tasks = active_tasks.lock().await;
            tasks.retain(|t| t.id != handle_id);
        });
        
        (handle, session_handle)
    }
    
    /// Check if chat session is active
    pub fn is_chat_session_active(&self) -> bool {
        self.chat_session_active.load(Ordering::SeqCst)
    }
    
    /// Interrupt all active tasks
    pub async fn interrupt_all(&self) {
        let tasks = self.active_tasks.lock().await;
        for task in tasks.iter() {
            task.interrupt();
        }
    }
    
    /// Spawn a worker with the given objective (via DelegateTool)
    pub async fn spawn_worker(&self, objective: String) -> Result<String, String> {
        let job_id = self.job_registry.create_job("worker", &objective);
        
        self.event_bus.publish(CoreEvent::WorkerSpawned {
            job_id: job_id.clone(),
            description: objective,
        });
        
        Ok(job_id)
    }
    
    /// Cancel a job by ID
    pub async fn cancel_job(&self, job_id: &str) -> Result<(), String> {
        self.job_registry.cancel_job(job_id);
        self.event_bus.publish(CoreEvent::StatusUpdate {
            message: format!("Job {} cancelled", job_id),
        });
        Ok(())
    }

    /// Get the escalation sender for worker restricted command approval
    /// This is used by DelegateTool to allow workers to request command approval
    pub fn get_escalation_sender(&self) -> Option<Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>> {
        self.escalation_tx.clone()
    }

    /// Take the escalation receiver (can only be done once)
    /// This should be called by the chat session loop to handle escalation requests
    pub async fn take_escalation_receiver(&self) -> Option<Receiver<(EscalationRequest, oneshot::Sender<EscalationResponse>)>> {
        let mut rx_lock = self.escalation_rx.lock().await;
        rx_lock.take()
    }

    /// Process a single escalation request
    /// Returns true if approved, false if rejected
    pub async fn process_escalation_request(&self, request: &EscalationRequest) -> bool {
        crate::info_log!(
            "Worker [{}] escalation request: {} - Reason: {}",
            request.worker_id,
            request.command,
            request.reason
        );
        
        // Publish event for UI to show escalation request
        self.event_bus.publish(CoreEvent::StatusUpdate {
            message: format!(
                "🔒 Worker [{}] requests approval: {} - {}",
                &request.worker_id[..request.worker_id.len().min(8)],
                &request.command[..request.command.len().min(30)],
                request.reason
            ),
        });
        
        // For now, auto-reject escalations (user approval not yet implemented)
        // TODO: Implement user approval flow via UI
        crate::warn_log!("Escalation auto-rejected (user approval not implemented)");
        false
    }

    /// Spawn the escalation handler background task
    /// This listens for escalation requests from workers and processes them
    pub fn spawn_escalation_handler(&self) {
        if let Some(mut rx) = self.escalation_rx.clone().try_lock().ok().and_then(|mut g| g.take()) {
            let event_bus = self.event_bus.clone();
            
            tokio::spawn(async move {
                crate::info_log!("Escalation handler started");
                
                while let Some((request, response_tx)) = rx.recv().await {
                    crate::info_log!(
                        "Escalation request from worker [{}]: {} - {}",
                        request.worker_id,
                        request.command,
                        request.reason
                    );
                    
                    // Publish event for UI to show the escalation
                    event_bus.publish(CoreEvent::StatusUpdate {
                        message: format!(
                            "🔒 Worker [{}] requests approval for: {} ({})",
                            &request.worker_id[..request.worker_id.len().min(8)],
                            &request.command[..request.command.len().min(40)],
                            request.reason
                        ),
                    });
                    
                    // For now, auto-reject with explanation
                    // TODO: Integrate with UI for actual user approval
                    let approved = false;
                    let message = if approved {
                        "Escalation approved".to_string()
                    } else {
                        "Escalation rejected: Workers cannot execute restricted commands without approval".to_string()
                    };
                    
                    let response = EscalationResponse {
                        approved,
                        reason: Some(message),
                    };
                    
                    if let Err(e) = response_tx.send(response) {
                        crate::error_log!("Failed to send escalation response: {:?}", e);
                    }
                }
                
                crate::info_log!("Escalation handler stopped");
            });
        } else {
            crate::warn_log!("Escalation receiver already taken or locked");
        }
    }
}
