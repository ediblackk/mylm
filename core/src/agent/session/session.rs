//! Session orchestrator
//!
//! Coordinates cognition and runtime layers.
//! Receives SessionInput, produces cognition steps, interprets decisions.

use crate::agent::cognition::engine::CognitiveEngine;
use crate::agent::cognition::input::InputEvent;
use crate::agent::cognition::state::AgentState;
use crate::agent::types::events::WorkerId;
use crate::agent::cognition::decision::{AgentDecision, AgentExitReason};
use crate::agent::cognition::error::CognitiveError;
use crate::agent::runtime::{AgentRuntime, RuntimeContext, RuntimeError};
use crate::agent::session::input::{SessionInput, WorkerEvent};

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
}

impl<E> Session<E>
where
    E: CognitiveEngine,
{
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
