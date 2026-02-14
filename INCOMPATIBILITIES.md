# Incompatibilities Between Old TUI/Hub and V3 Architecture

## Document Purpose
This file lists every incompatibility found when trying to link the old TUI/hub code to the current codebase. This will guide the rewrite of the hub/configuration menu + TUI to work with V3.

---

## 1. AGENT MODULE STRUCTURE

### 1.1 Agent Module Location
**Old Code:**
```rust
use mylm_core::agent::{Agent, AgentDecision, ToolKind};
use mylm_core::agent::v2::jobs::JobRegistry;
```

**Current State:**
- `mylm_core::agent` exists but exports have changed
- `AgentDecision` is now exported from both `v1` and `v2::protocol`
- `JobRegistry` is now wrapped in `Arc<JobRegistry>` throughout

### 1.2 AgentDecision Enum
**Old (V1):**
```rust
pub enum AgentDecision {
    MalformedAction(String),
    Message(String, TokenUsage),
    Action { tool: String, args: String, kind: ToolKind },
    Error(String),
    Stall { reason: String },  // MISSING IN CURRENT MATCH ARMS
}
```

**Current (V2 protocol):**
```rust
pub enum AgentDecision {
    MalformedAction(String),
    Message(String, TokenUsage),
    Action { tool: String, args: String, kind: ToolKind },
    Error(String),
    // Note: Stall variant exists in V1 but match arms don't handle it
}
```

**Issue:** `src/terminal/app.rs:1144` - match on `step_res` doesn't handle `Stall` variant

---

## 2. LLM CONFIGURATION

### 2.1 LlmConfig::new Signature
**Old (expected by terminal/mod.rs:59):**
```rust
LlmConfig::new(
    provider: LlmProvider,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> Self
```

**Current (in llm/mod.rs:55):**
```rust
pub fn new(
    provider: LlmProvider,
    base_url: String,
    model: String,
    api_key: Option<String>,
    max_context_tokens: usize,  // NEW REQUIRED ARGUMENT
) -> Self
```

**Issue:** Terminal code passes 4 args, function expects 5

### 2.2 ResolvedConfig.max_context_tokens
**Old:** Was `Option<usize>` with `unwrap_or()`
**Current:** Plain `usize` (not Optional)

**Issue:** `src/terminal/mod.rs:64` - tries to call `.unwrap_or(8192)` on `usize`

---

## 3. SYSTEM PROMPT BUILDING

### 3.1 build_system_prompt Signature
**Old (expected by terminal/mod.rs:126):**
```rust
pub async fn build_system_prompt(
    context: &TerminalContext,
    prompt_name: &str,
    mode: Option<&str>,
    customization: Option<serde_json::Value>,
) -> anyhow::Result<String>
```

**Current (in config/mod.rs):**
```rust
pub async fn build_system_prompt(
    _context: &crate::context::TerminalContext,
    _prompt_name: &str,
    _mode: Option<&str>,
    _prompts: Option<&crate::config::v2::PromptsConfig>,      // NEW
    _tools: Option<&[Arc<dyn crate::agent::Tool>]>,           // NEW
    _agent_config: Option<&crate::config::v2::AgentConfig>,   // NEW
) -> anyhow::Result<String>
```

**Issue:** Terminal code passes 4 args, function expects 6

---

## 4. TOOL CONSTRUCTORS

### 4.1 ShellTool::new
**Old (expected by terminal/mod.rs:142):**
```rust
ShellTool::new(
    executor: Arc<CommandExecutor>,
    context: TerminalContext,
    event_tx: UnboundedSender<TuiEvent>,  // WRONG TYPE
    memory_store: Option<Arc<VectorStore>>,
    categorizer: Option<Arc<MemoryCategorizer>>,
    permissions: Option<AgentPermissions>,
    job_registry: Option<Arc<JobRegistry>>,
)
```

**Current (in agent/tools/shell.rs:48):**
```rust
pub fn new(
    executor: Arc<CommandExecutor>,
    context: TerminalContext,
    terminal: Arc<dyn TerminalExecutor>,  // EXPECTS TerminalExecutor, NOT event_tx
    memory_store: Option<Arc<VectorStore>>,
    categorizer: Option<Arc<MemoryCategorizer>>,
    permissions: Option<AgentPermissions>,
    job_registry: Option<Arc<JobRegistry>>,
    shell_permissions: Option<AgentPermissions>,  // NEW 8TH ARGUMENT
) -> Self
```

**Issues:**
1. 3rd arg: Terminal passes `event_tx` (UnboundedSender), tool expects `Arc<dyn TerminalExecutor>`
2. Missing 8th argument `shell_permissions`

### 4.2 WebSearchTool::new
**Old (expected by terminal/mod.rs:144):**
```rust
WebSearchTool::new(
    config: WebSearchConfig,
    event_tx: UnboundedSender<TuiEvent>,  // EXTRA ARGUMENT
) -> Self
```

**Current (in agent/tools/web_search.rs:18):**
```rust
pub fn new(config: WebSearchConfig) -> Self
```

**Issue:** Terminal passes 2 args, function expects 1

### 4.3 CrawlTool::new
**Old (expected by terminal/mod.rs:145):**
```rust
CrawlTool::new(event_tx: UnboundedSender<TuiEvent>)
```

**Current (in agent/tools/crawl.rs:10):**
```rust
pub fn new(_event_bus: Arc<EventBus>) -> Self
```

**Issue:** Terminal passes `UnboundedSender<TuiEvent>`, tool expects `Arc<EventBus>`

