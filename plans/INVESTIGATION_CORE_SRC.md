# Core/src Investigation Report - COMPLETED

**Date:** 2026-02-08  
**Status:** ✅ STRUCTURE FIXED - Project compiles successfully

---

## FINAL DIRECTORY STRUCTURE (Option B Implemented)

```
core/src/
├── lib.rs                    # Main lib exports
├── error.rs                  # Core error types
├── factory.rs                # Session creation factory
├── protocol.rs               # Protocol definitions
├── rate_limiter.rs           # Rate limiting
├── util.rs                   # Utilities
│
├── agent/                    # AGENT MODULE (REORGANIZED)
│   ├── mod.rs                # Module declarations + re-exports
│   │
│   ├── tool.rs               # Tool trait (SHARED)
│   ├── protocol.rs           # Protocol types (SHARED)
│   ├── event_bus.rs          # Event bus (SHARED) - re-exports from v2/orchestrator
│   ├── tool_registry.rs      # Tool registry (SHARED)
│   ├── toolcall_log.rs       # Tool call logging (SHARED)
│   ├── execution.rs          # Execution helpers (SHARED)
│   ├── context.rs            # Context management (SHARED)
│   ├── permissions.rs        # Permissions (SHARED)
│   ├── role.rs               # Role definitions (SHARED)
│   ├── workspace.rs          # Workspace management (SHARED)
│   ├── wait.rs               # Wait functionality (SHARED)
│   ├── budget.rs             # Budget management (SHARED)
│   ├── logger.rs             # Logging (SHARED)
│   ├── wrapper.rs            # Agent wrapper (SHARED)
│   ├── traits.rs             # Shared traits (SHARED)
│   ├── prompt.rs             # Prompt builder (SHARED)
│   │
│   ├── v1/                   # V1 LEGACY - MARKED FOR DELETION
│   │   ├── mod.rs
│   │   └── core.rs           # Was agent/core.rs
│   │
│   ├── v2/                   # V2 ACTIVE IMPLEMENTATION
│   │   ├── mod.rs
│   │   ├── core.rs           # AgentV2 struct
│   │   ├── execution.rs      # Parallel tool execution
│   │   ├── jobs.rs           # Job registry
│   │   ├── lifecycle.rs      # State management
│   │   ├── memory.rs         # Memory helpers
│   │   ├── protocol/         # Extended protocol (parser + types)
│   │   ├── recovery.rs       # Error recovery
│   │   ├── driver/           # Execution drivers
│   │   │   ├── mod.rs
│   │   │   ├── factory.rs    # BuiltAgent, AgentBuilder
│   │   │   ├── event_driven.rs
│   │   │   └── legacy.rs
│   │   └── orchestrator/     # Session manager
│   │       ├── mod.rs
│   │       ├── event_bus.rs  # CoreEvent, EventBus (canonical)
│   │       ├── helpers.rs
│   │       ├── loops.rs      # Main loop
│   │       ├── types.rs
│   │       ├── pacore/       # PaCore integration
│   │       └── reasoning/    # Reasoning engine
│   │
│   └── tools/                # Tool implementations
│       ├── mod.rs
│       ├── delegate.rs       # Worker spawning
│       └── ... (other tools)
│
├── config/                   # Configuration
├── context/                  # Context management
├── executor/                 # Command execution
├── llm/                      # LLM client
├── memory/                   # Memory system
├── output/                   # Output handling
├── scheduler/                # Background scheduler
├── state/                    # State management
└── terminal/                 # Terminal UI
```

---

## KEY CHANGES MADE

### 1. Module Structure Fixed
- **Before:** Files scattered in `common/` folder, module declarations broken
- **After:** Shared files at `agent/` root, V1 isolated in `v1/`, V2 in `v2/`

### 2. Re-exports Added (agent/mod.rs)
```rust
// Shared components
pub use tool::{Tool, ToolKind, ToolOutput};
pub use event_bus::{CoreEvent, EventBus};
pub use prompt::PromptBuilder;
pub use wrapper::AgentWrapper;

// V2 (Active)
pub use v2::{AgentV2, AgentV2Config};
pub use v2::orchestrator::{AgentOrchestrator, OrchestratorConfig, ...};

// V1 (Legacy - marked for deletion)
pub use v1::{Agent, AgentConfig, AgentDecision};
```

### 3. Import Paths Fixed in Main Crate
Files in `src/` updated to use new paths:
- `mylm_core::agent::orchestrator::...` → `mylm_core::agent::...`
- `mylm_core::agent::factory::BuiltAgent` → `mylm_core::BuiltAgent`
- `mylm_core::agent::core::...` → `mylm_core::agent::v1::...`

### 4. EventBus Location
- **Canonical location:** `v2/orchestrator/event_bus.rs`
- **Re-export:** `agent/event_bus.rs` re-exports for convenience

---

## COMPILATION STATUS

```bash
$ cargo check
    Checking mylm-core v0.1.0
    Checking mylm v0.1.0
    Finished dev profile [unoptimized + debuginfo] target(s)
```

✅ **No errors, no warnings**

---

## MARKED FOR DELETION

The following are marked for future deletion when V1 is fully removed:

1. `core/src/agent/v1/` folder - Entire V1 implementation
2. `core/src/agent/v1/mod.rs` - V1 module
3. Exports in `agent/mod.rs`:
   - `pub use v1::{Agent, AgentConfig, AgentDecision};`

---

## NEXT STEPS (When Ready)

To completely remove V1:

1. Delete `core/src/agent/v1/` folder
2. Remove `pub mod v1;` from `agent/mod.rs`
3. Remove `pub use v1::...` from `agent/mod.rs`
4. Update `core/src/lib.rs` to remove V1 re-exports
5. Update any remaining code that uses V1 types

---

## ARCHITECTURE SUMMARY

```
┌─────────────────────────────────────────────────────────────┐
│                        TERMINAL UI                          │
│                   (src/terminal/mod.rs)                     │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                   AgentOrchestrator                         │
│              (agent::v2::orchestrator)                      │
│  - Manages chat session                                     │
│  - Polls for worker events                                  │
│  - Handles user input / agent response loop                 │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                      AgentV2                                │
│                  (agent::v2::core)                          │
│  - Executes one step at a time                              │
│  - Parses LLM responses                                     │
│  - Executes tools                                           │
└──────────────────────┬──────────────────────────────────────┘
                       │
           ┌───────────┴───────────┐
           │                       │
           ▼                       ▼
┌──────────────────┐    ┌──────────────────┐
│   DelegateTool   │    │   Other Tools    │
│                  │    │                  │
│  Spawns workers  │    │  Shell, Memory,  │
│  with shared     │    │  FileSystem, etc │
│  scratchpad      │    │                  │
└────────┬─────────┘    └──────────────────┘
         │
         ▼
┌──────────────────┐
│  Worker AgentV2  │
│  (background)    │
└──────────────────┘
```

---

## CONCLUSION

✅ **V2 is now working** with clean module structure  
✅ **V1 is isolated** in `v1/` folder, ready for deletion when desired  
✅ **Shared components** are at `agent/` root level  
✅ **All imports fixed** across the codebase  

The codebase is now organized and compiles successfully.
