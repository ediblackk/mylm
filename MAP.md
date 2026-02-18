# MyLM Core Agent Architecture Map

> Generated from full analysis of `/home/edward/workspace/personal/mylm/core/src/agent`
> Date: 2026-02-17

## Overview

The agent module implements a **layered architecture** with strict separation of concerns:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ LAYER 4: SESSION          Orchestration (async)                             │
│  - Session: Main event loop, coordinates all layers                         │
│  - Input handlers: Chat, Task, Worker                                       │
│  - Persistence: Session save/load                                           │
├─────────────────────────────────────────────────────────────────────────────┤
│ LAYER 3: RUNTIME          Async capability execution (side effects)         │
│  - ContractRuntime: Decision interpreter                                    │
│  - CapabilityGraph: Trait-based capability container                        │
│  - Tools: ToolRegistry with 8+ tools                                        │
│  - LLM: LlmClientCapability                                                 │
│  - Approval: Terminal/Auto-approve                                          │
├─────────────────────────────────────────────────────────────────────────────┤
│ LAYER 2: COGNITION        Pure state machine (NO async/IO)                  │
│  - CognitiveEngine: (state, input) -> Transition                            │
│  - LLMBasedEngine: Prompt construction, response parsing                    │
│  - AgentState: Immutable snapshot                                           │
│  - Parser: Short-Key JSON protocol                                          │
├─────────────────────────────────────────────────────────────────────────────┤
│ LAYER 1: TYPES            Primitive types (no dependencies)                 │
│  - IDs: TaskId, JobId, SessionId, IntentId                                  │
│  - Events: ToolResult, LLMResponse, etc.                                    │
│  - Intents: ToolCall, LLMRequest                                            │
├─────────────────────────────────────────────────────────────────────────────┤
│ CONTRACT                  Stable interfaces between layers                  │
│  - AgencyKernel: Pure kernel trait                                          │
│  - AgencyRuntime: Async runtime trait                                       │
│  - EventTransport: Pluggable event queue                                    │
│  - IntentGraph: Dynamic DAG expansion                                       │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Dependency Rules

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
- **contract**: Used by all layers for stable interfaces

---

## Directory Structure

