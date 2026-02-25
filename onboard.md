# MyLM Agent Onboarding

**Welcome!** This guide gets you oriented quickly.

## Quick Start

1. **Read the root AGENTS.md** (if it exists) - high-level project overview
2. **Read `core/src/agent/AGENTS.md`** - agent module architecture
3. **Read this file** - you're here!
4. **Check `core/ARCHITECTURE_MAP.md`** - file reference

## Project Layout

```
mylm/
├── onboard.md              ← YOU ARE HERE - agent guide
├── README.md               ← User-facing project overview
├── core/                   ← Main library (mylm-core crate)
│   ├── src/agent/          ← Agent system - MOST CODE IS HERE
│   │   ├── AGENTS.md       ← Agent architecture overview
│   │   ├── types/          ← Primitive types (no deps)
│   │   ├── cognition/      ← Pure logic, no async/IO
│   │   ├── runtime/        ← Async capabilities
│   │   ├── session/        ← Orchestration layer
│   │   ├── tools/          ← Tool implementations
│   │   ├── memory/         ← Memory integration
│   │   └── tests/          ← Integration tests
│   ├── ARCHITECTURE_MAP.md ← File-by-file reference
│   └── src/                ← Other core modules
├── src/                    ← Application layer (main binary)
└── mylm/                   ← Additional crates
```

## Where to Look For...

| Task | Location |
|------|----------|
| **Add a tool** | `core/src/agent/tools/` |
| **Change LLM logic** | `core/src/agent/cognition/step/llm_engine.rs` |
| **Add capability** | `core/src/agent/runtime/core/capability.rs` + `capabilities/` |
| **Modify orchestration** | `core/src/agent/runtime/orchestrator/orchestrator.rs` |
| **Change state** | `core/src/agent/cognition/kernel.rs` (`AgentState`) |
| **Add error type** | `core/src/agent/types/error.rs` |
| **Build agent** | `core/src/agent/builder.rs` |
| **Fix tests** | `core/src/agent/tests/` |

## Architecture Cheat Sheet

```
┌─────────────┐
│   Session   │ ← Orchestrates: async loop, coordinates layers
├─────────────┤
│   Runtime   │ ← Executes: side effects (tools, LLM calls)
├─────────────┤
│  Cognition  │ ← Decides: pure logic (state machine)
├─────────────┤
│    Types    │ ← Data: primitive types (no logic)
└─────────────┘
```

**Key rule:** Cognition has NO async, NO IO. Runtime has ALL side effects.

## Key Files for Common Changes

### 1. Adding/Modifying Tools
```
core/src/agent/tools/
├── mod.rs              ← Add tool to ToolRegistry
├── my_tool.rs          ← Your tool implementation
└── (existing tools)    ← Use as examples
```

### 2. Changing Decision Logic
```
core/src/agent/cognition/
├── kernel.rs           ← AgentState, GraphEngine trait
├── planner.rs          ← Main planner (implements GraphEngine)
├── step/llm_engine.rs  ← LLM-based step engine
└── prompts/system.rs   ← System prompts
```

### 3. Changing Capabilities
```
core/src/agent/runtime/
├── core/capability.rs  ← Trait definitions
├── capabilities/       ← Implementations
└── executor/runtime.rs ← Interpret decisions
```

### 4. Session/Orchestration Changes
```
core/src/agent/runtime/orchestrator/
├── orchestrator.rs     ← Main event loop
├── dag_executor.rs     ← DAG execution
└── commonbox/          ← Coordination
```

## Build & Test

```bash
# Build
cargo build

# Test all agent code
cargo test --package mylm-core agent::

# Test specific module
cargo test --package mylm-core agent::cognition
cargo test --package mylm-core agent::runtime

# Run the app
cargo run
```

## Documentation Strategy

| File Type | Location | Purpose |
|-----------|----------|---------|
| `AGENTS.md` | Each folder | Module overview, file index |
| `//!` headers | Top of .rs files | What this file does, key exports |
| Doc comments | On types/functions | How to use |

## Quick Reference: Common Types

| Type | Location | Purpose |
|------|----------|---------|
| `AgentState` | `cognition/kernel.rs` | Immutable agent state |
| `AgentDecision` | `cognition/decision.rs` | What to do next |
| `InputEvent` | `cognition/input.rs` | External input |
| `IntentGraph` | `types/graph.rs` | DAG of intents |
| `CapabilityGraph` | `runtime/executor/graph.rs` | Wired capabilities |
| `RuntimeContext` | `runtime/core/context.rs` | Execution context |

## Getting Help

1. Check `AGENTS.md` in the relevant folder
2. Check `core/ARCHITECTURE_MAP.md` for file locations
3. Read module-level comments (`//!`) in source files
4. Look at existing tests in `core/src/agent/tests/`

## First-Time Tasks (Practice)

1. **Add a stub tool**: Add `echo` tool that returns its input
2. **Trace a request**: Follow flow from SessionInput → decision → execution
3. **Read a test**: Understand how `testing_agent()` works
4. **Run tests**: Make sure you can run `cargo test --package mylm-core agent::`

---

**Remember:**
- Cognition = pure (no async/IO)
- Runtime = side effects (all async/IO)
- Session = orchestration (coordinates both)
