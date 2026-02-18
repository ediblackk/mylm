//! Architecture verification tests
//! 
//! These tests verify the layered architecture supports:
//! - Mock runtimes for testing
//! - Pure cognition without IO
//! - Session orchestration

#[cfg(test)]
mod tests {
    use crate::agent::{
        AgentState, AgentDecision, InputEvent, Transition, WorkerId,
        CognitiveEngine, CognitiveError,
        AgentRuntime,
        Session, SessionConfig, SessionInput,
    };
    use tokio::sync::mpsc;

    /// A mock cognitive engine for testing
    struct MockEngine {
        responses: Vec<AgentDecision>,
        call_count: usize,
    }

    impl MockEngine {
        fn new(responses: Vec<AgentDecision>) -> Self {
            Self { responses, call_count: 0 }
        }
    }

    impl CognitiveEngine for MockEngine {
        fn step(
            &mut self,
            state: &AgentState,
            _input: Option<InputEvent>,
        ) -> Result<Transition, CognitiveError> {
            let decision = self.responses.get(self.call_count)
                .cloned()
                .unwrap_or(AgentDecision::Exit(crate::agent::cognition::decision::AgentExitReason::Complete));
            self.call_count += 1;
            
            let next_state = state.clone().increment_step();
            Ok(Transition::new(next_state, decision))
        }
        
        fn build_prompt(&self, _state: &AgentState) -> String {
            "test prompt".to_string()
        }
        
        fn requires_approval(&self, _tool: &str, _args: &str) -> bool {
            false
        }
    }

    #[test]
    fn test_cognitive_engine_pure_function() {
        // Arrange - cognition should work without any async runtime
        let engine = MockEngine::new(vec![
            AgentDecision::EmitResponse("Hello".to_string()),
        ]);
        let state = AgentState::new(10);
        
        // Act - pure function call, no async
        let mut engine = engine;
        let result = engine.step(&state, Some(InputEvent::UserMessage("Hi".to_string())));
        
        // Assert
        assert!(result.is_ok());
        let transition = result.unwrap();
        assert_eq!(transition.next_state.step_count, 1);
        assert!(matches!(transition.decision, AgentDecision::EmitResponse(ref s) if s == "Hello"));
    }

    #[test]
    fn test_agent_state_is_pure() {
        // State should be cloneable, serializable conceptually
        let state1 = AgentState::new(100);
        let state2 = state1.clone();
        
        // Mutation should produce new state
        let state3 = state1.clone().increment_step();
        
        assert_eq!(state1.step_count, 0);
        assert_eq!(state2.step_count, 0);
        assert_eq!(state3.step_count, 1);
    }

    #[tokio::test]
    async fn test_session_event_loop() {
        // This verifies the SessionInput -> Engine -> Decision -> Runtime -> InputEvent loop
        use crate::agent::runtime::CapabilityGraph;
        
        // Create minimal capability graph (would use mocks in real test)
        let graph = CapabilityGraph::stub();
        let runtime = AgentRuntime::new(graph);
        
        let engine = MockEngine::new(vec![
            AgentDecision::EmitResponse("Test response".to_string()),
        ]);
        
        let (tx, rx) = mpsc::channel(10);
        let session = Session::new(engine, runtime, SessionConfig::default());
        
        // Send input
        tx.send(SessionInput::Chat("Hello".to_string())).await.ok();
        drop(tx); // Close channel
        
        // Run session
        let mut session = session;
        let result = session.run(rx).await;
        
        // Should complete successfully
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Test response");
    }

    #[test]
    fn test_worker_id_is_primitive() {
        // WorkerId should be a simple primitive type (u64)
        let id1 = WorkerId(123);
        let id2 = WorkerId(123);
        let id3 = WorkerId(456);
        
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
}
