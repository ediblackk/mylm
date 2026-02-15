//! Adapter: CognitiveEngine -> AgencyKernel
//!
//! Bridges the existing cognition module to the new contract.
//! This allows gradual migration without breaking existing code.

use crate::agent::contract::{
    AgencyKernel, KernelConfig,
    kernel::{AgentState as ContractAgentState, KernelError},
    events::{KernelEvent, ToolResult},
    graph::IntentGraph,
    intents::{Intent, ExitReason, WorkerSpec, Role, Message as ContractMessage},
    ids::IntentId,
};

use crate::agent::cognition::{
    CognitiveEngine, AgentState, InputEvent, AgentDecision, AgentExitReason,
};

/// Adapter that wraps a CognitiveEngine to implement AgencyKernel
///
/// This bridges the old single-step API to the new batch-process API.
pub struct CognitiveEngineAdapter<E: CognitiveEngine> {
    engine: E,
    state: AgentState,
    contract_state: ContractAgentState,
}

impl<E: CognitiveEngine> CognitiveEngineAdapter<E> {
    /// Create a new adapter with the given engine
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            state: AgentState::new(50),
            contract_state: ContractAgentState::new(),
        }
    }

    /// Convert contract KernelEvent to cognition InputEvent
    fn convert_event(&self, event: &KernelEvent) -> Option<InputEvent> {
        // Log event for debugging
        if matches!(event, KernelEvent::RuntimeError { .. }) {
            crate::info_log!("[KERNEL_ADAPTER] Converting event: {:?}", event);
        }

        match event {
            KernelEvent::UserMessage { content } => {
                Some(InputEvent::UserMessage(content.clone()))
            }
            KernelEvent::ToolCompleted { tool, result, .. } => {
                // Convert types::events::ToolResult to cognition InputEvent::ToolResult
                let tool_result = match result {
                    ToolResult::Success { output, .. } => {
                        crate::agent::types::events::ToolResult::Success {
                            output: output.clone(),
                            structured: None,
                        }
                    }
                    ToolResult::Error { message, .. } => {
                        crate::agent::types::events::ToolResult::Error {
                            message: message.clone(),
                            code: None,
                            retryable: false,
                        }
                    }
                    ToolResult::Cancelled => crate::agent::types::events::ToolResult::Cancelled,
                };
                Some(InputEvent::ToolResult {
                    tool: tool.clone(),
                    result: tool_result,
                })
            }
            KernelEvent::LLMCompleted { response, .. } => {
                Some(InputEvent::LLMResponse(crate::agent::types::events::LLMResponse {
                    content: response.content.clone(),
                    usage: response.usage,
                    model: response.model.clone(),
                    provider: response.provider.clone(),
                    finish_reason: response.finish_reason.clone(),
                    structured: response.structured.clone(),
                }))
            }
            KernelEvent::ApprovalGiven { outcome, .. } => {
                Some(InputEvent::ApprovalResult(outcome.clone()))
            }
            KernelEvent::WorkerCompleted { worker_id, result } => {
                Some(InputEvent::WorkerResult(
                    crate::agent::types::events::WorkerId(worker_id.0),
                    result.as_ref().map(|s| s.clone()).map_err(|e| 
                        crate::agent::cognition::input::WorkerError {
                            message: e.message.clone(),
                        }
                    ),
                ))
            }
            KernelEvent::WorkerFailed { worker_id, error, .. } => {
                Some(InputEvent::WorkerResult(
                    crate::agent::types::events::WorkerId(worker_id.0),
                    Err(crate::agent::cognition::input::WorkerError {
                        message: error.clone(),
                    }),
                ))
            }
            KernelEvent::Interrupt => None,
            KernelEvent::Tick { .. } => None,
            KernelEvent::Session { .. } => None,
            KernelEvent::RuntimeError { intent_id, error } => {
                crate::info_log!("[KERNEL_ADAPTER] RuntimeError event: intent_id={:?}, error={}", intent_id, error);
                Some(InputEvent::RuntimeError {
                    intent_id: *intent_id,
                    error: error.clone(),
                })
            }
        }
    }

    /// Convert AgentDecision to Intent
    fn convert_decision(&self, decision: AgentDecision, _intent_id: IntentId) -> Intent {
        match decision {
            AgentDecision::CallTool(call) => {
                Intent::CallTool(call) // Already the unified type
            }
            AgentDecision::RequestLLM(req) => {
                Intent::RequestLLM(req) // Already the unified type
            }
            AgentDecision::RequestApproval(req) => {
                Intent::RequestApproval(crate::agent::contract::intents::ApprovalRequest {
                    tool: req.tool,
                    args: req.args,
                    reason: req.reason,
                })
            }
            AgentDecision::SpawnWorker(spec) => {
                Intent::SpawnWorker(WorkerSpec {
                    objective: spec.objective,
                    context: String::new(),
                    max_iterations: None,
                    can_delegate: false,
                    allowed_tools: None,
                    model: None,
                })
            }
            AgentDecision::EmitResponse(text) => {
                Intent::EmitResponse(text)
            }
            AgentDecision::Exit(reason) => {
                crate::info_log!("[KERNEL_ADAPTER] Exit decision: {:?}", reason);
                Intent::Halt(match reason {
                    AgentExitReason::Complete => ExitReason::Completed,
                    AgentExitReason::StepLimit => ExitReason::StepLimit,
                    AgentExitReason::UserRequest => ExitReason::UserRequest,
                    AgentExitReason::Error(msg) => ExitReason::Error(msg),
                })
            }
            AgentDecision::None => {
                Intent::Halt(ExitReason::Completed)
            }
        }
    }
}

