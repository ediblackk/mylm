# Session Module

**Purpose:** Orchestration layer that coordinates cognition and runtime.

The Session is the main event loop that:
1. Receives external input (chat, tasks, worker events)
2. Feeds to CognitiveEngine for decisions
3. Dispatches decisions to AgentRuntime
4. Feeds results back to engine
5. Continues until completion

## Files

| File | Purpose | Key Items |
|------|---------|-----------|
| `mod.rs` | Module exports | `SessionInput`, `WorkerEvent`, persistence |
| `session.rs` | Main orchestration | `Session::run()` event loop |
| `persistence.rs` | Session persistence | `SessionPersistence`, checkpoints |
| `input/mod.rs` | Input exports | Input handlers |
| `input/chat.rs` | Chat handler | `ChatInputHandler` |
| `input/task.rs` | Task handler | `TaskInputHandler` |
| `input/worker.rs` | Worker handler | `WorkerInputHandler` |

## Session Flow

```
┌─────────────┐
│   Input     │ (Chat, Task, Worker event)
└──────┬──────┘
       ↓
┌─────────────┐
│  translate  │ Convert to InputEvent
└──────┬──────┘
       ↓
┌─────────────┐
│CognitiveEng │ step(state, input) -> Transition
└──────┬──────┘
       ↓
┌─────────────┐
│ update state│ state = transition.next_state
└──────┬──────┘
       ↓
┌─────────────┐
│AgentRuntime │ interpret(decision) -> InputEvent?
└──────┬──────┘
       ↓
┌─────────────┐
│   Output    │ (or loop back)
└─────────────┘
```

## Session Input Types

```rust
pub enum SessionInput {
    Chat(String),
    Task { command: String, args: Vec<String> },
    Worker(WorkerEvent),
    Approval(ApprovalOutcome),
    Interrupt,
}
```

## Dependencies

- Uses `crate::agent::types`
- Uses `crate::agent::cognition`
- Uses `crate::agent::runtime`
- Top of the dependency chain
