//! Session orchestrator
//!
//! Coordinates cognition and runtime layers.
//! Receives SessionInput, produces cognition steps, interprets decisions.

use std::sync::Arc;
use crate::agent::cognition::engine::CognitiveEngine;
use crate::agent::cognition::input::InputEvent;
use crate::agent::cognition::state::AgentState;
use crate::agent::types::events::WorkerId;
use crate::agent::cognition::decision::{AgentDecision, AgentExitReason};
use crate::agent::cognition::error::CognitiveError;
use crate::agent::runtime::{AgentRuntime, RuntimeContext, RuntimeError};
use crate::agent::session::input::{SessionInput, WorkerEvent};
use crate::agent::session::persistence::{SessionPersistence, PersistedSession, SessionBuilder};
use crate::agent::memory::AgentMemoryManager;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Receiver;

/// Session error
#[derive(Debug)]
pub enum SessionError {
    Cognitive(CognitiveError),
    Runtime(RuntimeError),
    Cancelled,
}

impl From<CognitiveError> for SessionError {
    fn from(e: CognitiveError) -> Self {
        SessionError::Cognitive(e)
    }
}

impl From<RuntimeError> for SessionError {
    fn from(e: RuntimeError) -> Self {
        SessionError::Runtime(e)
    }
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Cognitive(e) => write!(f, "Cognitive error: {}", e),
            SessionError::Runtime(e) => write!(f, "Runtime error: {}", e),
            SessionError::Cancelled => write!(f, "Session cancelled"),
        }
    }
}

impl std::error::Error for SessionError {}

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub max_steps: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_steps: 50,
        }
    }
}

/// Session coordinates cognition and runtime
pub struct Session<E> 
where
    E: CognitiveEngine,
{
    /// Cognitive engine (pure logic)
    engine: E,
    
    /// State tracking
    state: AgentState,
    
    /// Runtime for executing decisions
    runtime: AgentRuntime,
    
    /// Configuration
    #[allow(dead_code)]
    config: SessionConfig,
    
    /// Memory manager for long-term storage
    memory_manager: Option<Arc<AgentMemoryManager>>,
    
    /// Session persistence for autosave/resume
    persistence: Option<SessionPersistence>,
    
    /// Session ID
    session_id: String,
    
    /// Chat history for persistence
    history: Vec<crate::agent::cognition::history::Message>,
}

