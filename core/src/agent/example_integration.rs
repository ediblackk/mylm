//! Integration Example: Real Capability Graph
//!
//! Demonstrates how to wire real capabilities together without touching cognition.
//! 
//! Usage:
//! ```rust
//! let runtime = create_production_runtime(llm_client).await;
//! let session = Session::new(engine, runtime, config);
//! ```

#[cfg(test)]
mod example {
    use std::sync::Arc;
    use crate::agent::{
        AgentState, AgentDecision, Transition, InputEvent,
        CognitiveEngine, CognitiveError,
        AgentRuntime,
        CapabilityGraph,
        Session, SessionConfig, SessionInput,
    };
    use crate::agent::cognition::decision::{ToolCall, AgentExitReason, ApprovalRequest};
    use crate::agent::cognition::input::ApprovalOutcome;
    use crate::agent::runtime::impls::SimpleToolExecutor;
    use crate::agent::runtime::capability::ApprovalCapability;
    use crate::agent::runtime::error::ApprovalError;
    use tokio::sync::mpsc;
    use futures::Stream;

    /// Example: Production runtime with real LLM and terminal approval
    #[allow(dead_code)]
    fn create_production_runtime() -> AgentRuntime {
        // In real usage:
        // let llm_client = Arc::new(LlmClient::new(config).unwrap());
        // let llm_cap = Arc::new(LlmClientCapability::new(llm_client));
        
        // For this example, we use stubs
        let graph = CapabilityGraph::stub();
        AgentRuntime::new(graph)
    }

    /// Example: Testing runtime with auto-approval
    #[allow(dead_code)]
    fn create_test_runtime() -> AgentRuntime {
        use crate::agent::runtime::capability::{
            LLMCapability, Capability,
        };
        use crate::agent::runtime::context::RuntimeContext;
        use crate::agent::runtime::error::LLMError;
        use crate::agent::cognition::decision::LLMRequest;
        use crate::agent::cognition::input::LLMResponse;

        // Mock LLM that returns canned responses
        struct MockLLM;
        impl Capability for MockLLM {
            fn name(&self) -> &'static str { "mock-llm" }
        }
        #[async_trait::async_trait]
        impl LLMCapability for MockLLM {
            async fn complete(&self, _ctx: &RuntimeContext, _req: LLMRequest) -> Result<LLMResponse, LLMError> {
                Ok(LLMResponse { 
                    content: "Mock response".to_string(), 
                    usage: crate::agent::types::events::TokenUsage::new(5, 5),
                    model: "mock".to_string(),
                    provider: "mock".to_string(),
                    finish_reason: crate::agent::types::events::FinishReason::Stop,
                    structured: None,
                })
            }
            
            fn complete_stream<'a>(
                &'a self,
                _ctx: &'a RuntimeContext,
                _req: LLMRequest,
            ) -> std::pin::Pin<Box<dyn Stream<Item = Result<crate::agent::runtime::capability::StreamChunk, LLMError>> + Send + 'a>> {
                Box::pin(futures::stream::once(async {
                    Ok(crate::agent::runtime::capability::StreamChunk {
                        content: "Mock response".to_string(),
                        is_final: true,
                    })
                }))
            }
        }

        // Auto-approve all tools
        struct MockApproval;
        impl Capability for MockApproval {
            fn name(&self) -> &'static str { "mock-approval" }
        }
        #[async_trait::async_trait]
        impl ApprovalCapability for MockApproval {
            async fn request(&self, _ctx: &RuntimeContext, _req: ApprovalRequest) -> Result<ApprovalOutcome, ApprovalError> {
                Ok(ApprovalOutcome::Granted)
            }
        }

        let graph = CapabilityGraph::new(
            Arc::new(MockLLM),
            Arc::new(SimpleToolExecutor::new()),
            Arc::new(MockApproval),
            Arc::new(crate::agent::runtime::graph::StubWorkers),
            Arc::new(crate::agent::runtime::graph::StubTelemetry),
        );
        
        AgentRuntime::new(graph)
    }

    /// Example: Engine that calls a tool
    struct ToolCallingEngine;
    
    impl CognitiveEngine for ToolCallingEngine {
        fn step(
            &mut self,
            state: &AgentState,
            input: Option<InputEvent>,
        ) -> Result<Transition, CognitiveError> {
            match input {
                // Initial request - call a tool
                Some(InputEvent::UserMessage(msg)) if state.step_count == 0 => {
                    let decision = AgentDecision::CallTool(ToolCall {
                        name: "pwd".to_string(),
                        arguments: serde_json::json!(""),
                        working_dir: None,
                        timeout_secs: None,
                    });
                    let next_state = state.clone().increment_step();
                    Ok(Transition::new(next_state, decision))
                }
                
                // Tool result - emit response
                Some(InputEvent::ToolResult(result)) => {
                    let output = match result {
                        crate::agent::types::events::ToolResult::Success { output, .. } => output.clone(),
                        crate::agent::types::events::ToolResult::Error { message, .. } => message.clone(),
                        crate::agent::types::events::ToolResult::Cancelled => "Cancelled".to_string(),
                    };
                    let decision = AgentDecision::EmitResponse(
                        format!("Tool result: {}", output)
                    );
                    let next_state = state.clone().increment_step();
                    Ok(Transition::new(next_state, decision))
                }
                
                // Default - exit
                _ => {
                    Ok(Transition::exit(
                        state.clone(),
                        AgentExitReason::Complete
                    ))
                }
            }
        }
        
        fn build_prompt(&self, _state: &AgentState) -> String {
            "test".to_string()
        }
        
        fn requires_approval(&self, _tool: &str, _args: &str) -> bool {
            false // Auto-approve for this example
        }
    }

    #[tokio::test]
    async fn test_tool_execution_flow() {
        // Arrange
        let engine = ToolCallingEngine;
        let runtime = create_test_runtime();
        let (tx, rx) = mpsc::channel(10);
        
        // Act
        tx.send(SessionInput::Chat("What is my directory?".to_string())).await.ok();
        drop(tx);
        
        let mut session = Session::new(engine, runtime, SessionConfig::default());
        let result = session.run(rx).await;
        
        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.contains("Tool result:"));
        println!("✓ Tool execution flow completed: {}", response);
    }

    #[tokio::test]
    async fn test_capability_swapping() {
        // This test proves we can swap capabilities without touching cognition
        
        // Test 1: Stub runtime (fast, deterministic)
        let stub_runtime = AgentRuntime::new(CapabilityGraph::stub());
        
        // Test 2: Test runtime (mock LLM, auto-approval)
        let test_runtime = create_test_runtime();
        
        // Both work with the same engine!
        let engine = ToolCallingEngine;
        
        let (tx1, rx1) = mpsc::channel(10);
        tx1.send(SessionInput::Chat("test".to_string())).await.ok();
        drop(tx1);
        
        let mut session1 = Session::new(engine, stub_runtime, SessionConfig::default());
        let result1 = session1.run(rx1).await;
        assert!(result1.is_ok());
        
        let (tx2, rx2) = mpsc::channel(10);
        tx2.send(SessionInput::Chat("test".to_string())).await.ok();
        drop(tx2);
        
        let mut session2 = Session::new(ToolCallingEngine, test_runtime, SessionConfig::default());
        let result2 = session2.run(rx2).await;
        assert!(result2.is_ok());
        
        println!("✓ Capability swapping verified - same engine, different runtimes");
    }
}
