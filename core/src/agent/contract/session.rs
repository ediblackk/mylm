//! Session - orchestration layer with dynamic DAG expansion
//!
//! The Session is where:
//! - Async lives (the main loop is async)
//! - Kernel events flow in
//! - Intents are executed by runtime
//! - DAG expands dynamically based on observations
//! - Output is buffered for the UI
//!
//! # Distributed Execution Model
//!
//! This system uses a **Leader-Worker** model for distributed execution:
//!
//! ## Roles
//!
//! - **Leader** (Session): Owns the DAG, assigns IntentIds, tracks completion
//! - **Worker** (Runtime): Executes intents, returns observations
//! - **Kernel**: Pure function, knows nothing about distribution
//! - **Transport**: Moves events between nodes, preserves FIFO per session
//!
//! ## Execution Guarantees
//!
//! 1. **At-least-once delivery**: Events may be delivered multiple times
//! 2. **Exactly-once execution**: Leader deduplicates by IntentId
//! 3. **Idempotent intents**: All intents must be safe to retry
//! 4. **FIFO per session**: Events arrive in order per session (Leader enforces)
//!
//! ## IntentId Assignment
//!
//! IntentIds are **deterministically derived** from kernel state:
//! ```text
//! IntentId = (step_count << 32) | intent_index
//! ```
//!
//! This ensures:
//! - Replay generates identical IDs
//! - Workers never compute IDs (only Leader)
//! - Deterministic ordering across nodes
//!
//! ## Deduplication
//!
//! Leader maintains two sets:
//! - `completed`: Intents that finished successfully
//! - `in_flight`: Intents currently executing
//!
//! If duplicate result arrives:
//! - Ignore if in `completed`
//! - Update if in `in_flight` (idempotent)
//!
//! ## Failure Recovery
//!
//! If worker crashes:
//! - Leader detects timeout
//! - Leader reassigns same IntentId to new worker
//! - Intent executes again (idempotent)
//!
//! If leader crashes:
//! - Session dies (for now)
//! - No split-brain recovery (distributed consensus not implemented)
//!
//! ## Event Ordering
//!
//! Kernel only sees **already-ordered** events:
//! - Transport preserves FIFO per session
//! - Leader assigns LogicalClock before kernel sees event
//! - Replay uses persisted event log with original ordering

use async_trait::async_trait;
use tokio::sync::{broadcast, mpsc};
use std::collections::{HashMap, HashSet};

use super::{
    AgencyKernel,
    runtime::AgencyRuntime,
    transport::EventTransport,
    events::{KernelEvent, TokenUsage},
    graph::IntentGraph,
    observations::Observation,
    ids::{IntentId, EventId},
    envelope::KernelEventEnvelope,
};
use crate::agent::cognition::ApprovalOutcome;

/// Session orchestrates the kernel-runtime loop
///
/// This is the main coordination point. It:
/// 1. Receives events from transport
/// 2. Feeds them to kernel
/// 3. Gets back IntentGraph
/// 4. Executes intents via runtime
/// 5. Streams observations back
/// 6. Expands graph dynamically
#[async_trait]
pub trait Session: Send + Sync {
    /// Run the session loop until completion
    ///
    /// This is the main entry point. It runs until:
    /// - Kernel emits Halt intent
    /// - User interrupts
    /// - Error occurs
    async fn run(&mut self) -> Result<SessionResult, SessionError>;

    /// Submit user input to the session
    ///
    /// This can be called while run() is active.
    /// The input will be picked up on next loop iteration.
    async fn submit_input(&self, input: UserInput) -> Result<(), SessionError>;

    /// Subscribe to output events (for UI)
    fn subscribe_output(&self) -> broadcast::Receiver<OutputEvent>;

    /// Interrupt the session
    async fn interrupt(&self);

    /// Get current session state summary
    fn status(&self) -> SessionStatus;
}

/// User input types
#[derive(Debug, Clone)]
pub enum UserInput {
    /// User message
    Message(String),
    
    /// Command (slash command)
    Command(String),
    
    /// Approval/denial for a tool
    Approval { intent_id: IntentId, approved: bool },
    
    /// Interrupt request
    Interrupt,
}

/// Output events for UI
#[derive(Debug, Clone)]
pub enum OutputEvent {
    /// Agent is thinking/processing
    Thinking { intent_id: IntentId },
    
