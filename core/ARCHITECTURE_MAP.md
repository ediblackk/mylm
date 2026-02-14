# Agent V3 Architecture Map

Quick reference guide for navigating the codebase.

## File Index by Purpose

### Core Types (Pure Data)
| File | Purpose | Line Count |
|------|---------|------------|
| `src/agent_v3/types/ids.rs` | TaskId, JobId, SessionId | ~30 |
| `src/agent_v3/types/common.rs` | TokenUsage, ToolResult, Approval | ~30 |

### Cognition (Pure Logic)
| File | Purpose | Key Type |
|------|---------|----------|
| `src/agent_v3/cognition/state.rs` | Immutable agent state | `AgentState` |
| `src/agent_v3/cognition/input.rs` | External events | `InputEvent` |
| `src/agent_v3/cognition/decision.rs` | Intent/decisions | `AgentDecision` |
| `src/agent_v3/cognition/engine.rs` | Core trait | `CognitiveEngine` |
| `src/agent_v3/cognition/llm_engine.rs` | LLM-based engine | `LLMBasedEngine` |
| `src/agent_v3/cognition/error.rs` | Error types | `CognitiveError` |
| `src/agent_v3/cognition/history.rs` | Message history | `Message` |

### Runtime (Async Capabilities)
| File | Purpose | Key Type |
|------|---------|----------|
| `src/agent_v3/runtime/runtime.rs` | Decision interpreter | `AgentRuntime` |
| `src/agent_v3/runtime/graph.rs` | Capability container | `CapabilityGraph` |
| `src/agent_v3/runtime/capability.rs` | Trait definitions | `*Capability` traits |
| `src/agent_v3/runtime/context.rs` | Runtime context | `RuntimeContext`, `TraceId` |
| `src/agent_v3/runtime/error.rs` | Error types | `RuntimeError` |

### Capability Implementations
| File | Capability | Tools/Features |
|------|------------|----------------|
| `impls/tool_registry.rs` | `ToolCapability` | 8 built-in tools |
| `impls/llm_client.rs` | `LLMCapability` | LlmClient bridge |
| `impls/terminal_approval.rs` | `ApprovalCapability` | Interactive prompts |
| `impls/local_worker.rs` | `WorkerCapability` | Tokio task spawning |
| `impls/console_telemetry.rs` | `TelemetryCapability` | Logging/metrics |
| `impls/web_search.rs` | `ToolCapability` | Web search |
| `impls/memory.rs` | `TelemetryCapability` | Long-term memory |

### Session (Orchestration)
| File | Purpose |
|------|---------|
| `src/agent_v3/session/session.rs` | Main event loop |
| `src/agent_v3/session/input/mod.rs` | Input types |
| `src/agent_v3/session/input/chat.rs` | Chat handler |
| `src/agent_v3/session/input/task.rs` | Task handler |
| `src/agent_v3/session/input/worker.rs` | Worker handler |

### Builder & Utils
| File | Purpose |
|------|---------|
| `src/agent_v3/builder.rs` | AgentBuilder pattern |
| `src/agent_v3/test_architecture.rs` | Architecture tests |
| `src/agent_v3/example_integration.rs` | Integration examples |

## Documentation Files

| File | Location | Purpose |
|------|----------|---------|
| `README.md` | `agent_v3/` | Architecture overview |
| `ARCHITECTURE_MAP.md` | `core/` | This file - quick reference |
| `MOD.md` | `types/` | Types module guide |
| `MOD.md` | `cognition/` | Cognition module guide |
| `MOD.md` | `runtime/` | Runtime module guide |
| `MOD.md` | `runtime/impls/` | Implementation guide |
| `MOD.md` | `session/` | Session module guide |

## Common Tasks

### Add a New Tool
1. Open `impls/tool_registry.rs`
2. Add tool function (see existing examples)
3. Register in `register_defaults()`

### Add a New Capability
1. Define trait in `runtime/capability.rs` (if new trait type)
2. Implement in `runtime/impls/my_cap.rs`
3. Export in `runtime/impls/mod.rs`
4. Add stub in `runtime/graph.rs`
5. Update `AgentRuntime::interpret()` if needed

### Create Custom Engine
1. Implement `CognitiveEngine` trait
2. Override `step()`, `build_prompt()`, `requires_approval()`
3. Use with `AgentBuilder::new().with_engine(my_engine)`

### Modify State
1. Open `cognition/state.rs`
2. Add field to `AgentState`
3. Add builder method (e.g., `with_my_field()`)
4. Keep immutable pattern

### Add Test
1. Add to `test_architecture.rs` for architecture tests
2. Add to `example_integration.rs` for integration tests
3. Or create new test module

## Key Traits

```rust
// Cognition - Pure logic
trait CognitiveEngine {
    fn step(&mut self, state: &AgentState, input: Option<InputEvent>) 
        -> Result<Transition, CognitiveError>;
}

// Runtime - Async capabilities
#[async_trait]
trait ToolCapability {
    async fn execute(&self, ctx: &RuntimeContext, call: ToolCall) 
        -> Result<ToolResult, ToolError>;
}

#[async_trait]
trait LLMCapability {
    async fn complete(&self, ctx: &RuntimeContext, req: LLMRequest) 
        -> Result<LLMResponse, LLMError>;
}
```

## Flow Reference

```
Input → SessionInput → InputEvent → CognitiveEngine.step()
                                           ↓
                                   Transition { state, decision }
                                           ↓
                                   AgentRuntime.interpret(decision)
                                           ↓
                                   Capability.execute()
                                           ↓
                                   InputEvent → [loop back]
```

## Testing Commands

```bash
# All agent_v3 tests
cargo test --lib agent_v3

# Architecture tests only
cargo test --lib agent_v3::test_architecture

# Integration tests
cargo test --lib agent_v3::example_integration

# Builder tests
cargo test --lib agent_v3::builder
```

## Dependencies Between Layers

```
session → cognition, runtime
runtime → cognition (types only), types
cognition → types only
types → std only
```

No reverse dependencies allowed!