impl<E: CognitiveEngine> AgencyKernel for CognitiveEngineAdapter<E> {
    fn init(&mut self, config: KernelConfig) -> Result<(), KernelError> {
        self.state = AgentState::new(config.max_steps);
        self.contract_state = ContractAgentState::new();
        self.contract_state.max_steps = config.max_steps;
        Ok(())
    }

    fn process(&mut self, events: &[KernelEvent]) -> Result<IntentGraph, KernelError> {
        let mut builder = IntentGraphBuilder::at_step(self.state.step_count as u32);
        let mut intent_index = 0;

        // Process each event through the engine
        for event in events {
            // Update contract state with event
            match event {
                KernelEvent::UserMessage { content } => {
                    self.contract_state.history.push(ContractMessage {
                        role: Role::User,
                        content: content.clone(),
                    });
                }
                KernelEvent::LLMCompleted { response, .. } => {
                    // Add LLM response to history
                    self.contract_state.history.push(ContractMessage {
                        role: Role::Assistant,
                        content: response.content.clone(),
                    });
                    crate::info_log!("[KERNEL_ADAPTER] Added LLM response to history: {}", &response.content.chars().take(100).collect::<String>());
                }
                _ => {}
            }
            
            if let Some(input) = self.convert_event(event) {
                crate::info_log!("[KERNEL_ADAPTER] Processing input: {:?}", input);
                let transition = self.engine.step(&self.state, Some(input))
                    .map_err(|e| KernelError::Internal(e.to_string()))?;
                
                crate::info_log!("[KERNEL_ADAPTER] Engine decision: {:?}", transition.decision);
                
                // Update state
                self.state = transition.next_state;
                
                // Sync contract state
                self.contract_state.step_count = self.state.step_count;
                
                // Convert decision to intent and add to graph
                if !matches!(transition.decision, AgentDecision::None) {
                    let intent_id = IntentId::from_step(self.state.step_count as u32, intent_index);
                    let intent = self.convert_decision(transition.decision, intent_id);
                    crate::info_log!("[KERNEL_ADAPTER] Adding intent: {:?}", intent);
                    builder.add_with_id(intent_id, intent);
                    intent_index += 1;
                }
            }
        }

        // If no events or no decisions, check if we should emit something
        if intent_index == 0 && !events.is_empty() {
            // Process one more time with no input to check for pending actions
            let transition = self.engine.step(&self.state, None)
                .map_err(|e| KernelError::Internal(e.to_string()))?;
            
            self.state = transition.next_state;
            self.contract_state.step_count = self.state.step_count;
            
            if !matches!(transition.decision, AgentDecision::None) {
                let intent_id = IntentId::from_step(self.state.step_count as u32, intent_index);
                let intent = self.convert_decision(transition.decision, intent_id);
                builder.add_with_id(intent_id, intent);
            }
        }

        Ok(builder.build())
    }

    fn state(&self) -> &ContractAgentState {
        &self.contract_state
    }

    fn is_terminal(&self) -> bool {
        !self.state.can_continue() || self.state.shutdown_requested
    }
}

use crate::agent::contract::graph::IntentGraphBuilder;

/// Creates a kernel from an existing CognitiveEngine
pub fn kernel_from_engine<E: CognitiveEngine>(engine: E) -> CognitiveEngineAdapter<E> {
    CognitiveEngineAdapter::new(engine)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::cognition::StubEngine;

    #[test]
    fn test_adapter_basic() {
        let engine = StubEngine::new();
        let mut kernel = kernel_from_engine(engine);
        
        kernel.init(KernelConfig::default()).unwrap();
        
        let events = vec![KernelEvent::UserMessage {
            content: "hello".to_string(),
        }];
        
        let graph = kernel.process(&events).unwrap();
        assert!(!graph.is_empty());
    }
}
