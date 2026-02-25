# Types Module

**Purpose:** Primitive types only. No logic, no dependencies, no async.

## Files

| File | Purpose | Key Types |
|------|---------|-----------|
| `mod.rs` | Module exports | Re-exports all types |
| `ids.rs` | Identifier types | `TaskId`, `JobId`, `SessionId`, `TraceId`, `WorkerId` |
| `common.rs` | Common primitives | `TokenUsage`, `ToolResult`, `Approval` |
| `intents.rs` | Intent types | `ToolCall`, `WorkerSpec`, etc. |
| `events.rs` | Event types | `InputEvent`, `OutputEvent`, `ToolResult` |
| `observations.rs` | Observation types | `Observation`, `ObservationKind` |
| `graph.rs` | Graph types | `IntentGraph`, `NodeId`, `EdgeId` |
| `envelope.rs` | Message envelopes | `Envelope`, `RoutingInfo` |
| `config.rs` | Configuration types | `AgentConfig`, `ToolConfig`, etc. |
| `error.rs` | Error types | `AgentError`, etc. |
| `parser/` | Response parsing | `ResponseParser`, `ParsedResponse` |

## Design Principles

1. **Zero dependencies** - Only std library (except serde)
2. **Clone-friendly** - All types implement `Clone`
3. **Serializable** - All types implement `Serialize`/`Deserialize`
4. **Immutable** - Create new values, don't mutate

## Usage

These types are used by BOTH cognition and runtime layers. Neither layer should depend on the other - both depend on types.

```rust
use mylm_core::agent::types::{TaskId, ToolResult, Approval};
```
