//! Cognitive engine trait
//! 
//! The core abstraction: (state, input) -> Transition
//! 
//! No async. No IO. Pure state evolution.

use crate::agent::cognition::state::AgentState;
use crate::agent::cognition::input::InputEvent;
use crate::agent::cognition::decision::Transition;
use crate::agent::cognition::error::CognitiveError;

/// Pure cognitive engine
/// 
/// Implementors produce intent (AgentDecision), never execute.
pub trait CognitiveEngine {
    /// Single cognitive step
    /// 
    /// # Arguments
    /// * `state` - Current agent state snapshot
    /// * `input` - Optional external input
    /// 
    /// # Returns
    /// Transition containing next state and decision
    fn step(
        &mut self,
        state: &AgentState,
        input: Option<InputEvent>,
    ) -> Result<Transition, CognitiveError>;
    
    /// Build LLM prompt from state
    /// 
    /// Used when engine emits RequestLLM decision.
    fn build_prompt(&self, state: &AgentState) -> String;
    
    /// Check if tool requires approval
    fn requires_approval(&self, tool: &str, args: &str) -> bool;
}

/// Stub engine for testing
#[derive(Debug, Default)]
pub struct StubEngine;

impl StubEngine {
    pub fn new() -> Self {
        Self
    }
}

impl CognitiveEngine for StubEngine {
    fn step(
        &mut self,
        state: &AgentState,
        input: Option<InputEvent>,
    ) -> Result<Transition, CognitiveError> {
        use crate::agent::cognition::decision::AgentDecision;
        
        // Simple stub: echo user message or exit
        let decision = if let Some(InputEvent::UserMessage(msg)) = input {
            AgentDecision::EmitResponse(format!("Echo: {}", msg))
        } else if state.at_limit() {
            AgentDecision::Exit(crate::agent::cognition::decision::AgentExitReason::StepLimit)
        } else {
            AgentDecision::None
        };
        
        let next_state = state.clone().increment_step();
        Ok(Transition::new(next_state, decision))
    }
    
    fn build_prompt(&self, _state: &AgentState) -> String {
        String::from("Stub prompt")
    }
    
    fn requires_approval(&self, _tool: &str, _args: &str) -> bool {
        false
    }
}