```
core/src/agent/
├── mod.rs                          # Main module exports
├── builder.rs                      # AgentBuilder for construction
├── factory.rs                      # AgentSessionFactory from Config
├── worker.rs                       # Worker management
├── 
├── types/                          # LAYER 1: Primitive types
│   ├── mod.rs
│   ├── ids.rs                      # TaskId, JobId, SessionId, IntentId
│   ├── intents.rs                  # ToolCall, LLMRequest
│   ├── events.rs                   # ToolResult, LLMResponse
│   ├── observations.rs             # Observation types
│   └── common.rs                   # TokenUsage, Approval
│
├── cognition/                      # LAYER 2: Pure logic (NO async/IO)
│   ├── mod.rs
│   ├── engine.rs                   # CognitiveEngine trait
│   ├── llm_engine.rs               # LLMBasedEngine implementation
│   ├── state.rs                    # AgentState (immutable)
│   ├── decision.rs                 # AgentDecision, Transition
│   ├── input.rs                    # InputEvent enum
│   ├── error.rs                    # CognitiveError
│   ├── history.rs                  # Message history
│   ├── kernel_adapter.rs           # Bridges to AgencyKernel contract
│   └── parser/                     # Response parsing
│       ├── mod.rs                  # ParsedResponse, ParseError
│       └── short_key.rs            # Short-Key JSON protocol
│
├── runtime/                        # LAYER 3: Async + side effects
│   ├── mod.rs
│   ├── capability.rs               # Capability traits (LLM, Tools, etc.)
│   ├── context.rs                  # RuntimeContext
│   ├── runtime.rs                  # AgentRuntime
│   ├── contract_runtime.rs         # ContractRuntime implementation
│   ├── graph.rs                    # CapabilityGraph
│   ├── error.rs                    # RuntimeError
│   ├── llm.rs                      # LLMCapability trait
│   ├── approval.rs                 # ApprovalCapability trait
│   ├── workers.rs                  # WorkerCapability trait
│   ├── terminal.rs                 # TerminalExecutor trait
│   │
│   ├── impls/                      # Capability implementations
│   │   ├── mod.rs
│   │   ├── llm_client.rs           # LlmClientCapability
│   │   ├── tool_registry.rs        # ToolRegistry (8+ tools)
│   │   ├── terminal_approval.rs    # TerminalApprovalCapability
│   │   ├── local_worker.rs         # LocalWorkerCapability
│   │   ├── console_telemetry.rs    # ConsoleTelemetry
│   │   ├── web_search.rs           # WebSearchCapability
│   │   ├── memory.rs               # MemoryCapability
│   │   ├── vector_store.rs         # VectorStore implementations
│   │   ├── in_memory_transport.rs  # InMemoryTransport
│   │   ├── dag_executor.rs         # DagExecutor
│   │   ├── retry.rs                # Retry wrappers
│   │   └── local.rs                # Local runtime
│   │
│   ├── tools/                      # Tool implementations
│   │   ├── mod.rs                  # ToolRegistry
│   │   ├── shell.rs                # Shell tool
│   │   ├── fs.rs                   # File read/write
│   │   ├── list_files.rs           # Directory listing
│   │   ├── git.rs                  # Git tools
│   │   ├── web_search.rs           # Web search (DuckDuckGo, etc.)
│   │   └── memory.rs               # Memory tool
│   │
│   ├── llm/                        # LLM-related
│   │   └── (empty or minimal)
│   │
│   └── approval/                   # Approval-related
│       └── (empty or minimal)
│
├── session/                        # LAYER 4: Orchestration
│   ├── mod.rs
│   ├── session.rs                  # Session struct, main loop
│   ├── persistence.rs              # Session save/load
│   └── input/                      # Input handlers
│       ├── mod.rs
│       ├── chat.rs                 # Chat input
│       ├── task.rs                 # Task input
│       └── worker.rs               # Worker input
│
├── contract/                       # Stable interfaces
│   ├── mod.rs
│   ├── kernel.rs                   # AgencyKernel trait
│   ├── runtime.rs                  # AgencyRuntime trait
│   ├── transport.rs                # EventTransport trait
│   ├── session.rs                  # Session trait
│   ├── ids.rs                      # IntentId, NodeId, etc.
│   ├── events.rs                   # KernelEvent
│   ├── intents.rs                  # Intent, IntentNode
│   ├── observations.rs             # Observation
│   ├── graph.rs                    # IntentGraph
│   ├── config.rs                   # KernelConfig
│   └── envelope.rs                 # KernelEventEnvelope
│
├── memory/                         # Memory subsystem
│   ├── mod.rs
│   ├── manager.rs                  # AgentMemoryManager
│   ├── extraction.rs               # Memory extraction
│   └── (types re-exported from impls)
│
└── (test files)
    ├── test_architecture.rs
    ├── example_integration.rs
    └── integration_tests.rs
```

---

## Key Files Analysis

### Core Module (`mod.rs`)
- **Purpose**: Central exports and documentation
- **Key Rules**: Cognition is pure, runtime handles side effects
- **Re-exports**: All public types from submodules

### Builder (`builder.rs`)
- **Purpose**: Fluent API for constructing agents
- **Pattern**: Builder pattern with `with_*` methods
- **Creates**: `AgentRuntime` with `CapabilityGraph`

### Factory (`factory.rs`)
- **Purpose**: Create sessions from unified Config
- **Key Function**: `create_session(profile_name)`
- **Integrates**: LLM client, ToolRegistry, memory, terminal
- **Creates**: `AgencySession` with `CognitiveEngineAdapter`

### Cognition Engine (`cognition/llm_engine.rs`)
- **Purpose**: Pure cognitive logic - prompt building, response parsing
- **Key Struct**: `LLMBasedEngine`
- **Methods**: `build_full_prompt()`, `parse_response()`
- **Architecture Violation**: Has `memory_provider` field with side effects!

