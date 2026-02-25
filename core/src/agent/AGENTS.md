# Agent Module

**Purpose:** MyLM's agent system - a layered architecture for LLM-powered agents.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│  SESSION          Orchestration layer (async)               │
│  - Session: Main event loop                                 │
│  - Input handlers: Chat, Task, Worker                       │
├─────────────────────────────────────────────────────────────┤
│  RUNTIME          Async capability execution (side effects) │
│  - AgentRuntime: Decision interpreter                       │
│  - CapabilityGraph: Trait-based capability container        │
│  - Capabilities: LLM, Tools, Approval, Workers, Telemetry   │
├─────────────────────────────────────────────────────────────┤
│  COGNITION        Pure state machine (no async/IO)          │
│  - StepEngine: (state, input) -> Transition                 │
│  - GraphEngine: events -> IntentGraph (DAG)                 │
│  - AgentState: Immutable snapshot                           │
│  - AgentDecision: Intent only (no execution)                │
├─────────────────────────────────────────────────────────────┤
│  TYPES            Primitive types (no dependencies)         │
│  - IDs: TaskId, WorkerId, SessionId, TraceId               │
│  - Common: TokenUsage, ToolResult, Approval                │
└─────────────────────────────────────────────────────────────┘
```

## Directory Structure

| Path | Purpose | Documentation |
|------|---------|---------------|
| `mod.rs` | Module exports and architecture overview | [View](mod.rs) |
| `README.md` | User-facing architecture guide | [View](README.md) |
| `builder.rs` | AgentBuilder for constructing agents | [View](builder.rs) |
| `factory.rs` | AgentSessionFactory from Config | [View](factory.rs) |
| `worker.rs` | Worker spawning and management | [View](worker.rs) |
| `identity.rs` | AgentId, AgentType for multi-agent | [View](identity.rs) |
| `types/` | Primitive types (no deps) | [AGENTS.md](types/AGENTS.md) |
| `cognition/` | Pure logic, no async/IO | [AGENTS.md](cognition/AGENTS.md) |
| `runtime/` | Async capabilities | [AGENTS.md](runtime/AGENTS.md) |
| `session/` | Orchestration layer | [AGENTS.md](session/AGENTS.md) |
| `tools/` | Tool implementations | [AGENTS.md](tools/AGENTS.md) |
| `memory/` | Agent memory integration | [AGENTS.md](memory/AGENTS.md) |
| `tests/` | Integration tests | [AGENTS.md](tests/AGENTS.md) |

## Key Design Rules

1. **Cognition is pure** - No async, no IO, no external deps
2. **Runtime handles side effects** - All IO, network, files
3. **Session orchestrates** - Connects layers in a loop
4. **Capabilities are swappable** - Implement traits to replace components
5. **State is immutable** - Each step produces new state

## Quick Start

```rust
use mylm_core::agent::{AgentBuilder, SessionInput, presets::testing_agent};
use tokio::sync::mpsc;

// Quick testing agent (all stubs)
let mut agent = testing_agent();

// Or build custom
let mut agent = AgentBuilder::new()
    .with_llm_client(llm_client)
    .with_tools(ToolRegistry::new())
    .with_terminal_approval()
    .build_with_llm_engine();

// Run session
let (tx, rx) = mpsc::channel(10);
tx.send(SessionInput::Chat("Hello".to_string())).await?;
drop(tx);

let result = agent.run(rx).await?;
```

## Cross-Module Dependencies

```
types/ (no deps)
    ↓
cognition/ (uses types/)
    ↓
runtime/ (uses types/, cognition/)
    ↓
session/ (uses all above)
    ↓
tools/ (used by runtime/)
memory/ (used by runtime/)
```
