# Agent Architecture Map

Quick reference guide for navigating the MyLM agent codebase.

## Project Structure

```
mylm/
├── core/src/agent/          # Agent system (main code)
│   ├── AGENTS.md           # ← START HERE - module overview
│   ├── types/              # Primitive types
│   ├── cognition/          # Pure logic (no async/IO)
│   ├── runtime/            # Async capabilities
│   ├── session/            # Orchestration
│   ├── tools/              # Tool implementations
│   ├── memory/             # Memory integration
│   └── tests/              # Integration tests
├── src/                     # Application layer
├── mylm/                    # Additional modules
└── onboard.md              # ← YOU ARE HERE
```

## File Index by Purpose

### Core Types (`core/src/agent/types/`)
| File | Purpose | Key Types |
|------|---------|-----------|
| `ids.rs` | Identifiers | `IntentId`, `TaskId`, `JobId`, `SessionId` |
| `common.rs` | Primitives | `Approval`, `TokenUsage` |
| `intents.rs` | What to do | `Intent`, `ToolCall`, `LLMRequest` |
| `events.rs` | What happened | `KernelEvent`, `LLMResponse`, `ToolResult` |
| `observations.rs` | Results | `Observation` |
| `graph.rs` | DAG structure | `IntentGraph` |
| `error.rs` | Errors | `AgentError` |

### Cognition (`core/src/agent/cognition/`)
| File | Purpose | Key Type |
|------|---------|----------|
| `kernel.rs` | Graph engine trait | `GraphEngine`, `AgentState` |
| `planner.rs` | Decision planner | `Planner` |
| `engine.rs` | Step engine trait | `StepEngine` |
| `step/llm_engine.rs` | LLM engine impl | `LlmEngine` |
| `input.rs` | Input events | `InputEvent` |
| `decision.rs` | Decisions | `AgentDecision`, `Transition` |
| `error.rs` | Errors | `CognitiveError` |
| `prompts/system.rs` | Prompt building | `build_system_prompt()` |
| `policy/approval.rs` | Approval policy | `requires_approval()` |

### Runtime Core (`core/src/agent/runtime/core/`)
| File | Purpose | Key Types |
|------|---------|-----------|
| `capability.rs` | Trait definitions | `*Capability` traits |
| `context.rs` | Execution context | `RuntimeContext`, `TraceId` |
| `error.rs` | Runtime errors | `RuntimeError`, `*Error` |
| `terminal.rs` | Terminal abstraction | `TerminalExecutor` |

### Runtime Executor (`core/src/agent/runtime/executor/`)
| File | Purpose | Key Type |
|------|---------|----------|
| `runtime.rs` | Decision interpreter | `AgentRuntime` |
| `graph.rs` | Capability container | `CapabilityGraph` |

### Runtime Capabilities (`core/src/agent/runtime/capabilities/`)
| File | Capability | Purpose |
|------|------------|---------|
| `llm.rs` | `LLMCapability` | LLM client bridge |
| `local.rs` | `ToolCapability` | Tool registry wrapper |
| `approval.rs` | `ApprovalCapability` | Terminal/auto approval |
| `worker.rs` | `WorkerCapability` | Worker spawning |
| `telemetry.rs` | `TelemetryCapability` | Logging/metrics |
| `memory.rs` | `TelemetryCapability` | Memory events |
| `retry.rs` | Wrappers | Retry decorators |

### Runtime Orchestrator (`core/src/agent/runtime/orchestrator/`)
| File | Purpose | Key Type |
|------|---------|----------|
| `orchestrator.rs` | Main session | `AgencySession` |
| `dag_executor.rs` | DAG execution | `execute_dag()` |
| `contract_bridge.rs` | Legacy bridge | `ContractRuntime` |
| `commonbox/` | Coordination | `Commonbox`, `Job` |
| `transport/` | Event transport | `EventTransport` |

### Session (`core/src/agent/session/`)
| File | Purpose |
|------|---------|
| `session.rs` | `Session` orchestrator |
| `persistence.rs` | Session persistence |
| `input/mod.rs` | `SessionInput` types |
| `input/chat.rs` | Chat handler |
| `input/task.rs` | Task handler |
| `input/worker.rs` | Worker handler |

### Tools (`core/src/agent/tools/`)
| File | Tool | Purpose |
|------|------|---------|
| `mod.rs` | `ToolRegistry` | Aggregates all tools |
| `shell.rs` | `ShellTool` | Command execution |
| `worker_shell.rs` | `WorkerShellTool` | Restricted shell |
| `read_file/` | `ReadFileTool` | File reading with chunks |
| `write_file.rs` | `WriteFileTool` | File writing |
| `list_files.rs` | `ListFilesTool` | Directory listing |
| `git.rs` | Git tools | Status, log, diff |
| `web_search.rs` | `WebSearchTool` | Web search |
| `search_files.rs` | `SearchFilesTool` | Full-text search |
| `memory.rs` | `MemoryTool` | Memory storage |
| `delegate/` | `DelegateTool` | Worker spawning |
| `scratchpad.rs` | `ScratchpadTool` | Agent notes |
| `commonboard.rs` | `CommonboardTool` | Coordination |