    /// Tool is being executed
    ToolExecuting { intent_id: IntentId, tool: String, args: String },
    
    /// Tool completed
    ToolCompleted { intent_id: IntentId, result: String },
    
    /// Response chunk (for streaming)
    ResponseChunk { content: String },
    
    /// Response complete
    ResponseComplete,
    
    /// Approval requested
    ApprovalRequested { intent_id: IntentId, tool: String, args: String },
    
    /// Worker spawned
    WorkerSpawned { worker_id: super::events::WorkerId, objective: String },
    
    /// Worker completed
    WorkerCompleted { worker_id: super::events::WorkerId },
    
    /// Error occurred
    Error { message: String },
    
    /// Status update
    Status { message: String },
    
    /// Session halted
    Halted { reason: String },
}

/// Session result
#[derive(Debug, Clone)]
pub struct SessionResult {
    pub completed_successfully: bool,
    pub total_steps: usize,
    pub total_tokens: TokenUsage,
    pub halt_reason: Option<String>,
}

/// Session status snapshot
#[derive(Debug, Clone)]
pub struct SessionStatus {
    pub running: bool,
    pub step_count: usize,
    pub pending_intents: usize,
    pub active_workers: usize,
    pub waiting_for_approval: bool,
}

/// Session errors
#[derive(Debug)]
pub enum SessionError {
    Kernel(super::kernel::KernelError),
    Runtime(super::runtime::RuntimeError),
    Transport(super::transport::TransportError),
    Interrupted,
    Internal(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Kernel(e) => write!(f, "Kernel error: {}", e),
            SessionError::Runtime(e) => write!(f, "Runtime error: {}", e),
            SessionError::Transport(e) => write!(f, "Transport error: {}", e),
            SessionError::Interrupted => write!(f, "Session interrupted"),
            SessionError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for SessionError {}

/// Generic Session implementation
///
/// This ties together Kernel + Runtime + Transport
pub struct AgencySession<K, R, T>
where
    K: AgencyKernel + Send + Sync,
    R: AgencyRuntime,
    T: EventTransport,
{
    kernel: K,
    runtime: R,
    transport: T,
    
    // DAG expansion state
    pending_graph: Option<IntentGraph>,
    completed_intents: HashSet<IntentId>,
    intent_results: HashMap<IntentId, Observation>,
    
    // Event sequencing (for envelope generation, not intent IDs)
    next_event_id: u64,
    
    // Distributed execution tracking (Leader only)
    in_flight: HashSet<IntentId>,
    
    // Output channel
    output_tx: broadcast::Sender<OutputEvent>,
    
    // Input channel
    input_rx: mpsc::Receiver<UserInput>,
    input_tx: mpsc::Sender<UserInput>,
    
    // Control
    interrupted: std::sync::Arc<std::sync::atomic::AtomicBool>,
    
    // Error tracking for backoff
    consecutive_errors: u32,
    max_consecutive_errors: u32,
}

impl<K, R, T> AgencySession<K, R, T>
where
    K: AgencyKernel + Send + Sync,
    R: AgencyRuntime,
    T: EventTransport,
{
    /// Create a new session
    pub fn new(kernel: K, runtime: R, transport: T) -> Self {
        let (output_tx, _) = broadcast::channel(100);
        Self::new_with_output(kernel, runtime, transport, output_tx)
    }
    
    /// Create a new session with a pre-configured output channel
    pub fn new_with_output(kernel: K, runtime: R, transport: T, output_tx: broadcast::Sender<OutputEvent>) -> Self {
        let (input_tx, input_rx) = mpsc::channel(100);
        
        Self {
            kernel,
            runtime,
            transport,
            pending_graph: None,
            completed_intents: HashSet::new(),
            intent_results: HashMap::new(),
            next_event_id: 1,
            in_flight: HashSet::new(),
            output_tx,
            input_rx,
            input_tx,
            interrupted: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            consecutive_errors: 0,
            max_consecutive_errors: 3,
        }
    }

    /// Get a clone of the input sender
    /// 
    /// This allows sending input to the session while it's running
    pub fn input_sender(&self) -> mpsc::Sender<UserInput> {
        self.input_tx.clone()
    }

    /// Generate next event ID
    fn next_event_id(&mut self) -> EventId {
        let id = EventId::new(self.next_event_id);
        self.next_event_id += 1;
        id
    }

    /// Publish an event to the transport
    async fn publish_event(&mut self, event: KernelEvent) -> Result<(), SessionError> {
        let envelope = KernelEventEnvelope::new(
            self.next_event_id(),
            super::ids::NodeId::new(0), // Local node
            super::ids::LogicalClock::new(self.kernel.state().step_count as u64),
            super::ids::SessionId::generate(), // Should be real session ID
            event,
            0, // sequence
        );
        self.transport.publish(envelope).await.map_err(SessionError::Transport)
    }

    /// Process a batch of events through the kernel
    async fn process_events(&mut self, events: Vec<KernelEvent>) -> Result<IntentGraph, SessionError> {
        // Process through kernel
        let graph = self.kernel.process(&events)
            .map_err(SessionError::Kernel)?;
        
        Ok(graph)
    }

    /// Execute ready intents from the pending graph
    async fn execute_ready_intents(&mut self) -> Result<Vec<(IntentId, Observation)>, SessionError> {
        let Some(ref graph) = self.pending_graph else {
            return Ok(Vec::new());
        };

        // Get ready intents
        let ready: Vec<_> = graph.ready_nodes(&self.completed_intents.iter().copied().collect::<Vec<_>>())
            .into_iter()
            .cloned()
            .collect();

        if ready.is_empty() {
            return Ok(Vec::new());
        }

        // Emit output events for ready intents
        for node in &ready {
            match &node.intent {
                super::intents::Intent::CallTool(call) => {
                    let _ = self.output_tx.send(OutputEvent::ToolExecuting {
                        intent_id: node.id,
                        tool: call.name.clone(),
                        args: call.arguments.to_string(),
                    });
                }
                super::intents::Intent::EmitResponse(_text) => {
                    // Don't send chunk here - runtime already streamed it
                    // ResponseComplete will be sent after execution
                }
                _ => {}
            }
        }

        // Track intents as in-flight (for distributed execution tracking)
        for node in &ready {
            self.in_flight.insert(node.id);
        }

        // Execute via runtime
        let observations = self.runtime.execute_dag(graph).await
            .map_err(SessionError::Runtime)?;

        // Store results and update tracking
        let mut has_error = false;
        let mut error_msg = String::new();

        for (id, obs) in &observations {
            self.completed_intents.insert(*id);
            self.in_flight.remove(id);
            self.intent_results.insert(*id, obs.clone());
            
            // Check for response emitted - send completion event
            if let Observation::ResponseEmitted { .. } = obs {
                let _ = self.output_tx.send(OutputEvent::ResponseComplete);
            }
            
            // Check for LLM/runtime errors
            if let Observation::RuntimeError { error, .. } = obs {
                has_error = true;
                error_msg = error.message.clone();
                
                // Emit error event immediately so UI can show it
                let _ = self.output_tx.send(OutputEvent::Error {
                    message: format!("Runtime error: {}", error.message),
                });

                // Immediate halt if not retryable
                if !error.retryable {
                    let _ = self.output_tx.send(OutputEvent::Halted {
                        reason: format!("Non-retryable runtime error: {}", error.message),
                    });
                    return Err(SessionError::Runtime(super::runtime::RuntimeError::Internal {
                        message: format!("Non-retryable runtime error: {}", error.message)
                    }));
                }
            }
        }

        // Track consecutive errors for backoff
        if has_error {
            self.consecutive_errors += 1;
            crate::error_log!("[SESSION] Consecutive error count: {}/{}", 
                self.consecutive_errors, self.max_consecutive_errors);
            
            // Add backoff delay to prevent spamming
            let delay_ms = 500 * (2u64.pow(self.consecutive_errors.saturating_sub(1)));
            let delay = std::time::Duration::from_millis(delay_ms.min(5000));
            crate::warn_log!("[SESSION] Error detected, backing off for {:?}", delay);
            tokio::time::sleep(delay).await;

            if self.consecutive_errors >= self.max_consecutive_errors {
                let _ = self.output_tx.send(OutputEvent::Error {
                    message: format!("Stopping session after {} consecutive errors. Last error: {}", 
                        self.consecutive_errors, error_msg),
                });
                return Err(SessionError::Internal("Max consecutive errors reached".to_string()));
            }
        } else if !observations.is_empty() {
            // Reset error count ONLY on success with actual observations
            if self.consecutive_errors > 0 {
                self.consecutive_errors = 0;
                crate::info_log!("[SESSION] Error count reset after success");
            }
        }

        Ok(observations)
    }

    /// Check if interrupted
    fn is_interrupted(&self) -> bool {
        self.interrupted.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl<K, R, T> Session for AgencySession<K, R, T>
where
    K: AgencyKernel + Send + Sync + 'static,
    R: AgencyRuntime + 'static,
    T: EventTransport + 'static,
{
    async fn run(&mut self) -> Result<SessionResult, SessionError> {
        crate::info_log!("[SESSION] Session started");
        
        // Initial state check - if we already have a pending graph, execute it
        if self.pending_graph.is_some() {
            let observations = self.execute_ready_intents().await?;
            for (_, obs) in &observations {
                if let Observation::RuntimeError { error, .. } = obs {
                    let _ = self.output_tx.send(OutputEvent::Halted {
                        reason: format!("Runtime error: {}", error.message),
                    });
                    return Err(SessionError::Runtime(super::runtime::RuntimeError::Internal {
                        message: error.message.clone(),
                    }));
                }
                self.publish_event(obs.clone().into_event()).await?;
            }
        } else if self.kernel.state().step_count == 0 {
            // First run - trigger kernel to get starting intents
            let initial_graph = self.process_events(vec![]).await?;
            self.pending_graph = Some(initial_graph);
            
            let observations = self.execute_ready_intents().await?;
            for (_, obs) in &observations {
                if let Observation::RuntimeError { error, .. } = obs {
                    let _ = self.output_tx.send(OutputEvent::Halted {
                        reason: format!("Runtime error: {}", error.message),
                    });
                    return Err(SessionError::Runtime(super::runtime::RuntimeError::Internal {
                        message: error.message.clone(),
                    }));
                }
                self.publish_event(obs.clone().into_event()).await?;
            }
        }

        loop {
            // Check for interrupt
            if self.is_interrupted() {
                return Err(SessionError::Interrupted);
            }

            // Check if graph complete
            if let Some(ref graph) = self.pending_graph {
                if graph.is_complete(&self.completed_intents.iter().copied().collect::<Vec<_>>()) {
                    crate::debug_log!("[SESSION] Graph complete, step_count={}", self.kernel.state().step_count);
                    
                    // Check for halt
                    if self.kernel.is_terminal() {
                        crate::info_log!("[SESSION] Session halted");
                        let _ = self.output_tx.send(OutputEvent::Halted {
                            reason: "Completed".to_string(),
                        });
                        break;
                    }
                    
                    // Graph complete but not halted - wait for more input
                    crate::debug_log!("[SESSION] Waiting for input");
                    self.pending_graph = None;
                }
            }

            // Wait for either transport events or user input
            tokio::select! {
                // Transport events
                batch = self.transport.next_batch() => {
                    match batch {
                        Ok(batch) => {
                            let events: Vec<KernelEvent> = batch.into_iter().map(|e| e.payload).collect();
                            if !events.is_empty() {
                                crate::debug_log!("[SESSION] Processing {} events", events.len());
                                
                                let new_graph = self.process_events(events).await?;
                                
                                // Merge into pending graph
                                if let Some(ref mut pending) = self.pending_graph {
                                    pending.merge(new_graph);
                                } else {
                                    self.pending_graph = Some(new_graph);
                                }
                                
                                // Execute any ready intents from new graph
                                let observations = self.execute_ready_intents().await?;
                                
                                // Check for halt or runtime error observation
                                for (_, obs) in &observations {
                                    if let Observation::RuntimeError { error, .. } = obs {
                                        crate::error_log!("[SESSION] Runtime error received, stopping loop to prevent feedback loop");
                                        let _ = self.output_tx.send(OutputEvent::Halted {
                                            reason: format!("Runtime error: {}", error.message),
                                        });
                                        return Err(SessionError::Runtime(super::runtime::RuntimeError::Internal {
                                            message: error.message.clone(),
                                        }));
                                    }

                                    if matches!(obs, Observation::Halted { .. }) {
                                        crate::info_log!("[SESSION] Halt observation received, stopping loop");
                                        let _ = self.output_tx.send(OutputEvent::Halted {
                                            reason: "Agent halted".to_string(),
                                        });
                                        return Ok(SessionResult {
                                            completed_successfully: true,
                                            total_steps: self.kernel.state().step_count,
                                            total_tokens: TokenUsage::default(),
                                            halt_reason: self.kernel.state().halt_reason.clone(),
                                        });
                                    }
                                }
                                
                                // Convert to events and publish back to transport for potential expansion
                                for (_, obs) in observations {
                                    let event = obs.clone().into_event();
                                    crate::debug_log!("[SESSION] Publishing observation: {:?}", event);
                                    self.publish_event(event).await?;
                                }
                            }
                        }
                        Err(e) => return Err(SessionError::Transport(e)),
                    }
                }
                
                // User input
                input = self.input_rx.recv() => {
                    match input {
                        Some(UserInput::Message(content)) => {
                            crate::info_log!("[SESSION] User message received ({} bytes)", content.len());
                            self.publish_event(KernelEvent::UserMessage { content }).await?;
                        }
                        Some(UserInput::Command(cmd)) => {
                            crate::debug_log!("[SESSION] Command received: {}", &cmd);
                            self.publish_event(KernelEvent::UserMessage { content: cmd }).await?;
                        }
                        Some(UserInput::Approval { intent_id, approved }) => {
                            crate::debug_log!("[SESSION] Approval for intent_id={}: approved={}", 
                                intent_id.0, approved);
                            self.publish_event(KernelEvent::ApprovalGiven {
                                intent_id,
                                outcome: if approved {
                                    ApprovalOutcome::Granted
                                } else {
                                    ApprovalOutcome::Denied { reason: Some("User rejected".to_string()) }
                                },
                            }).await?;
                        }
                        Some(UserInput::Interrupt) => {
                            crate::warn_log!("[SESSION] Received UserInput::Interrupt");
                            return Err(SessionError::Interrupted);
                        }
                        None => {
                            crate::error_log!("[SESSION] Input channel closed (recv returned None)");
                            break;
                        }
                    }
                }
            }

            // Small yield to prevent tight loop when idle
            if self.pending_graph.is_none() {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        }

        Ok(SessionResult {
            completed_successfully: true,
            total_steps: self.kernel.state().step_count,
            total_tokens: TokenUsage::default(), // Aggregate from results
            halt_reason: self.kernel.state().halt_reason.clone(),
        })
    }

    async fn submit_input(&self, input: UserInput) -> Result<(), SessionError> {
        crate::info_log!("[SESSION] submit_input called with: {:?}", std::mem::discriminant(&input));
        match self.input_tx.send(input).await {
            Ok(_) => {
                crate::info_log!("[SESSION] submit_input: message sent successfully");
                Ok(())
            }
            Err(_e) => {
                crate::error_log!("[SESSION] submit_input: failed to send - channel closed");
                Err(SessionError::Internal("Input channel closed".to_string()))
            }
        }
    }

    fn subscribe_output(&self) -> broadcast::Receiver<OutputEvent> {
        self.output_tx.subscribe()
    }

    async fn interrupt(&self) {
        self.interrupted.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn status(&self) -> SessionStatus {
        SessionStatus {
            running: true,
            step_count: self.kernel.state().step_count,
            pending_intents: self.pending_graph.as_ref().map(|g| g.len()).unwrap_or(0),
            active_workers: self.kernel.state().active_workers,
            waiting_for_approval: self.kernel.state().has_pending_approvals(),
        }
    }
}

// cognition module imported at top level already

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_status() {
        // Would need mock implementations to test properly
    }

    #[test]
    fn test_user_input_variants() {
        let msg = UserInput::Message("hello".to_string());
        let cmd = UserInput::Command("/help".to_string());
        let approval = UserInput::Approval { intent_id: IntentId::new(1), approved: true };
        
        // Just verify they compile
        let _ = (msg, cmd, approval);
    }

    #[test]
    fn test_output_event_variants() {
        let events = vec![
            OutputEvent::Thinking { intent_id: IntentId::new(1) },
            OutputEvent::ResponseChunk { content: "hello".to_string() },
            OutputEvent::ResponseComplete,
        ];
        
        assert_eq!(events.len(), 3);
    }
}