### Tool Registry (`runtime/tools/mod.rs`)
- **Purpose**: Dynamic tool management
- **Tools**: shell, read_file, write_file, list_files, search, git_*, web_search
- **Method**: `descriptions()` for dynamic prompt generation

### Contract Runtime (`runtime/contract_runtime.rs`)
- **Purpose**: Bridge contract traits to V3 capabilities
- **Implements**: `AgencyRuntime`
- **Contains**: ToolRegistry, LLM client, workers, approval

---

## Critical Observations

### 1. Architecture Violation in Cognition
**File**: `cognition/llm_engine.rs`

```rust
pub struct LLMBasedEngine {
    system_prompt: String,
    memory_provider: Option<Arc<dyn MemoryProvider>>,  // ❌ Side effects!
    tool_descriptions: Vec<ToolDescription>,           // ✅ Pure data
}
```

**Issue**: `MemoryProvider` has `remember()` method that causes side effects.
**Impact**: Violates "cognition is pure" rule.
**Recommendation**: Move remember handling to `kernel_adapter.rs` or runtime layer.

### 2. Hardcoded Prompt Template
**File**: `cognition/llm_engine.rs` - `build_system_prompt()`

The function has a hardcoded prompt template with tool descriptions embedded.
This is partially mitigated by dynamic tool descriptions passed via `with_tool_descriptions()`,
but the base template still has hardcoded examples and format descriptions.

**Recommendation**: Move entire prompt generation to use dynamic templates.

### 3. Duplicate ToolRegistry Definitions
**Files**: 
- `runtime/tools/mod.rs` - Main ToolRegistry
- `runtime/impls/tool_registry.rs` - Alternative implementation

Two different `ToolRegistry` types exist with similar functionality.
Factory imports from `runtime::tools` but there's also one in `impls`.

### 4. Unused Functions
**File**: `cognition/llm_engine.rs`

```rust
fn format_tools() -> String  // ❌ Never used
```

Dead code from refactoring.

### 5. Short-Key Protocol "r" Field
**File**: `cognition/parser/short_key.rs`

The "r" (remember) field is parsed but the handling in `llm_engine.rs` causes
side effects within the pure cognition layer.

**Current flow**:
1. LLM sends `{"r": "content"}`
2. `parse_response()` calls `provider.remember()` ❌ Side effect in pure layer!

**Recommended flow**:
1. LLM sends `{"r": "content"}`
2. `parse_response()` returns `AgentDecision::Remember(content)`
3. `kernel_adapter` converts to intent
4. Runtime handles the actual remember operation

### 6. ✅ APPROVAL SYSTEM ARCHITECTURE (FIXED)
**Files**: 
- `cognition/llm_engine.rs` - Removed hardcoded `requires_approval()`
- `runtime/contract_runtime.rs` - Added approval policy check
- `config/store.rs` - Per-profile approval settings
- `src/tui/approval.rs` - Auto-approve integration

**OLD BUG**: Cognition layer had hardcoded approval logic that bypassed user settings.

**NEW ARCHITECTURE**:
```
┌─────────────────────────────────────────────────────────────┐
│  CONFIG (per-profile)                                       │
│  ├─ auto_approve: bool          (Ctrl+A toggle)             │
│  ├─ always_allow: Vec<String>   (light tools)               │
│  ├─ always_restrict: Vec<String>(never auto-approved)       │
│  └─ main_agent vs worker: separate configs                  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│  COGNITION (pure)                                           │
│  ├─ Parse LLM response → AgentDecision::CallTool            │
│  └─ If LLM sends {"c": true} → RequestApproval (explicit)   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│  RUNTIME (side effects)                                     │
│  ├─ Check approval_policy:                                  │
│  │   1. IF tool in always_restrict → RequestApproval        │
│  │   2. ELIF tool in always_allow → Execute                 │
│  │   3. ELIF auto_approve ON → Execute                      │
│  │   4. ELSE → RequestApproval                              │
│  └─ ApprovalCapability checks auto_approve flag             │
└─────────────────────────────────────────────────────────────┘
```

