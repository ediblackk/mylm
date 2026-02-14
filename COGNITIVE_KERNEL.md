# Cognitive Kernel

## Overview

Pure, deterministic, side-effect-free state machine.

```
(state, input) -> Transition
```

## Files

| File | Purpose | Lines |
|------|---------|-------|
| `cognition/mod.rs` | Module exports | 18 |
| `cognition/state.rs` | AgentState | 130 |
| `cognition/input.rs` | InputEvent | 60 |
| `cognition/decision.rs` | AgentDecision, Transition | 110 |
| `cognition/engine.rs` | CognitiveEngine trait | 75 |
| `cognition/error.rs` | CognitiveError | 40 |
| `cognition/history.rs` | Message, MessageRole | 50 |
| **Total** | | **483** |

## Core Types

### AgentState

```rust
pub struct AgentState {
    pub history: Vec<Message>,
    pub step_count: usize,
    pub max_steps: usize,
    pub delegation_count: usize,
    pub max_delegations: usize,
    pub rejection_count: usize,
    pub max_rejections: usize,
    pub scratchpad: String,
    pub shutdown_requested: bool,
    pub pending_llm: bool,
    pub pending_approval: bool,
}
```

All fields public. Immutable updates via `with_*` methods.

### InputEvent

```rust
pub enum InputEvent {
    UserMessage(String),
    WorkerResult(WorkerId, Result<String, WorkerError>),
    ToolResult(ToolResult),
    ApprovalResult(ApprovalOutcome),
    LLMResponse(LLMResponse),
    Shutdown,
    Tick,
}
```

### AgentDecision

```rust
pub enum AgentDecision {
    CallTool(ToolCall),
    SpawnWorker(WorkerSpec),
    RequestApproval(ApprovalRequest),
    RequestLLM(LLMRequest),
    EmitResponse(String),
    Exit(AgentExitReason),
    None,
}
```

### CognitiveEngine

```rust
pub trait CognitiveEngine {
    fn step(
        &mut self,
        state: &AgentState,
        input: Option<InputEvent>,
    ) -> Result<Transition, CognitiveError>;
    
    fn build_prompt(&self, state: &AgentState) -> String;
    fn requires_approval(&self, tool: &str, args: &str) -> bool;
}
```

## Invariants

- ✅ 100% deterministic
- ✅ No async
- ✅ No IO
- ✅ No tokio
- ✅ No channels
- ✅ No LLM client
- ✅ No tool execution
- ✅ No approval waiting
- ✅ No config loading
- ✅ Clone + Debug
- ✅ Zero unsafe

## Dependencies

Only std library:
- `std::fmt`
- `std::error`
- `std::collections::HashMap` (in types/ids.rs)

No external crates in cognition layer.

## Example Usage

```rust
use mylm_core::cognition::*;

// Initial state
let state = AgentState::new(50);

// Create engine
let mut engine = StubEngine::new();

// Step 1: User message
let transition = engine.step(
    &state,
    Some(InputEvent::UserMessage("Hello".into()))
).unwrap();

// Check decision
match transition.decision {
    AgentDecision::EmitResponse(resp) => {
        println!("Response: {}", resp);
    }
    _ => {}
}

// New state
let state = transition.next_state;
```

## Design Principles

1. **Intent, not execution** - Engine produces decisions, runtime executes
2. **Immutable state** - Each step produces new state, no mutation
3. **External fulfillment** - LLM requests emitted as decisions, responses fed back as input
4. **Deterministic** - Same (state, input) always produces same transition
5. **Testable** - Pure functions, no mocking needed

## Portability

This kernel can run on:
- CLI
- Background daemon
- Web server
- Distributed runtime
- Embedded system

Without modification.
