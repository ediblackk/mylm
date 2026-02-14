# Runtime Module

**Purpose**: Async capability execution. All side effects live here.

This layer interprets `AgentDecision` from cognition into actual actions.

## Files

| File | Purpose | Key Items |
|------|---------|-----------|
| `runtime.rs` | Main runtime | `AgentRuntime`, `interpret()` |
| `graph.rs` | Capability container | `CapabilityGraph` |
| `capability.rs` | Trait definitions | `LLMCapability`, `ToolCapability`, etc. |
| `context.rs` | Runtime context | `RuntimeContext`, `TraceId` |
| `error.rs` | Error types | `RuntimeError`, `LLMError`, `ToolError` |
| `impls/` | Implementations | See `impls/MOD.md` |

## Core Abstraction

```rust
/// Async decision interpreter
pub struct AgentRuntime {
    graph: CapabilityGraph,
}

impl AgentRuntime {
    pub async fn interpret(
        &self,
        ctx: &RuntimeContext,
        decision: AgentDecision,
    ) -> Result<Option<InputEvent>, RuntimeError> {
        // Dispatch to appropriate capability
    }
}
```

## Capability Traits

| Trait | Purpose | Methods |
|-------|---------|---------|
| `LLMCapability` | Text completion | `complete(ctx, req) -> LLMResponse` |
| `ToolCapability` | Tool execution | `execute(ctx, call) -> ToolResult` |
| `ApprovalCapability` | User approval | `request(ctx, req) -> ApprovalOutcome` |
| `WorkerCapability` | Spawn workers | `spawn(ctx, spec) -> WorkerHandle` |
| `TelemetryCapability` | Logging/metrics | `record_decision()`, `record_result()` |

## Adding a New Capability

1. Define trait in `capability.rs`
2. Implement in `impls/`
3. Add to `CapabilityGraph`
4. Handle in `AgentRuntime::interpret()`

## Example: Custom Capability

```rust
// capability.rs
#[async_trait]
pub trait MyCapability: Capability {
    async fn do_something(&self, ctx: &RuntimeContext) -> Result<String, MyError>;
}

// impls/my_cap.rs
pub struct MyCapImpl;

impl Capability for MyCapImpl {
    fn name(&self) -> &'static str { "my-cap" }
}

#[async_trait]
impl MyCapability for MyCapImpl {
    async fn do_something(&self, ctx: &RuntimeContext) -> Result<String, MyError> {
        // Implementation
    }
}
```

## Testing

Runtime is tested via integration tests with mock capabilities:

```rust
// Use Stub implementations for testing
let runtime = AgentRuntime::new(CapabilityGraph::stub());
```