### Memory (`core/src/agent/memory/`)
| File | Purpose | Key Type |
|------|---------|----------|
| `manager.rs` | Memory manager | `AgentMemoryManager` |
| `context.rs` | Context building | `MemoryContextBuilder` |
| `extraction.rs` | Memory extraction | `MemoryExtractor` |

### Other Agent Files
| File | Purpose |
|------|---------|
| `builder.rs` | `AgentBuilder` - construct agents |
| `factory.rs` | `AgentSessionFactory` - from Config |
| `worker.rs` | Worker spawning |
| `identity.rs` | `AgentId`, `AgentType` |

## Documentation Files

| File | Location | Purpose |
|------|----------|---------|
| `onboard.md` | Root | Agent onboarding guide |
| `AGENTS.md` | `core/src/agent/` | Module overview |
| `AGENTS.md` | `core/src/agent/types/` | Types guide |
| `AGENTS.md` | `core/src/agent/cognition/` | Cognition guide |
| `AGENTS.md` | `core/src/agent/runtime/` | Runtime guide |
| `AGENTS.md` | `core/src/agent/session/` | Session guide |
| `AGENTS.md` | `core/src/agent/tools/` | Tools guide |
| `AGENTS.md` | `core/src/agent/memory/` | Memory guide |
| `AGENTS.md` | `core/src/agent/tests/` | Tests guide |
| `ARCHITECTURE_MAP.md` | `core/` | This file - quick reference |

## Common Tasks

### Add a New Tool
1. Create tool in `tools/my_tool.rs`
2. Add to `ToolRegistry` in `tools/mod.rs`
3. Add description in `descriptions()` method

### Add a New Capability
1. Define trait in `runtime/core/capability.rs`
2. Implement in `runtime/capabilities/my_cap.rs`
3. Add to `CapabilityGraph` in `runtime/executor/graph.rs`
4. Handle in `AgentRuntime::interpret()` if needed

### Create Custom Engine
1. Implement `StepEngine` trait (cognition)
2. Or implement `GraphEngine` trait for DAG planning
3. Use with `AgentBuilder::new().with_engine(my_engine)`

### Modify State
1. Open `cognition/kernel.rs` - find `AgentState`
2. Add field, keep immutable pattern
3. Add builder method if needed

### Add Test
1. Add to `tests/test_architecture.rs` for arch tests
2. Add to `tests/integration_tests.rs` for integration
3. Or create new test file in `tests/`

## Key Traits

```rust
// Cognition - Pure logic (no async, no IO)
pub trait StepEngine {
    fn step(&mut self, state: &AgentState, input: Option<InputEvent>) 
        -> Result<Transition, CognitiveError>;
}

pub trait GraphEngine {
    fn process(&mut self, events: &[KernelEvent]) 
        -> Result<IntentGraph, KernelError>;
}

// Runtime - Async capabilities (side effects)
#[async_trait]
pub trait ToolCapability: Capability {
    async fn execute(&self, ctx: &RuntimeContext, call: ToolCall) 
        -> Result<ToolResult, ToolError>;
}

#[async_trait]
pub trait LLMCapability: Capability {
    async fn complete(&self, ctx: &RuntimeContext, req: LLMRequest) 
        -> Result<LLMResponse, LLMError>;
}
```

## Flow Reference

```
Input → SessionInput → InputEvent → StepEngine::step()
                                           ↓
                                   Transition { state, decision }
                                           ↓
                                   AgentRuntime::interpret(decision)
                                           ↓
                                   Capability::execute()
                                           ↓
                                   InputEvent → [loop back]
```

## Testing Commands

```bash
# All agent tests
cargo test --package mylm-core agent::

# Architecture tests
cargo test --package mylm-core agent::tests::test_architecture

# Integration tests
cargo test --package mylm-core agent::tests::integration_tests

# Builder tests
cargo test --package mylm-core agent::builder
```

## Dependencies Between Layers

```
session → runtime, cognition
tools → runtime::core, types
runtime → cognition (types only), types
cognition → types only
types → std only
```

**No reverse dependencies allowed!**

## Architecture Rules

1. **Cognition is pure** - No async, no IO, no external deps
2. **Runtime handles side effects** - All IO, network, files
3. **Session orchestrates** - Connects layers in a loop
4. **Capabilities are swappable** - Implement traits to replace
5. **State is immutable** - Each step produces new state
