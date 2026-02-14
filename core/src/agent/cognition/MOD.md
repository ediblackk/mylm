# Cognition Module

**Purpose**: Pure cognitive logic. No async, no IO, no external dependencies.

This is the "brain" of the agent - it decides what to do next based on state and input.

## Files

| File | Purpose | Key Items |
|------|---------|-----------|
| `state.rs` | Agent state | `AgentState`, `WorkerId` |
| `input.rs` | External events | `InputEvent`, `ToolResult`, `LLMResponse` |
| `decision.rs` | Intent/decisions | `AgentDecision`, `Transition`, `ToolCall` |
| `engine.rs` | Core trait | `CognitiveEngine` trait, `StubEngine` |
| `llm_engine.rs` | LLM-based engine | `LLMBasedEngine`, `ResponseParser` |
| `error.rs` | Error types | `CognitiveError` |
| `history.rs` | Message history | `Message`, `MessageRole` |

## Core Abstraction

```rust
/// Pure cognitive step: (state, input) -> Transition
pub trait CognitiveEngine {
    fn step(
        &mut self,
        state: &AgentState,
        input: Option<InputEvent>,
    ) -> Result<Transition, CognitiveError>;
}
```

## State Flow

```
AgentState (immutable)
    ↓
CognitiveEngine::step(state, input)
    ↓
Transition { next_state, decision }
    ↓
Session updates state
    ↓
Runtime interprets decision
    ↓
New InputEvent
    ↓
[Repeat]
```

## Rules

1. **No async**: Can't use `.await`, can't spawn tasks
2. **No IO**: Can't read files, make network calls, etc.
3. **No external crates**: Only std library (except serde for serialization)
4. **Pure functions**: Same input → same output, no side effects
5. **Immutable state**: Never mutate, always create new state

## Implementing a Custom Engine

```rust
use mylm_core::agent_v3::cognition::*;

pub struct MyEngine;

impl CognitiveEngine for MyEngine {
    fn step(&mut self, state: &AgentState, input: Option<InputEvent>) 
        -> Result<Transition, CognitiveError> 
    {
        // Your logic here
        let decision = match input {
            Some(InputEvent::UserMessage(msg)) => {
                AgentDecision::EmitResponse(format!("Echo: {}", msg))
            }
            _ => AgentDecision::None,
        };
        
        Ok(Transition::new(
            state.clone().increment_step(),
            decision
        ))
    }
    
    fn build_prompt(&self, state: &AgentState) -> String {
        "My prompt".to_string()
    }
    
    fn requires_approval(&self, tool: &str, args: &str) -> bool {
        tool == "shell" // Require approval for dangerous tools
    }
}
```

## Testing

Cognition is easy to test because it's pure:

```rust
#[test]
fn test_my_engine() {
    let mut engine = MyEngine;
    let state = AgentState::new(10);
    
    let transition = engine.step(
        &state,
        Some(InputEvent::UserMessage("hello".to_string()))
    ).unwrap();
    
    assert!(matches!(transition.decision, 
        AgentDecision::EmitResponse(_)));
}
```