impl<E> Session<E>
where
    E: CognitiveEngine,
{
    /// Create a new session
    /// 
    /// Autosave is enabled by default. Use `from_config()` to respect config settings.
    pub fn new(
        engine: E,
        runtime: AgentRuntime,
        config: SessionConfig,
    ) -> Self {
        let state = AgentState::new(config.max_steps);
        
        Self {
            engine,
            state,
            runtime,
            config,
            memory_manager: None,
            persistence: Some(SessionPersistence::new()),
            session_id: uuid::Uuid::new_v4().to_string(),
            history: Vec::new(),
        }
    }
    
    /// Create a new session from full configuration
    /// 
    /// Respects memory_config.autosave and memory_config.incognito settings
    pub fn from_config(
        engine: E,
        runtime: AgentRuntime,
        session_config: SessionConfig,
        memory_config: &crate::config::agent::MemoryConfig,
    ) -> Self {
        let state = AgentState::new(session_config.max_steps);
        
        Self {
            engine,
            state,
            runtime,
            config: session_config,
            memory_manager: None,
            persistence: Some(SessionPersistence::from_config(memory_config)),
            session_id: uuid::Uuid::new_v4().to_string(),
            history: Vec::new(),
        }
    }
    
    /// Create a new session with memory manager
    pub fn with_memory(
        engine: E,
        runtime: AgentRuntime,
        config: SessionConfig,
        memory_manager: Arc<AgentMemoryManager>,
    ) -> Self {
        let state = AgentState::new(config.max_steps);
        let persistence = SessionPersistence::from_config(memory_manager.config());
        
        Self {
            engine,
            state,
            runtime,
            config,
            memory_manager: Some(memory_manager),
            persistence: Some(persistence),
            session_id: uuid::Uuid::new_v4().to_string(),
            history: Vec::new(),
        }
    }
    
    /// Create a session from a persisted session (resume)
    pub fn from_persisted(
        engine: E,
        runtime: AgentRuntime,
        persisted: PersistedSession,
        memory_manager: Option<Arc<AgentMemoryManager>>,
    ) -> Self {
        let config = persisted.agent_state
            .as_ref()
            .map(|s| s.config.clone())
            .unwrap_or_default();
        
        let state = AgentState::new(config.max_steps);
        
        Self {
            engine,
            state,
            runtime,
            config,
            memory_manager,
            persistence: Some(SessionPersistence::new()),
            session_id: persisted.id,
            history: persisted.history,
        }
    }
    
    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    
    /// Get the memory manager (if configured)
    pub fn memory_manager(&self) -> Option<&Arc<AgentMemoryManager>> {
        self.memory_manager.as_ref()
    }
    
    /// Get mutable reference to memory manager
    pub fn memory_manager_mut(&mut self) -> Option<&mut Arc<AgentMemoryManager>> {
        self.memory_manager.as_mut()
    }
    
    /// Set the memory manager
    pub fn set_memory_manager(&mut self, manager: Arc<AgentMemoryManager>) {
        self.memory_manager = Some(manager);
    }
    
    /// Get the persistence manager
    pub fn persistence(&self) -> Option<&SessionPersistence> {
        self.persistence.as_ref()
    }
    
    /// Disable autosave for this session
    pub fn disable_autosave(&mut self) {
        if let Some(ref mut persistence) = self.persistence {
            persistence.set_autosave(false);
        }
    }
    
    /// Enable autosave for this session (default)
    pub fn enable_autosave(&mut self) {
        if let Some(ref mut persistence) = self.persistence {
            persistence.set_autosave(true);
        }
    }
    
    /// Build a persisted session snapshot
    pub fn build_persisted_session(&self) -> PersistedSession {
        SessionBuilder::new()
            .with_id(&self.session_id)
            .with_history(self.history.clone())
            .build()
    }
    
    /// Save the current session state
    pub async fn save(&self) {
        if let Some(ref persistence) = self.persistence {
            let session = self.build_persisted_session();
            persistence.save(&session).await;
        }
    }
    
    /// Convert SessionInput to InputEvent
    fn translate_input(&self, input: SessionInput) -> Option<InputEvent> {
        match input {
            SessionInput::Chat(msg) => Some(InputEvent::UserMessage(msg)),
            SessionInput::Task { command, args } => {
                Some(InputEvent::UserMessage(format!("Execute: {} {}", command, args.join(" "))))
            }
            SessionInput::Approval(approval) => Some(InputEvent::ApprovalResult(approval)),
            SessionInput::Worker(event) => {
                match event {
                    WorkerEvent::Completed { job_id, result } => {
                        let id = job_id.parse::<u64>().unwrap_or_else(|_| 0);
                        Some(InputEvent::WorkerResult(
                            WorkerId(id),
                            Ok(result)
                        ))
                    }
                    WorkerEvent::Failed { job_id, error } => {
                        let id = job_id.parse::<u64>().unwrap_or_else(|_| 0);
                        Some(InputEvent::WorkerResult(
                            WorkerId(id),
                            Err(crate::agent::cognition::input::WorkerError { message: error })
                        ))
                    }
                    _ => None,
                }
            }
            SessionInput::Interrupt => None, // Handle separately
        }
    }
    
    /// Run session until completion
    pub async fn run(&mut self, mut input_rx: Receiver<SessionInput>) -> Result<String, SessionError> {
        let ctx = RuntimeContext::new();
        let mut last_observation: Option<InputEvent> = None;
        
        loop {
            // Check for external input
            if let Ok(input) = input_rx.try_recv() {
                if matches!(input, SessionInput::Interrupt) {
                    return Err(SessionError::Cancelled);
                }
                last_observation = self.translate_input(input);
            }
            
            // Check cancellation
            if ctx.is_cancelled() {
                return Err(SessionError::Cancelled);
            }
            
            // Cognitive step
            let transition = self.engine.step(&self.state, last_observation.clone())?;
            
            // Update state
            self.state = transition.next_state.clone();
            
            // Sync session history with state history for persistence
            self.history = self.state.history.clone();
            
            // Handle decision
            match transition.decision {
                AgentDecision::Exit(AgentExitReason::Complete) | 
                AgentDecision::Exit(AgentExitReason::UserRequest) => {
                    return Ok("Session completed".to_string());
                }
                AgentDecision::Exit(AgentExitReason::Error(reason)) => {
                    return Ok(format!("Session exited: {}", reason));
                }
                AgentDecision::Exit(AgentExitReason::StepLimit) => {
                    return Ok("Step limit reached".to_string());
                }
                AgentDecision::EmitResponse(response) => {
                    return Ok(response);
                }
                decision => {
                    // Execute through runtime
                    if let Some(event) = self.runtime.interpret(&ctx, decision).await? {
                        last_observation = Some(event);
                    } else {
                        last_observation = None;
                    }
                }
            }
        }
    }
}
