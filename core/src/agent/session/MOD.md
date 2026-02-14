# Session Module

**Purpose**: Orchestration layer that coordinates cognition and runtime.

The Session is the main event loop that:
1. Receives external input (chat, tasks, worker events)
2. Feeds to CognitiveEngine for decisions
3. Dispatches decisions to AgentRuntime
4. Feeds results back to engine
5. Continues until completion

## Files

| File | Purpose | Key Items |
|------|---------|-----------|
| `session.rs` | Main orchestration | `Session::run()` event loop |
| `mod.rs` | Module exports | `SessionInput`, `WorkerEvent` |
| `input/` | Input handlers | Translate external events to SessionInput |

## Input Handlers

| Handler | File | Purpose |
|---------|------|---------|
| `ChatInputHandler` | `chat.rs` | User chat messages |
| `TaskInputHandler` | `task.rs` | Single task commands |
| `WorkerInputHandler` | `worker.rs` | Worker lifecycle events |

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

## Using Sessions

### Basic Usage

```rust
use mylm_core::agent_v3::{
    AgentBuilder, SessionInput
};
use tokio::sync::mpsc;

// Build agent
let mut session = AgentBuilder::new()
    .with_auto_approve()
    .build_with_llm_engine();

// Create input channel
let (tx, rx) = mpsc::channel(10);

// Send input
tx.send(SessionInput::Chat("Hello".to_string())).await?;
drop(tx); // Close to signal end

// Run session
let result = session.run(rx).await?;
println!("Result: {}", result);
```

### Session Input Types

```rust
pub enum SessionInput {
    /// User chat message
    Chat(String),
    
    /// Single task execution
    Task { command: String, args: Vec<String> },
    
    /// Worker event
    Worker(WorkerEvent),
    
    /// Approval response
    Approval(ApprovalOutcome),
    
    /// Interrupt/cancel
    Interrupt,
}
```

### Worker Events

```rust
pub enum WorkerEvent {
    Spawned { job_id: JobId, description: String },
    Completed { job_id: JobId, result: String },
    Failed { job_id: JobId, error: String },
    Stalled { job_id: JobId, reason: String },
}
```

## Adding New Input Types

1. Add variant to `SessionInput` in `mod.rs`
2. Add handler in `input/` directory
3. Update `translate_input()` in `session.rs`
4. Handle in cognitive engine if needed

## Testing Sessions

```rust
#[tokio::test]
async fn test_session() {
    let mut session = testing_agent();
    let (tx, rx) = mpsc::channel(10);
    
    tx.send(SessionInput::Chat("test".to_string())).await.ok();
    drop(tx);
    
    let result = session.run(rx).await;
    assert!(result.is_ok());
}
```
