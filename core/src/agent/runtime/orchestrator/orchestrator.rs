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
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio::time::Duration;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use crate::agent::cognition::kernel::{GraphEngine};
use crate::agent::runtime::core::{AgencyRuntimeError, AgencyRuntime};
use crate::agent::runtime::orchestrator::transport::EventTransport;
use crate::agent::types::graph::IntentGraph;
use crate::agent::types::ids::IntentId;
use crate::agent::types::intents::Intent;
use crate::agent::types::observations::Observation;
use crate::agent::types::envelope::KernelEventEnvelope;
use crate::agent::types::{
    events::{KernelEvent, TokenUsage},
    ids::EventId,
};
use crate::agent::cognition::input::ApprovalOutcome;

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

    /// Graceful shutdown with timeout
    /// 
    /// Signals the session to stop and waits for in-flight operations
    /// to complete up to the specified timeout.
    async fn shutdown(&self, timeout: Duration) -> Result<(), &'static str>;

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
/// 
/// These events are serialized for transport to the UI layer (e.g., Tauri).
/// All variants must be serializable for IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum OutputEvent {
    /// Agent is thinking/processing
    Thinking { intent_id: IntentId },
    
    /// Tool is being executed
    ToolExecuting { intent_id: IntentId, tool: String, args: String },
    
    /// Tool completed
    ToolCompleted { intent_id: IntentId, result: String },
    
    /// Response chunk (for streaming)
    ResponseChunk { content: String },
    
    /// Response complete (with optional token usage for metrics)
    ResponseComplete { usage: Option<crate::agent::types::events::TokenUsage> },
    
    /// Approval requested
    ApprovalRequested { intent_id: IntentId, tool: String, args: String },
    
    /// Worker spawned
    WorkerSpawned { 
        worker_id: crate::agent::types::events::WorkerId, 
        job_id: crate::agent::runtime::orchestrator::commonbox::JobId,
        objective: String,
        agent_id: String, // worker config id like "file_lister"
    },
    
    /// Worker completed
    WorkerCompleted { 
        worker_id: crate::agent::types::events::WorkerId,
        job_id: crate::agent::runtime::orchestrator::commonbox::JobId,
    },
    
    /// Worker failed
    WorkerFailed { 
        worker_id: crate::agent::types::events::WorkerId,
        job_id: crate::agent::runtime::orchestrator::commonbox::JobId,
        error: String,
        is_stall: bool,
    },
    
    /// Worker tool executing (richer event for job tracking)
    WorkerToolExecuting {
        worker_id: crate::agent::types::events::WorkerId,
        job_id: crate::agent::runtime::orchestrator::commonbox::JobId,
        tool: String,
        args: String,
    },
    
    /// Worker tool completed (richer event for job tracking)
    WorkerToolCompleted {
        worker_id: crate::agent::types::events::WorkerId,
        job_id: crate::agent::runtime::orchestrator::commonbox::JobId,
        result: String,
    },
    
    /// Worker response complete with token usage (for job metrics tracking)
    WorkerResponseComplete {
        worker_id: crate::agent::types::events::WorkerId,
        job_id: crate::agent::runtime::orchestrator::commonbox::JobId,
        usage: Option<crate::agent::types::events::TokenUsage>,
    },
    
    /// Worker is thinking/processing (isolated from main agent thinking state)
    WorkerThinking {
        worker_id: crate::agent::types::events::WorkerId,
        job_id: crate::agent::runtime::orchestrator::commonbox::JobId,
    },
    
    /// Error occurred
    Error { message: String },
    
    /// Status update
    Status { message: String },
    
    /// Session halted
    Halted { reason: String },
    
    /// Context was pruned (smart pruning indicator)
    ContextPruned {
        /// Summary of what was pruned
        summary: String,
        /// Number of messages pruned
        message_count: usize,
        /// Approximate tokens saved
        tokens_saved: usize,
        /// Memories extracted before pruning
        extracted_memories: Vec<String>,
        /// Segment ID for recovery
        segment_id: String,
    },
}

/// Session result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResult {
    pub completed_successfully: bool,
    pub total_steps: usize,
    pub total_tokens: TokenUsage,
    pub halt_reason: Option<String>,
}