### 4.4 DelegateTool::new
**Old (expected by terminal/mod.rs:148):**
```rust
DelegateTool::new(
    llm_client: Arc<LlmClient>,
    scribe: Arc<Scribe>,
    job_registry: JobRegistry,  // NOT Arc WRAPPED
    memory_store: Option<Arc<VectorStore>>,
    categorizer: Option<Arc<MemoryCategorizer>>,
    permissions: Option<AgentPermissions>,
)
```

**Current signature unknown** - but likely changed significantly

---

## 5. CONFIGURATION FUNCTIONS

### 5.1 get_prompts_dir
**Status:** DOES NOT EXIST in current codebase
**Used by:** src/main.rs:266
**Purpose:** Get directory path for prompt files

### 5.2 load_prompt
**Status:** DOES NOT EXIST in current codebase
**Used by:** src/main.rs:267
**Purpose:** Load a prompt file by name

---

## 6. AGENT OVERRIDE DEFAULT

### 6.1 AgentOverride struct initialization
**Old (in hub.rs:745):**
```rust
profile.agent = Some(mylm_core::config::AgentOverride {
    max_iterations: current_agent.max_iterations,
    iteration_rate_limit: current_agent.iteration_rate_limit,
    main_model: current_agent.main_model,
    worker_model: worker_model.clone(),
});
```

**Current struct has 16 fields:**
- max_iterations
- iteration_rate_limit
- main_model
- worker_model
- max_context_tokens
- input_price
- output_price
- condensation_threshold
- permissions
- main_rpm
- workers_rpm
- worker_limit
- rate_limit_tier
- max_actions_before_stall
- max_consecutive_messages
- max_recovery_attempts
- max_tool_failures

**Fix Applied:** Added `..Default::default()` to use default for missing fields

---

## 7. PACORE MODULE

### 7.1 pacore::exp::Exp
**Old:** `use mylm_core::pacore::exp::Exp;`
**Current:** Module exists at `agent::v2::orchestrator::pacore`

**Status:** Re-export added to lib.rs

### 7.2 pacore::PaCoReProgressEvent
**Used by:** terminal/app.rs:1423

### 7.3 pacore::ChatClient
**Used by:** terminal/app.rs:1404, main.rs:331, main.rs:372

### 7.4 pacore::model::Message
**Used by:** terminal/app.rs:1483

---

## 8. TYPE MISMATCHES IN AGENT WRAPPER

### 8.1 JobRegistry wrapping
**Old:** `JobRegistry` (bare struct)
**Current:** `Arc<JobRegistry>` (wrapped in Arc)

**Files affected:**
- core/src/agent/v2/core.rs:64 - struct field
- core/src/agent/wrapper.rs:220 - function return type
- core/src/agent/v2/orchestrator/types.rs:182 - struct field
- core/src/agent/v2/orchestrator/mod.rs:65, 83 - local variable and struct init
- core/src/agent/v2/orchestrator/loops.rs:27, 240, 501 - function parameters

---

## 9. UNSAFE CODE

### 9.1 AgentWrapper Send/Sync impls
**Location:** core/src/agent/wrapper.rs:493-494
```rust
unsafe impl Send for AgentWrapper {}
unsafe impl Sync for AgentWrapper {}
```

**Issue:** lib.rs has `#![deny(unsafe_code)]` at crate root
**Fix Applied:** Added `#![allow(unsafe_code)]` to wrapper.rs

---

## 10. LOGGING MACROS

### 10.1 Duplicate macro definitions
**Issue:** core/src/lib.rs defined debug_log, info_log, warn_log, error_log
**Also defined in:** core/src/agent/logger.rs (but removed)

**Status:** Fixed by consolidating macros in lib.rs

---

## 11. FACTORY MODULE

### 11.1 BuiltAgent re-export
**Old:** `mylm_core::BuiltAgent`
**Current:** Defined in `agent::v2::driver::factory::BuiltAgent`

**Fix Applied:** Added re-export in lib.rs

---

## 12. SERVER MODULE

### 12.1 AgentDecision import
**Old:** `use mylm_core::agent::core::AgentDecision;`
**Current:** `agent::core` module created that re-exports `v2::protocol::AgentDecision`

---

## SUMMARY OF FILES REQUIRING CHANGES

### High Impact (Major structural changes needed):
1. `src/terminal/mod.rs` - Tool instantiation, LlmConfig calls
2. `src/terminal/app.rs` - AgentDecision match arms, tool usage
3. `src/cli/hub.rs` - AgentOverride initialization (partially fixed)
4. `src/main.rs` - Prompt loading functions, pacore imports

### Medium Impact (Import/Export fixes):
5. `core/src/lib.rs` - Re-exports (mostly done)
6. `core/src/agent/mod.rs` - Core module for AgentDecision (done)
7. `core/src/config/mod.rs` - build_system_prompt signature, helper functions (done)
8. `core/src/agent/wrapper.rs` - Unsafe code allowance (done)

### Already Fixed:
- JobRegistry Arc wrapping
- Logging macros
- AgentOverride Default
- Pacore module re-export
- BuiltAgent re-export
- AgentDecision core module

---

## NEXT STEPS FOR REWRITE

1. **Design V3-compatible hub menu** - Same user-facing features, but using V3 APIs
2. **Design V3-compatible TUI** - Same UX, but using Session/Runtime/Cognition layers
3. **Port configuration settings** - Provider management, model selection, web search, agent settings
4. **Create V3 tool wrappers** - If needed to bridge old tool expectations

---

*Generated: 2026-02-12*
*Total incompatibilities found: 12 major categories, 25+ specific issues*
