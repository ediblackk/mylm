# Cognition Module

**Purpose:** Pure cognitive logic. No async, no IO, no external dependencies.

This is the "brain" of the agent - it decides what to do next based on state and input.

## Files

| File | Purpose | Key Items |
|------|---------|-----------|
| `mod.rs` | Module exports | Re-exports all cognition types |
| `kernel.rs` | Core trait + state | `GraphEngine`, `AgentState`, `Message` |
| `planner.rs` | Decision planner | `Planner` implements `GraphEngine` |
| `engine.rs` | Step engine trait | `StepEngine`, `StubEngine` |
| `input.rs` | Input events | `InputEvent`, `ToolResult`, `LLMResponse` |
| `decision.rs` | Decisions | `AgentDecision`, `Transition`, `ToolCall` |
| `error.rs` | Error types | `CognitiveError` |
| `step/mod.rs` | Step engine module | `StepEngine` implementations |
| `step/llm_engine.rs` | LLM-based engine | `LlmEngine` |
| `prompts/mod.rs` | Prompts module | System prompt builders |
| `prompts/system.rs` | System prompts | `build_system_prompt()`, tool defs |
| `policy/mod.rs` | Policy module | Approval policies |
| `policy/approval.rs` | Approval logic | `requires_approval()` |

## Core Abstraction

```rust
/// Pure cognitive step: (state, input) -> Transition
pub trait StepEngine {
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
StepEngine::step(state, input)
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

1. **No async** - Can't use `.await`, can't spawn tasks
2. **No IO** - Can't read files, make network calls, etc.
3. **No external crates** - Only std library (except serde)
4. **Pure functions** - Same input → same output, no side effects
5. **Immutable state** - Never mutate, always create new state

## Dependencies

- Only uses `crate::agent::types` (primitive types)
- NO dependency on runtime/ or session/
