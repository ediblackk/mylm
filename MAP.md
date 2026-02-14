# Core Module Structure

## Overview
- **Total Rust files:** 106
- **Main directories:** agent, config, context, executor, llm, memory, output, scheduler, state, terminal

## NEW ARCHITECTURE (Strict Layering)

Created stub structure with compiler-enforced boundaries:

```
core/src/
├── lib.rs                    # Updated with new modules
│
├── types/                    # LAYER 1: Primitives only
│   ├── mod.rs                # Re-export
│   ├── ids.rs                # TaskId, JobId, SessionId
│   └── common.rs             # TokenUsage, ToolResult, Approval
│
├── cognition/                # LAYER 2: Pure logic, NO async, NO IO
│   ├── mod.rs                # Re-export
│   ├── engine.rs             # CognitiveEngine trait
│   ├── state.rs              # AgentState (step counting, limits)
│   ├── decision.rs           # Decision, Transition types
│   ├── input.rs              # InputEvent enum
│   └── error.rs              # CognitiveError
│
├── runtime/                  # LAYER 3: Async + side effects, NO decision logic
│   ├── mod.rs                # Re-export + RuntimeError
│   ├── tools.rs              # ToolExecutor trait
│   ├── llm.rs                # LlmRuntime trait
│   ├── approval.rs           # ApprovalRuntime (pending approvals)
│   └── workers.rs            # WorkerRuntime (job management)
│
└── session/                  # LAYER 4: Orchestration only
    ├── mod.rs                # Re-export Session
    ├── session.rs            # Session struct (coordinates all layers)
    └── input/
        ├── mod.rs            # SessionInput, WorkerEvent
        ├── chat.rs           # ChatInputHandler
        ├── task.rs           # TaskInputHandler
        └── worker.rs         # WorkerInputHandler
```

## Dependency Rules (Enforced by Structure)

```
session ──────► runtime ──────► cognition ──────► types
    │               │               │               │
    └───────────────┴───────────────┴───────────────┘
              (no upward dependencies allowed)
```

- **types**: No dependencies
- **cognition**: types only
- **runtime**: cognition + types
- **session**: runtime + cognition + types

## OLD ARCHITECTURE (Being Refactored)

### Agent Module (`core/src/agent/`)

**STATUS: V1 DELETED**
- Deleted: `core/src/agent/v1/` folder
- Remaining cleanup needed in: wrapper.rs, factory.rs, orchestrator/

### Current Issues Found
1. `factory.rs` - imports deleted `Agent`
2. `wrapper.rs` - has `V1` variant
3. `orchestrator/mod.rs` - imports deleted `Agent`
4. `orchestrator/loops.rs` - imports deleted `V1AgentDecision`

---

## Files Changed/Added

| Action | File | Description |
|--------|------|-------------|
| DELETED | `agent/v1/` | V1 agent folder removed |
| CREATED | `types/` | New layer 1 - primitives |
| CREATED | `cognition/` | New layer 2 - pure logic |
| CREATED | `runtime/` | New layer 3 - async/IO |
| CREATED | `session/` | New layer 4 - orchestration |
| MODIFIED | `lib.rs` | Added new modules |

---

## Next Steps

1. **Fix compilation errors** - Clean up remaining V1 references
2. **Move logic from loops.rs** - Migrate to session/session.rs
3. **Implement real cognitive engine** - Replace stubs with actual logic
4. **Connect to existing tools/llm** - Bridge old and new architecture
5. **Update UI layer** - Use new session API

## Current Compilation Status

```
❌ Not compiling
Error: V1 references still exist in:
  - agent/wrapper.rs
  - agent/factory.rs
  - agent/v2/orchestrator/mod.rs
  - agent/v2/orchestrator/loops.rs
  - agent/v2/orchestrator/types.rs
```

Need to either:
- A) Clean up remaining V1 references in old code
- B) Keep V1 until new architecture is complete
