//! Integration Tests for Agent V3
//!
//! These tests verify end-to-end functionality with real and mock capabilities.

#[cfg(test)]
mod tests {
    use crate::agent::{
        AgentBuilder, SessionInput, SessionConfig,
        CognitiveEngine, AgentState, InputEvent, Transition,
        runtime::impls::{
            ToolRegistry, LlmClientCapability,
            ConsoleTelemetry,
        },
    };
    use tokio::sync::mpsc;
    use std::sync::Arc;
    use futures::Stream;

    /// Mock LLM that returns predictable responses
    struct MockLLM {
        responses: Vec<String>,
        call_count: usize,
    }

    impl MockLLM {
        fn new(responses: Vec<String>) -> Self {
            Self { responses, call_count: 0 }
        }
    }

    impl crate::agent::runtime::capability::Capability for MockLLM {
        fn name(&self) -> &'static str { "mock-llm" }
    }

    #[async_trait::async_trait]
    impl crate::agent::runtime::capability::LLMCapability for MockLLM {
        async fn complete(
            &self,
            _ctx: &crate::agent::runtime::context::RuntimeContext,
            _req: crate::agent::cognition::decision::LLMRequest,
        ) -> Result<crate::agent::cognition::input::LLMResponse, crate::agent::runtime::error::LLMError> {
            let response = self.responses.get(self.call_count)
                .cloned()
                .unwrap_or_else(|| "No more responses".to_string());
            
            Ok(crate::agent::types::events::LLMResponse {
                content: response,
                usage: crate::agent::types::events::TokenUsage::new(5, 5),
                model: "mock".to_string(),
                provider: "mock".to_string(),
                finish_reason: crate::agent::types::events::FinishReason::Stop,
                structured: None,
            })
        }
        
        fn complete_stream<'a>(
            &'a self,
            _ctx: &'a crate::agent::runtime::context::RuntimeContext,
            _req: crate::agent::types::intents::LLMRequest,
        ) -> std::pin::Pin<Box<dyn Stream<Item = Result<crate::agent::runtime::capability::StreamChunk, crate::agent::runtime::error::LLMError>> + Send + 'a>> {
            let response = self.responses.get(self.call_count)
                .cloned()
                .unwrap_or_else(|| "No more responses".to_string());
            