**Configuration Example**:
```toml
[profiles.default.permissions.main_agent]
always_allow = ["read_file", "list_files", "git_status"]
always_restrict = ["shell", "write_file", "rm", "sudo"]

[profiles.worker.permissions.main_agent]
always_allow = []
always_restrict = ["shell", "rm", "sudo"]
```

**TUI Integration**: Ctrl+A toggles shared `Arc<AtomicBool>` that both TUI and approval capability check.

---

## Tool System Architecture

### ToolRegistry (`runtime/tools/mod.rs`)

```rust
pub struct ToolRegistry {
    shell: ShellTool,
    read_file: ReadFileTool,
    write_file: WriteFileTool,
    list_files: ListFilesTool,
    git_status: GitStatusTool,
    git_log: GitLogTool,
    git_diff: GitDiffTool,
    web_search: WebSearchTool,
    memory: Option<MemoryTool>,
    terminal: Arc<dyn TerminalExecutor>,
}
```

### Tool Descriptions (Dynamic)

```rust
pub fn descriptions(&self) -> Vec<ToolDescription> {
    vec![
        ToolDescription { name: "shell", description: "...", usage: "..." },
        ToolDescription { name: "read_file", description: "...", usage: "..." },
        ToolDescription { name: "web_search", description: "...", usage: "..." },
        // ...
    ]
}
```

---

## Session Flow

```
User Input
    │
    ▼
┌─────────────────┐
│ Session         │  (async orchestration)
│                 │
│ 1. Convert to   │
│    KernelEvent  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Kernel Adapter  │  (adapter pattern)
│                 │
│ 2. Convert to   │
│    InputEvent   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Cognition Engine│  (pure, no async)
│                 │
│ 3. Build prompt │
│ 4. Parse response│
│ 5. Return       │
│    AgentDecision│
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Kernel Adapter  │
│                 │
│ 6. Convert to   │
│    Intent       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Runtime         │  (async side effects)
│                 │
│ 7. Execute tools│
│ 8. Call LLM     │
│ 9. Emit events  │
└─────────────────┘
```

---

## Recommendations

### High Priority

1. **Remove side effects from cognition layer**
   - Remove `memory_provider` from `LLMBasedEngine`
   - Add `AgentDecision::Remember` variant
   - Handle remember in adapter or runtime

2. **Clean up dead code**
   - Remove unused `format_tools()` function
   - Consolidate duplicate ToolRegistry implementations

3. **Fully dynamic prompts**
   - Move all prompt generation to use dynamic tool descriptions
   - Remove hardcoded examples from `build_system_prompt()`

### Medium Priority

4. **Documentation**
   - Add MOD.md files to each subdirectory as referenced in `mod.rs`
   - Document the Short-Key JSON protocol fully

5. **Testing**
   - Add unit tests for `parse_response()` with "r" field
   - Test that cognition layer has no side effects

### Low Priority

6. **Code organization**
   - Move `ToolRegistry` to single location (not both `tools/` and `impls/`)
   - Consider moving `kernel_adapter` to contract layer

---

## Current Compilation Status

✅ **Compiling** (with warnings)

**Warnings**:
- `format_tools()` is never used
- `copy_visible_conversation_to_clipboard()` is never used

---

## File Count Summary

| Module | File Count | Key Files |
|--------|------------|-----------|
| types | 6 | ids.rs, intents.rs, events.rs |
| cognition | 11 | llm_engine.rs, engine.rs, parser/short_key.rs |
| runtime | 25+ | contract_runtime.rs, tools/mod.rs, impls/*.rs |
| session | 6 | session.rs, persistence.rs, input/*.rs |
| contract | 11 | kernel.rs, runtime.rs, events.rs |
| memory | 3 | manager.rs, extraction.rs |
| **Total** | **~62** | - |
