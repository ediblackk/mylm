//! Agent runtime
//!
//! Interprets AgentDecision using CapabilityGraph.
//! No decision logic. Pure intent dispatch.

use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::graph::CapabilityGraph;
use crate::agent::runtime::error::RuntimeError;
use crate::agent::cognition::{AgentDecision, InputEvent};
/// Agent runtime - intent interpreter
#[derive(Clone)]
pub struct AgentRuntime {
    graph: CapabilityGraph,
}

impl AgentRuntime {
    pub fn new(graph: CapabilityGraph) -> Self {
        Self { graph }
    }
    
    /// Interpret AgentDecision, optionally producing InputEvent
    ///
    /// This is the core runtime loop:
    /// 1. Record telemetry
    /// 2. Dispatch to capability
    /// 3. Record result
    /// 4. Return event (if any)
    pub async fn interpret(
        &self,
        ctx: &RuntimeContext,
        decision: AgentDecision,
    ) -> Result<Option<InputEvent>, RuntimeError> {
        // Record decision telemetry
        self.graph.telemetry.record_decision(ctx, &decision).await;
        
        // Check cancellation before execution
        if ctx.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        
        // Dispatch decision to appropriate capability
        let result = match decision {
            AgentDecision::CallTool(call) => {
                let tool_result = self.graph.tools.execute(ctx, call).await?;
                let event = InputEvent::ToolResult(tool_result);
                self.graph.telemetry.record_result(ctx, &event).await;
                Ok(Some(event))
            }
            
            AgentDecision::RequestLLM(req) => {
                let llm_response = self.graph.llm.complete(ctx, req).await?;
                let event = InputEvent::LLMResponse(llm_response);
                self.graph.telemetry.record_result(ctx, &event).await;
                Ok(Some(event))
            }
            
            AgentDecision::RequestApproval(req) => {
                let approval = self.graph.approval.request(ctx, req).await?;
                let event = InputEvent::ApprovalResult(approval);
                self.graph.telemetry.record_result(ctx, &event).await;
                Ok(Some(event))
            }
            
            AgentDecision::SpawnWorker(spec) => {
                let _handle = self.graph.workers.spawn(ctx, spec).await?;
                // Worker spawn returns ID but no immediate event
                // Worker completion will come asynchronously
                Ok(None)
            }
            
            AgentDecision::EmitResponse(_response) => {
                // Response emission is handled by session/presentation layer
                // No runtime capability needed
                Ok(None)
            }
            
            AgentDecision::Exit(_reason) => {
                // Exit is handled by session termination
                // No runtime capability needed
                Ok(None)
            }
            
            AgentDecision::None => {
                // No operation
                Ok(None)
            }
        };
        
        result
    }
}