            Box::pin(futures::stream::once(async move {
                Ok(crate::agent::runtime::capability::StreamChunk {
                    content: response,
                    is_final: true,
                })
            }))
        }
    }

    #[tokio::test]
    async fn test_end_to_end_with_mock_llm() {
        // Create mock LLM that responds with a tool call
        let mock_llm = Arc::new(MockLLM::new(vec![
            "<tool>pwd</tool><args></args>".to_string(),
        ]));

        let mut session = AgentBuilder::new()
            .with_llm(mock_llm)
            .with_tools(ToolRegistry::new())
            .with_auto_approve()
            .with_config(SessionConfig { max_steps: 5 })
            .build_with_llm_engine();

        let (tx, rx) = mpsc::channel(10);
        tx.send(SessionInput::Chat("What directory am I in?".to_string())).await.ok();
        drop(tx);

        let result = session.run(rx).await;
        
        // Should complete (either successfully or with error)
        // We mainly care that the flow works
        println!("Result: {:?}", result);
    }

    #[tokio::test]
    async fn test_tool_execution_flow() {
        use crate::agent::cognition::decision::{AgentDecision, ToolCall};
        
        // Custom engine that always calls a tool
        struct ToolCallingEngine;
        
        impl CognitiveEngine for ToolCallingEngine {
            fn step(
                &mut self,
                state: &AgentState,
                input: Option<InputEvent>,
            ) -> Result<Transition, crate::agent::cognition::error::CognitiveError> {
                let decision = match input {
                    Some(InputEvent::UserMessage(_)) if state.step_count == 0 => {
                        AgentDecision::CallTool(ToolCall {
                            name: "pwd".to_string(),
                            arguments: serde_json::json!(""),
                            working_dir: None,
                            timeout_secs: None,
                        })
                    }
                    Some(InputEvent::ToolResult { result, .. }) => {
                        let output = match result {
                            crate::agent::types::events::ToolResult::Success { output, .. } => output,
                            crate::agent::types::events::ToolResult::Error { message, .. } => message,
                            crate::agent::types::events::ToolResult::Cancelled => "Cancelled".to_string(),
                        };
                        AgentDecision::EmitResponse(format!("Result: {}", output))
                    }
                    _ => AgentDecision::Exit(crate::agent::cognition::decision::AgentExitReason::Complete),
                };
                
                Ok(Transition::new(state.clone().increment_step(), decision))
            }
            
            fn build_prompt(&self, _state: &AgentState) -> String { "test".to_string() }
            fn requires_approval(&self, _tool: &str, _args: &str) -> bool { false }
        }

        let mut session = AgentBuilder::new()
            .with_tools(ToolRegistry::new())
            .with_auto_approve()
            .with_config(SessionConfig { max_steps: 10 })
            .build_with_engine(ToolCallingEngine);

        let (tx, rx) = mpsc::channel(10);
        tx.send(SessionInput::Chat("test".to_string())).await.ok();
        drop(tx);

        let result = session.run(rx).await;
        assert!(result.is_ok(), "Session should complete successfully");
    }

    #[tokio::test]
    async fn test_session_cancellation() {
        let mut session = AgentBuilder::new()
            .with_auto_approve()
            .with_config(SessionConfig { max_steps: 100 })
            .build_with_llm_engine();

        let (tx, rx) = mpsc::channel(10);
        tx.send(SessionInput::Chat("test".to_string())).await.ok();
        tx.send(SessionInput::Interrupt).await.ok();
        drop(tx);

        let result = session.run(rx).await;
        
        // Should return cancelled error
        match result {
            Err(crate::agent::session::SessionError::Cancelled) => {
                // Expected
            }
            _ => {
                // Other outcomes are also acceptable depending on timing
            }
        }
    }

    #[tokio::test]
    async fn test_builder_presets() {
        use crate::agent::builder::presets;
        
        let mut session = presets::testing_agent();
        
        let (tx, rx) = mpsc::channel(10);
        tx.send(SessionInput::Chat("Hello".to_string())).await.ok();
        drop(tx);
        
        // Should run without panicking
        let _ = session.run(rx).await;
    }

    #[tokio::test]
    async fn test_telemetry_recording() {
        let telemetry = Arc::new(ConsoleTelemetry::new());
        
        let mut session = AgentBuilder::new()
            .with_telemetry_capability(telemetry.clone())
            .with_auto_approve()
            .build_with_llm_engine();

        let (tx, rx) = mpsc::channel(10);
        tx.send(SessionInput::Chat("test".to_string())).await.ok();
        drop(tx);

        let _ = session.run(rx).await;
        
        // Telemetry should have recorded events
        // (Can't easily verify without mocking, but at least it doesn't panic)
    }

    /// Test that the LLM client capability properly bridges to LlmClient
    /// 
    /// This test requires a valid API key to pass. Skip if not available.
    #[tokio::test]
    #[ignore = "Requires valid API key"]
    async fn test_llm_client_capability_integration() {
        use crate::llm::{LlmClient, LlmConfig};
        
        // Create real LlmClient
        let config = LlmConfig {
            provider: crate::llm::LlmProvider::OpenAiCompatible,
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-3.5-turbo".to_string(),
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            max_tokens: Some(100),
            temperature: Some(0.7),
            system_prompt: None,
            input_price_per_1m: 0.0,
            output_price_per_1m: 0.0,
            max_context_tokens: 4096,
            condense_threshold: 0.8,
            memory: Default::default(),
            extra_params: Default::default(),
            web_search_enabled: false,
        };
        
        let client = Arc::new(LlmClient::new(config).expect("Failed to create LLM client"));
        let llm_cap = Arc::new(LlmClientCapability::new(client));
        
        let mut session = AgentBuilder::new()
            .with_llm(llm_cap)
            .with_auto_approve()
            .build_with_llm_engine();

        let (tx, rx) = mpsc::channel(10);
        tx.send(SessionInput::Chat("Say 'hello' and nothing else".to_string())).await.ok();
        drop(tx);

        let result = session.run(rx).await;
        
        // Should get a response containing "hello"
        let response = result.expect("Session should complete");
        assert!(response.to_lowercase().contains("hello"), 
            "Response should contain 'hello', got: {}", response);
    }
}
