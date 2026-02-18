# MyLM Architecture Analysis

## Current Issues Identified

### 1. Cognition / Contract API Confusion

**The Problem:**
There are TWO competing cognitive APIs in the codebase:

#### OLD API: CognitiveEngine (cognition/)
```rust
pub trait CognitiveEngine {
    fn step(&mut self, state: &AgentState, input: Option<InputEvent>) 
        -> Result<Transition, CognitiveError>;
}
```
- **Single-step**, sequential, deterministic
- No async, no IO
- Returns ONE decision at a time
- Clean and simple

#### NEW API: AgencyKernel (contract/)
```rust
pub trait AgencyKernel {
    fn process(&mut self, events: &[KernelEvent]) -> Result<IntentGraph, KernelError>;
    fn state(&self) -> &AgentState;
}
```
- **Batch processing** with `IntentGraph` (DAG of parallel intents)
- Tries to parallelize cognition internally
- Adds unnecessary complexity

**Why the New API is Wrong:**
- **Parallelism should be at session level**, not cognition level
- Each session has its own (sync) cognition + (async) runtime
- Multiple sessions run in parallel via tokio tasks
- Cognition being single-threaded is a FEATURE (deterministic, debuggable)

**The Correct Architecture:**
```
┌─────────────────────────────────────────┐
│         Async Runtime (Tokio)           │
│                                         │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐   │
│  │Session 1│ │Session 2│ │Session 3│   │
│  │┌───────┐│ │┌───────┐│ │┌───────┐│   │
│  ││Runtime││ ││Runtime││ ││Runtime││   │
│  ││ ASYNC ││ ││ ASYNC ││ ││ ASYNC ││   │
│  │└───┬───┘│ │└───┬───┘│ │└───┬───┘│   │
│  │    │    │ │    │    │ │    │    │   │
│  │┌───┴───┐│ │┌───┴───┐│ │┌───┴───┐│   │
│  ││Cognit.││ ││Cognit.││ ││Cognit.││   │
│  ││ SYNC  ││ ││ SYNC  ││ ││ SYNC  ││   │
│  ││ No IO ││ ││ No IO ││ ││ No IO ││   │
│  │└───────┘│ │└───────┘│ │└───────┘│   │
│  └─────────┘ └─────────┘ └─────────┘   │
│                                         │
│  Parallelism = Multiple Sessions        │
│  (Each session has sequential cognition)│
└─────────────────────────────────────────┘
```

**Bridge Code:**
- `cognition/kernel_adapter.rs` (332 lines) bridges OLD → NEW API
- Should not exist - the NEW API shouldn't exist either

---

### 2. File Locations (Fixed)

#### Config Module Reorganization ✅ DONE
```
Before:
  config/
    ├── types.rs      # Core types (renamed to base.rs)
    ├── store.rs      # New Config (renamed to unified.rs)
    ├── llm.rs        # Legacy ConfigV2 (DELETED)
    ├── tests.rs      # Tests for legacy (DELETED)
    └── ...

After:
  config/
    ├── mod.rs        # Exports
    ├── base.rs       # Provider, SearchProvider, ConfigError, etc.
    ├── unified.rs    # Main Config (profiles, providers, app settings)
    ├── app.rs        # AppConfig, FeatureConfig, Theme
    ├── profile.rs    # ProfileConfig, ResolvedProfile
    ├── provider.rs   # ProviderConfig, ProviderType
    ├── legacy.rs     # Minimal ConfigV2 for migration only
    ├── agent.rs      # AgentConfig (tool, retry, memory settings)
    ├── manager.rs    # ConfigManager with hot-reload
    ├── bridge.rs     # Config → LLM/Agent config conversion
    └── prompt_schema.rs  # Prompt configuration types
```

#### Parser Module Reorganization ✅ DONE
```
Before:
  cognition/
    ├── parser/       # Response parsers (WRONG LOCATION)
    └── llm_engine.rs # Duplicate parsing logic

After:
  types/
    └── parser/       # Response parsers (CORRECT - data transformation)
  cognition/
    └── llm_engine.rs # Uses parser from types::parser
```

**Rationale:** Parser converts string → structured type, belongs in `types/`

---

### 3. Naming Collisions (Fixed)

#### WorkerHandle Confusion ✅ FIXED
| Name | Location | Purpose |
|------|----------|---------|
| `WorkerSpawnHandle` | `runtime/capability.rs` | Minimal handle from spawn (just `id`) |
| `JobHandle` | `runtime/workers.rs` | Job tracking handle (`id: JobId` + `status`) |
| `WorkerHandle` | `worker.rs` | Full worker handle with `result_rx` |

---

### 4. Code Duplication (Fixed)

#### llm_engine.rs ✅ CLEANED UP
**Removed:**
- Duplicate `ShortKeyAction` struct
- `parse_short_key_action()` function
- `parse_kimi_xml_tool_call()` function
- `parse_user_response()` function
- `extract_json_objects()` function
- `ResponseParser` struct

**Now uses:**
```rust
use crate::agent::types::parser::{ShortKeyParser, ParsedResponse};
```

---

### 5. Test File Organization (Fixed)

```
Before:
  agent/
    ├── example_integration.rs  # Test file in main dir
    ├── integration_tests.rs    # Test file in main dir
    └── test_architecture.rs    # Test file in main dir

After:
  agent/
    └── tests/
        ├── example_integration.rs
        ├── integration_tests.rs
        └── test_architecture.rs
```

---

## Module Status

| Module | Status | Notes |
|--------|--------|-------|
| `config/` | ✅ Clean | Reorganized into logical submodules |
| `agent/cognition/` | ✅ Clean | Parser moved out, llm_engine cleaned |
| `agent/types/` | ✅ Clean | Now includes parser |
| `agent/contract/` | ⚠️ Over-engineered | IntentGraph adds unnecessary complexity |
| `agent/runtime/` | TBD | Needs review |
| `agent/session/` | TBD | Needs review |
| `agent/commonbox.rs` | TBD | Large file (1579 lines), but used |
| `agent/identity.rs` | ✅ Clean | Well documented, comprehensive tests |

---

## Key Architectural Principles

1. **Cognition is pure** - No async, no IO, no network, deterministic
2. **Runtime handles side effects** - All async operations, file/network IO
3. **Parser is data transformation** - String → Structured types, belongs in `types/`
4. **Parallelism at session level** - Each session has its own (sync) cognition
5. **No batch processing in cognition** - One event → One step → One decision

---

## Open Questions

1. Should `contract/` module be simplified or removed?
   - The `AgencyKernel` with `IntentGraph` seems over-engineered
   - `CognitiveEngine` pattern is simpler and sufficient

2. `kernel_adapter.rs` is temporary bridge code
   - Only needed if keeping both APIs
   - Should be removed once architecture is unified

3. `commonbox.rs` is large (1579 lines)
   - But it's actively used for multi-agent state
   - May need splitting in future

---

## Build Status

- ✅ All modules compile
- ✅ 112 tests passing
- ✅ No errors, only minor warnings
