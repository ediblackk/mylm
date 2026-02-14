# Types Module

**Purpose**: Primitive types only. No logic, no dependencies, no async.

## Files

| File | Purpose | Key Types |
|------|---------|-----------|
| `ids.rs` | Identifier types | `TaskId`, `JobId`, `SessionId` |
| `common.rs` | Common primitives | `TokenUsage`, `ToolResult`, `Approval` |

## Design Principles

1. **Zero dependencies**: This module should not import anything outside std
2. **Clone-friendly**: All types implement `Clone`
3. **Serializable**: All types implement `Serialize`/`Deserialize` where applicable
4. **Immutable**: No mutable references, create new values instead

## Adding New Types

When adding new primitive types:

1. Add to appropriate file (ids.rs for IDs, common.rs for everything else)
2. Derive Clone, Debug, PartialEq
3. Keep it simple - no methods that do IO or complex logic
4. Export in `mod.rs`

## Example

```rust
/// New primitive type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MyNewType {
    pub field: String,
}
```
