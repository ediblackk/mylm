# Agent V3 - Capability Graph Architecture

## Overview

Clean, layered agent architecture with strict separation of concerns:

```
┌─────────────────────────────────────────────────────────────┐
│  SESSION          Orchestration layer (async)               │
│  - Session: Main event loop                                 │
│  - Input handlers: Chat, Task, Worker                       │
├─────────────────────────────────────────────────────────────┤
│  RUNTIME          Async capability execution (side effects) │
│  - AgentRuntime: Decision interpreter                       │
│  - CapabilityGraph: Trait-based capability container        │
│  - Capability traits: LLM, Tools, Approval, Workers, Telemetry│
│  - Implementations: Local, Stub, Retry wrappers             │
├─────────────────────────────────────────────────────────────┤
│  COGNITION        Pure state machine (no async/IO)          │
│  - CognitiveEngine: (state, input) → Transition             │
│  - AgentState: Immutable snapshot                           │
│  - AgentDecision: Intent only (CallTool, RequestLLM, etc.)  │
│  - InputEvent: External stimuli                             │
├─────────────────────────────────────────────────────────────┤
│  TYPES            Primitive types (no dependencies)         │
│  - IDs: TaskId, WorkerId, SessionId, TraceId               │
│  - Common: TokenUsage, ToolResult, Approval                │
└─────────────────────────────────────────────────────────────┘
```

## Directory Structure

```
agent_v3/
├── README.md           # This file - architecture overview
├── builder.rs          # AgentBuilder for easy construction
├── types/              # Primitive types only
│   ├── MOD.md          # Module documentation
│   ├── ids.rs          # Identifier types
│   └── common.rs       # Common primitives
├── cognition/          # Pure logic (no async)
│   ├── MOD.md          # Module documentation
│   ├── state.rs        # AgentState (immutable)
│   ├── input.rs        # InputEvent enum
│   ├── decision.rs     # AgentDecision enum
│   ├── engine.rs       # CognitiveEngine trait
│   ├── llm_engine.rs   # LLM-based engine implementation
│   ├── error.rs        # CognitiveError
│   └── history.rs      # Message history types
├── runtime/            # Async capabilities (side effects)
│   ├── MOD.md          # Module documentation
│   ├── runtime.rs      # AgentRuntime (interpreter)
│   ├── graph.rs        # CapabilityGraph
│   ├── capability.rs   # Capability traits
│   ├── context.rs      # RuntimeContext, TraceId
│   ├── error.rs        # RuntimeError
│   └── impls/          # Capability implementations
│       ├── MOD.md      # Implementation guide
│       ├── tool_registry.rs    # ToolRegistry with 8 tools
│       ├── llm_client.rs       # LlmClient bridge
│       ├── terminal_approval.rs # Interactive approval
│       ├── local_worker.rs     # Tokio task workers
│       ├── console_telemetry.rs # Logging/metrics
│       ├── web_search.rs       # Web search capability
│       ├── memory.rs           # Long-term memory
│       ├── simple_tool.rs      # Basic tool executor
│       ├── retry.rs            # Retry wrappers
│       └── local.rs            # Local implementations
├── session/            # Orchestration layer
│   ├── MOD.md          # Module documentation
│   ├── session.rs      # Main Session loop
│   ├── mod.rs          # Session exports
│   └── input/          # Input handlers
│       ├── mod.rs
│       ├── chat.rs
│       ├── task.rs
│       └── worker.rs
├── test_architecture.rs # Architecture verification tests
└── example_integration.rs # Integration examples
```

## Quick Start

### Creating a Simple Agent

```rust
use mylm_core::agent_v3::{
    AgentBuilder, Session, SessionInput,
    presets::testing_agent,
};

// Quick testing agent (all stubs)
let mut agent = testing_agent();

// Or build custom
let mut agent = AgentBuilder::new()
    .with_llm_client(llm_client)
    .with_tools(ToolRegistry::new())
    .with_terminal_approval()
    .with_local_workers()
    .with_telemetry()
    .with_memory()
    .build_with_llm_engine();
```

### Running a Session

```rust
use tokio::sync::mpsc;

let (tx, rx) = mpsc::channel(10);
tx.send(SessionInput::Chat("Hello".to_string())).await?;
drop(tx); // Close channel to signal end

let result = agent.run(rx).await?;
```

### Adding a Custom Tool

```rust
use mylm_core::agent_v3::runtime::impls::ToolRegistry;

let mut tools = ToolRegistry::new();
tools.register("my_tool", Arc::new(|_ctx, args| {
    Box::pin(async move {
        // Tool implementation
        Ok(ToolResult {
            tool: "my_tool".to_string(),
            output: format!("Processed: {}", args),
            success: true,
        })
    })
}));
```

## Architecture Rules

1. **Cognition is pure**: No async, no IO, no external dependencies
2. **Runtime handles side effects**: All IO, network, file operations
3. **Session orchestrates**: Connects cognition and runtime in a loop
4. **Capabilities are swappable**: Implement traits to replace any component
5. **State is immutable**: Each step produces new state, never mutates

## Testing

```bash
# Run architecture tests
cargo test --lib agent_v3

# Run all tests
cargo test --lib
```

## Adding New Capabilities

See `runtime/impls/MOD.md` for the capability implementation guide.