/// Session status snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Kernel(crate::agent::cognition::kernel::KernelError),
    Runtime(crate::agent::runtime::core::AgencyRuntimeError),
    Transport(crate::agent::runtime::orchestrator::transport::TransportError),
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
    K: GraphEngine + Send + Sync,
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
    next_event_id: AtomicU64,
    
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
    
    // Memory manager ownership - held here so Weak references in engine remain valid
    // The session owns this, engine's MemoryProvider holds Weak reference
    #[allow(dead_code)]
    memory_manager: Option<std::sync::Arc<crate::agent::memory::AgentMemoryManager>>,
    
    // Chunk pool for managing persistent file chunk workers
    // The session owns this, tools hold references to it
    
    chunk_pool: Option<std::sync::Arc<crate::agent::tools::ChunkPool>>,
    
    // INVARIANT: Transport identity check - ensures transport is never swapped
    
    transport_instance_id: u64,
}

impl<K, R, T> AgencySession<K, R, T>
where
    K: GraphEngine + Send + Sync,
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
        Self::new_with_memory(kernel, runtime, transport, output_tx, None)
    }
    
    /// Create a new session with memory manager
    /// 
    /// The memory manager is owned by the session and kept alive for its lifetime.
    /// The engine's MemoryProvider holds a Weak reference to it.
    pub fn new_with_memory(
        kernel: K, 
        runtime: R, 
        transport: T, 
        output_tx: broadcast::Sender<OutputEvent>,
        memory_manager: Option<std::sync::Arc<crate::agent::memory::AgentMemoryManager>>,
    ) -> Self {
        Self::new_full(kernel, runtime, transport, output_tx, memory_manager, None)
    }
    
    /// Create a new session with both memory manager and chunk pool
    /// 
    /// This is the full constructor that allows passing all optional components.
    /// The session owns these components and keeps them alive for its lifetime.
    pub fn new_full(
        kernel: K, 
        runtime: R, 
        transport: T, 
        output_tx: broadcast::Sender<OutputEvent>,
        memory_manager: Option<std::sync::Arc<crate::agent::memory::AgentMemoryManager>>,
        chunk_pool: Option<std::sync::Arc<crate::agent::tools::ChunkPool>>,
    ) -> Self {
        let (input_tx, input_rx) = mpsc::channel(100);
        let transport_instance_id = transport.instance_id();
        crate::info_log!("[SESSION] Creating session with transport instance_id: {}", transport_instance_id);
        
        Self {
            kernel,
            runtime,
            transport,
            pending_graph: None,
            completed_intents: HashSet::new(),
            intent_results: HashMap::new(),
            next_event_id: AtomicU64::new(1),
            in_flight: HashSet::new(),
            output_tx,
            input_rx,
            input_tx,
            interrupted: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            consecutive_errors: 0,
            max_consecutive_errors: 3,
            memory_manager,
            chunk_pool,
            transport_instance_id,
        }
    }

    /// Get a clone of the input sender
    /// 
    /// This allows sending input to the session while it's running
    pub fn input_sender(&self) -> mpsc::Sender<UserInput> {
        self.input_tx.clone()
    }
    
    /// Get the transport instance ID for debugging
    /// 
    /// Used to verify transport identity across session moves
    pub fn transport_instance_id(&self) -> u64 {
        self.transport.instance_id()
    }
    
    /// Get a reference to the chunk pool (if configured)
    pub fn chunk_pool(&self) -> Option<&Arc<crate::agent::tools::ChunkPool>> {
        self.chunk_pool.as_ref()
    }

    /// Generate next event ID
    fn next_event_id(&self) -> EventId {
        let id = self.next_event_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        EventId::new(id)
    }

    /// Publish an event to the transport
    async fn publish_event(&mut self, event: KernelEvent) -> Result<(), SessionError> {
        // INVARIANT: Verify transport has not been swapped
        let current_instance_id = self.transport.instance_id();
        assert_eq!(
            current_instance_id, self.transport_instance_id,
            "TRANSPORT INVARIANT VIOLATED in publish_event: Session created with transport instance_id: {}, but publishing via transport instance_id: {}",
            self.transport_instance_id, current_instance_id
        );
        
        let envelope = KernelEventEnvelope::new(
            self.next_event_id(),
            crate::agent::types::ids::NodeId::new(0), // Local node
            crate::agent::types::ids::LogicalClock::new(self.kernel.state().step_count as u64),
            crate::agent::types::ids::SessionId::generate(), // Should be real session ID
            event,
            0, // sequence
        );
        self.transport.publish(envelope).await.map_err(SessionError::Transport)
    }

    /// Abort the current pending execution graph.
    /// 
    /// Called when execution fails to prevent automatic retry of the same
    /// failing intent. Session does not automatically retry failed DAGs.
    /// User must provide new input to generate a fresh execution plan.
    fn abort_pending_graph(&mut self) {
        if self.pending_graph.is_some() {
            crate::warn_log!("[SESSION] Aborting pending execution graph ({} intents)", 
                self.pending_graph.as_ref().map(|g| g.len()).unwrap_or(0));
            self.pending_graph = None;
            // Also clear in-flight tracking since we're aborting
            self.in_flight.clear();
        }
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
        crate::debug_log!("[SESSION] execute_ready_intents CALLED");
        
        let Some(ref graph) = self.pending_graph else {
            crate::debug_log!("[SESSION] execute_ready_intents: NO PENDING GRAPH, returning empty");
            return Ok(Vec::new());
        };
        
        crate::debug_log!("[SESSION] execute_ready_intents: graph has {} nodes, completed_intents has {} ids", graph.len(), self.completed_intents.len());
        
        // Log all nodes and their dependencies
        for node in graph.nodes() {
            crate::debug_log!("[SESSION]   Node {}: deps={:?}", node.id.0, node.dependencies);
        }

        // Get ready intents
        let completed: Vec<_> = self.completed_intents.iter().copied().collect();
        let ready: Vec<_> = graph.ready_nodes(&completed)
            .into_iter()
            .cloned()
            .collect();
        
        crate::debug_log!("[SESSION] execute_ready_intents: {} ready intents (completed: {:?})", ready.len(), completed.iter().map(|i| i.0).collect::<Vec<_>>());

        if ready.is_empty() {
            crate::debug_log!("[SESSION] execute_ready_intents: NO READY INTENTS, returning empty");
            return Ok(Vec::new());
        }

        // Emit output events for ready intents
        for node in &ready {
            match &node.intent {
                Intent::CallTool(call) => {
                    let _ = self.output_tx.send(OutputEvent::ToolExecuting {
                        intent_id: node.id,
                        tool: call.name.clone(),
                        args: call.arguments.to_string(),
                    });
                }
                Intent::RequestLLM(_) => {
                    // Emit Thinking event so UI shows activity during memory fetch + LLM TTFB
                    let _ = self.output_tx.send(OutputEvent::Thinking {
                        intent_id: node.id,
                    });
                }
                Intent::EmitResponse(_text) => {
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
        
        crate::debug_log!("[SESSION] execute_dag returned {} observations", observations.len());
        for (i, (id, obs)) in observations.iter().enumerate() {
            crate::debug_log!("[SESSION] Observation {}: id={}, type={:?}", i, id.0, std::mem::discriminant(obs));
        }

        // Store results and update tracking
        let mut has_error = false;
        let mut error_msg = String::new();

        for (id, obs) in &observations {
            self.completed_intents.insert(*id);
            self.in_flight.remove(id);
            self.intent_results.insert(*id, obs.clone());
            
            // Check for response emitted - send completion event
            if let Observation::ResponseEmitted { .. } = obs {
                let _ = self.output_tx.send(OutputEvent::ResponseComplete { usage: None });
            }
            
            // Check for LLM/runtime errors - mark error but don't return early
            // Main loop will handle RuntimeError with circuit breaker logic
            if let Observation::RuntimeError { error, .. } = obs {
                has_error = true;
                error_msg = error.message.clone();
                
                crate::error_log!("[SESSION] RuntimeError observation in execute_ready_intents: {} (retryable={}, id={})", 
                    error.message, error.retryable, id.0);
                
                // Emit error event immediately so UI can show it
                let _ = self.output_tx.send(OutputEvent::Error {
                    message: format!("Runtime error: {}", error.message),
                });
                
                // Don't return early - let main loop handle circuit breaker logic
                // This keeps the session alive for retry
            }
        }

        // Track consecutive errors for backoff
        if has_error {
            self.consecutive_errors += 1;
            crate::error_log!("[SESSION] Consecutive error count: {}/{}", 
                self.consecutive_errors, self.max_consecutive_errors);
            
            // MANDATORY 3-second delay after ANY error before continuing
            // This prevents spamming the provider regardless of error type
            let delay = std::time::Duration::from_secs(3);
            crate::warn_log!("[SESSION] MANDATORY 3-second delay after error. Will halt if {}/{} errors", 
                self.consecutive_errors, self.max_consecutive_errors);
            tokio::time::sleep(delay).await;

            if self.consecutive_errors >= self.max_consecutive_errors {
                crate::error_log!("[SESSION] HALTING after {} consecutive errors", self.consecutive_errors);
                let _ = self.output_tx.send(OutputEvent::Error {
                    message: format!("STOPPING session after {} consecutive errors. Last error: {}", 
                        self.consecutive_errors, error_msg),
                });
                let _ = self.output_tx.send(OutputEvent::Halted {
                    reason: format!("Circuit breaker: {} consecutive errors", self.consecutive_errors),
                });
                return Err(SessionError::Internal(format!(
                    "Circuit breaker triggered after {} consecutive errors", 
                    self.consecutive_errors
                )));
            }
            
            crate::warn_log!("[SESSION] Continuing after error delay. Error count: {}/{}",
                self.consecutive_errors, self.max_consecutive_errors);
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
    K: GraphEngine + Send + Sync + 'static,
    R: AgencyRuntime + 'static,
    T: EventTransport + 'static,
{
    async fn run(&mut self) -> Result<SessionResult, SessionError> {
        crate::info_log!("[SESSION] Session started, transport instance_id: {}", self.transport.instance_id());
        
        // INVARIANT: Verify transport has not been swapped
        let current_instance_id = self.transport.instance_id();
        assert_eq!(
            current_instance_id, self.transport_instance_id,
            "TRANSPORT INVARIANT VIOLATED: Session created with transport instance_id: {}, but run() sees transport instance_id: {}",
            self.transport_instance_id, current_instance_id
        );
        crate::info_log!("[SESSION] Transport identity verified: instance_id {}", current_instance_id);
        
        // Initial state check - if we already have a pending graph, execute it
        if self.pending_graph.is_some() {
            let observations = self.execute_ready_intents().await?;
            for (_, obs) in &observations {
                if let Observation::RuntimeError { error, .. } = obs {
                    let _ = self.output_tx.send(OutputEvent::Halted {
                        reason: format!("Runtime error: {}", error.message),
                    });
                    return Err(SessionError::Runtime(AgencyRuntimeError::Internal {
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
                    return Err(SessionError::Runtime(AgencyRuntimeError::Internal {
                        message: error.message.clone(),
                    }));
                }
                self.publish_event(obs.clone().into_event()).await?;
            }
        }

        loop {
            // Loop iteration only logged at trace level to reduce noise
            // Error count is logged when errors occur
            
            // Check for interrupt
            if self.is_interrupted() {
                return Err(SessionError::Interrupted);
            }
            
            // Circuit breaker: halt if too many consecutive errors
            if self.consecutive_errors >= self.max_consecutive_errors {
                crate::error_log!("[SESSION] CIRCUIT BREAKER TRIGGERED: {} errors >= {} max", 
                    self.consecutive_errors, self.max_consecutive_errors);
                let _ = self.output_tx.send(OutputEvent::Error {
                    message: format!("Session halted after {} consecutive errors", self.consecutive_errors),
                });
                let _ = self.output_tx.send(OutputEvent::Halted {
                    reason: format!("Circuit breaker: {} consecutive errors", self.consecutive_errors),
                });
                return Err(SessionError::Internal(format!(
                    "Circuit breaker triggered after {} consecutive errors", 
                    self.consecutive_errors
                )));
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
                    crate::debug_log!("[SESSION] transport.next_batch() returned");
                    match batch {
                        Ok(batch) => {
                            let events: Vec<KernelEvent> = batch.into_iter().map(|e| e.payload).collect();
                            if !events.is_empty() {
                                crate::debug_log!("[SESSION] Processing {} events", events.len());
                                
                                let new_graph = self.process_events(events).await?;
                                crate::debug_log!("[SESSION] process_events returned graph with {} nodes", new_graph.len());
                                
                                // Merge into pending graph
                                if let Some(ref mut pending) = self.pending_graph {
                                    let old_len = pending.len();
                                    pending.merge(new_graph);
                                    crate::debug_log!("[SESSION] Merged into pending graph: {} + new = {} nodes", old_len, pending.len());
                                } else {
                                    crate::debug_log!("[SESSION] Setting pending_graph to new graph with {} nodes", new_graph.len());
                                    self.pending_graph = Some(new_graph);
                                }
                                
                                // Execute any ready intents from new graph
                                let observations = match self.execute_ready_intents().await {
                                    Ok(obs) => obs,
                                    Err(e) => {
                                        crate::error_log!("[SESSION] execute_ready_intents FAILED: {:?}", e);
                                        // Increment consecutive errors for circuit breaker
                                        self.consecutive_errors += 1;
                                        crate::error_log!("[SESSION] Consecutive error count: {}/{}", 
                                            self.consecutive_errors, self.max_consecutive_errors);
                                        
                                        // ALWAYS wait 3 seconds after any error
                                        crate::warn_log!("[SESSION] MANDATORY 3-second delay after error");
                                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                                        
                                        // Abort the failed execution graph to prevent automatic retry.
                                        // Session does not automatically retry failed DAGs.
                                        self.abort_pending_graph();
                                        
                                        if self.consecutive_errors >= self.max_consecutive_errors {
                                            crate::error_log!("[SESSION] HALTING after {} consecutive errors", 
                                                self.consecutive_errors);
                                            let _ = self.output_tx.send(OutputEvent::Error {
                                                message: format!("STOPPING after {} errors", self.consecutive_errors),
                                            });
                                            let _ = self.output_tx.send(OutputEvent::Halted {
                                                reason: format!("Circuit breaker: {} errors", self.consecutive_errors),
                                            });
                                            return Err(e);
                                        }
                                        
                                        // Continue to next loop iteration.
                                        // Session now waits for new events (user input) rather than
                                        // retrying the same failing intent.
                                        continue;
                                    }
                                };
                                
                                // Reset consecutive errors on success
                                if self.consecutive_errors > 0 {
                                    self.consecutive_errors = 0;
                                    crate::info_log!("[SESSION] Error count reset after success");
                                }
                                
                                // Check for runtime errors first - handle them with circuit breaker logic
                                let mut runtime_error_handled = false;
                                for (_, obs) in &observations {
                                    if let Observation::RuntimeError { error, .. } = obs {
                                        crate::error_log!("[SESSION] Runtime error detected in main loop: {} (retryable={})", 
                                            error.message, error.retryable);
                                        
                                        // Send error event so UI shows it
                                        let _ = self.output_tx.send(OutputEvent::Error {
                                            message: format!("Runtime error: {}", error.message),
                                        });
                                        
                                        // Increment consecutive errors for circuit breaker
                                        self.consecutive_errors += 1;
                                        crate::error_log!("[SESSION] Consecutive error count: {}/{}", 
                                            self.consecutive_errors, self.max_consecutive_errors);
                                        
                                        // ALWAYS wait 3 seconds after any error
                                        crate::warn_log!("[SESSION] MANDATORY 3-second delay after RuntimeError");
                                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                                        
                                        // Abort the failed execution graph to prevent automatic retry
                                        self.abort_pending_graph();
                                        
                                        // Check if we've hit the circuit breaker limit
                                        if self.consecutive_errors >= self.max_consecutive_errors {
                                            crate::error_log!("[SESSION] CIRCUIT BREAKER: Halting after {} consecutive errors", 
                                                self.consecutive_errors);
                                            let _ = self.output_tx.send(OutputEvent::Halted {
                                                reason: format!("Circuit breaker: {} consecutive RuntimeErrors", self.consecutive_errors),
                                            });
                                            return Err(SessionError::Internal(format!(
                                                "Circuit breaker triggered after {} consecutive RuntimeErrors", 
                                                self.consecutive_errors
                                            )));
                                        }
                                        
                                        // Mark as handled - will continue main loop after this block
                                        runtime_error_handled = true;
                                        crate::info_log!("[SESSION] RuntimeError handled, will continue to next loop iteration");
                                        break;
                                    }
                                }
                                
                                // If we handled a runtime error, skip to next main loop iteration
                                if runtime_error_handled {
                                    continue;
                                }
                                
                                // Send OutputEvents for observations that UI needs to display
                                for (intent_id, obs) in &observations {
                                    match obs {
                                        Observation::ToolCompleted { result, .. } => {
                                            let output = match result {
                                                crate::agent::types::events::ToolResult::Success { output, .. } => output.clone(),
                                                crate::agent::types::events::ToolResult::Error { message, .. } => format!("Error: {}", message),
                                                crate::agent::types::events::ToolResult::Cancelled => "Cancelled".to_string(),
                                            };
                                            let _ = self.output_tx.send(OutputEvent::ToolCompleted {
                                                intent_id: *intent_id,
                                                result: output,
                                            });
                                        }
                                        Observation::WorkerSpawned { worker_id, job_id, objective, agent_id, .. } => {
                                            let _ = self.output_tx.send(OutputEvent::WorkerSpawned {
                                                worker_id: *worker_id,
                                                job_id: *job_id,
                                                objective: objective.clone(),
                                                agent_id: agent_id.clone(),
                                            });
                                        }
                                        Observation::WorkerCompleted { worker_id, job_id, .. } => {
                                            let _ = self.output_tx.send(OutputEvent::WorkerCompleted {
                                                worker_id: *worker_id,
                                                job_id: *job_id,
                                            });
                                        }
                                        Observation::LLMCompleted { .. } => {
                                            // Send completion signal only (ResponseChunk already sent by streaming)
                                            let _ = self.output_tx.send(OutputEvent::ResponseComplete { usage: None });
                                        }
                                        _ => {} // Other observations handled elsewhere or not needed for UI
                                    }
                                }
                                
                                // Check for halt observation
                                for (_, obs) in &observations {
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
                                // BUT skip RuntimeError to prevent feedback loops (already handled above)
                                for (_, obs) in observations {
                                    if matches!(obs, Observation::RuntimeError { .. }) {
                                        continue;
                                    }
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
                    crate::debug_log!("[SESSION] input_rx.recv() returned: {:?}", input.is_some());                    match input {
                        Some(UserInput::Message(content)) => {
                            crate::info_log!("[SESSION] User message received ({} bytes)", content.len());
                            self.publish_event(KernelEvent::UserMessage { content }).await?;
                        }
                        Some(UserInput::Command(cmd)) => {
                            crate::debug_log!("[SESSION] Command received: {}", &cmd);
                            self.publish_event(KernelEvent::UserMessage { content: cmd }).await?;
                        }
                        Some(UserInput::Approval { intent_id, approved }) => {
                            crate::info_log!("[SESSION] Approval for intent_id={}: approved={}", 
                                intent_id.0, approved);
                            self.publish_event(KernelEvent::ApprovalGiven {
                                intent_id,
                                outcome: if approved {
                                    ApprovalOutcome::Granted
                                } else {
                                    ApprovalOutcome::Denied { reason: Some("User rejected".to_string()) }
                                },
                            }).await?;
                            crate::info_log!("[SESSION] ApprovalGiven event published, loop will continue");
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
        
        // Handle interrupt immediately
        if matches!(input, UserInput::Interrupt) {
            self.interrupt().await;
            return Ok(());
        }
        
        // Send UserInput directly to the input channel
        // The main run() loop will receive it and convert to events
        self.input_tx.send(input).await
            .map_err(|_| SessionError::Internal("Input channel closed".to_string()))
    }

    fn subscribe_output(&self) -> broadcast::Receiver<OutputEvent> {
        self.output_tx.subscribe()
    }

    async fn interrupt(&self) {
        self.interrupted.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    async fn shutdown(&self, timeout: Duration) -> Result<(), &'static str> {
        // Signal shutdown
        self.interrupted.store(true, std::sync::atomic::Ordering::SeqCst);
        
        // Wait for in-flight operations to complete
        match tokio::time::timeout(timeout, async {
            loop {
                let status = self.status();
                if status.pending_intents == 0 && !status.running {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }).await {
            Ok(_) => {
                crate::info_log!("[SESSION] Graceful shutdown completed");
                Ok(())
            }
            Err(_) => {
                crate::warn_log!("[SESSION] Shutdown timeout, forcing exit");
                Err("shutdown timeout")
            }
        }
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
            OutputEvent::ResponseComplete { usage: None },
        ];
        
        assert_eq!(events.len(), 3);
    }
}
