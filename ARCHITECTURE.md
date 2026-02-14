# New Architecture: Capability Graph Runtime

## Overview

Three-layer architecture with strict boundaries:

```
┌─────────────────────────────────────────────────────────────┐
│ SESSION                                                     │
│ (orchestration)                                             │
│ - Session::run() loop                                       │
│ - Input translation                                         │
│ - State management                                          │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│ RUNTIME                                                     │
│ (async, side effects)                                       │
│ - AgentRuntime::interpret()                                 │
│ - CapabilityGraph                                           │
│ - LLM, Tool, Approval, Worker, Telemetry capabilities       │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│ COGNITION                                                   │
│ (pure, deterministic)                                       │
│ - CognitiveEngine::step(state, input) -> Transition         │
│ - AgentState (immutable)                                    │
│ - AgentDecision (intent)                                    │
└─────────────────────────────────────────────────────────────┘
```

## Layer Rules

| Layer | Can Import | Cannot |
|-------|-----------|--------|
| cognition | types | runtime, session, async |
| runtime | cognition, types | session |
| session | runtime, cognition, types | - |
| types | - | everything |

## File Structure

### types/ (Layer 0)
- `ids.rs` - TaskId, JobId, WorkerId
- `common.rs` - TokenUsage, ToolResult, Approval

### cognition/ (Layer 1)
- `state.rs` - AgentState (immutable, Clone, Debug)
- `input.rs` - InputEvent (UserMessage, WorkerResult, etc.)
- `decision.rs` - AgentDecision (CallTool, RequestLLM, etc.)
- `engine.rs` - CognitiveEngine trait
- `error.rs` - CognitiveError
- `history.rs` - Message, MessageRole

### runtime/ (Layer 2)
- `capability.rs` - Capability traits (LLM, Tool, Approval, Worker, Telemetry)
- `context.rs` - RuntimeContext (TraceId, CancellationToken)
- `graph.rs` - CapabilityGraph (strongly typed composition)
- `runtime.rs` - AgentRuntime::interpret()
- `error.rs` - RuntimeError
- `impls/retry.rs` - RetryLLMWrapper (capability wrapping)
- `impls/local.rs` - Stub implementations

### session/ (Layer 3)
- `session.rs` - Session::run() loop
- `input/` - Input handlers (chat, task, worker)

## Key Types

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

All updates return new state: `state.increment_step()`, `state.with_message(msg)`

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

### CognitiveEngine
```rust
pub trait CognitiveEngine {
    fn step(&mut self, state: &AgentState, input: Option<InputEvent>) 
        -> Result<Transition, CognitiveError>;
    fn build_prompt(&self, state: &AgentState) -> String;
    fn requires_approval(&self, tool: &str, args: &str) -> bool;
}
```

### AgentRuntime
```rust
pub struct AgentRuntime {
    graph: CapabilityGraph,
}

impl AgentRuntime {
    pub async fn interpret(&self, ctx: &RuntimeContext, decision: AgentDecision) 
        -> Result<Option<InputEvent>, RuntimeError>;
}
```

## Data Flow

```
User Input
    │
    ▼
SessionInput ──► Session::translate_input() ──► InputEvent
    │
    ▼
CognitiveEngine::step(state, input) ──► Transition
    │
    ▼
AgentDecision ──► AgentRuntime::interpret() ──► CapabilityGraph
    │                                               │
    │                    ┌──────────────────────────┼──────────┐
    │                    ▼                          ▼          ▼
    │                 LLM.complete()           Tool.execute()  etc.
    │                    │                          │
    │                    └──────────────────────────┘
    │                                               │
    └───────────────────────────────────────────────┘
                        │
                        ▼
                 Option<InputEvent>
                        │
                        ▼
                (loop continues)
```

## Capability Wrapping

Example: Retry wrapper
```rust
pub struct RetryLLMWrapper {
    inner: Arc<dyn LLMCapability>,
    max_retries: usize,
}

#[async_trait::async_trait]
impl LLMCapability for RetryLLMWrapper {
    async fn complete(&self, ctx: &RuntimeContext, req: LLMRequest) 
        -> Result<LLMResponse, LLMError> {
        // Retry logic here
        self.inner.complete(ctx, req).await
    }
}
```

## Distributed Readiness

Replace local capabilities with remote:
```rust
// Local
let llm = Arc::new(LocalLLMStub::new());

// Remote (RPC)
let llm = Arc::new(RpcLLMClient::new("http://llm-service:8080"));

// Same interface, no code changes in cognition/session
```

## Invariants

- ✅ cognition: No async, no IO, no channels
- ✅ runtime: Async, side effects, no decisions
- ✅ session: Orchestration only
- ✅ All layers: Zero unsafe
- ✅ Capability graph: Strongly typed, no dynamic lookup
- ✅ Cancellation: Propagates through RuntimeContext
- ✅ Telemetry: Records but never modifies

## Remaining Cleanup

Old code still references deleted V1:
- `agent/v2/driver/factory.rs`
- `agent/v2/orchestrator/loops.rs`
- `agent/v2/orchestrator/types.rs`
- `agent/v2/orchestrator/mod.rs`
- `agent/wrapper.rs`
- `factory.rs`

These need to be either:
1. Updated to use new architecture
2. Deleted if no longer needed

## Next Steps

1. Clean up old V1 references
2. Implement real CognitiveEngine (not stub)
3. Connect to existing LLM client
4. Connect to existing tool registry
5. Wire up to UI layer
