Now I have read all the required files. Let me compile a comprehensive architecture audit report.

---

# Comprehensive Architecture Audit Report: mylm

## File-by-File Analysis

### 1. `Cargo.toml` (Root)
- **Lines**: 123
- **Purpose**: Workspace manifest for the mylm binary crate
- **Key exports**: N/A (manifest file)
- **Dependencies**: mylm-core (local), clap, tokio, reqwest, sysinfo, serde family, regex, urlencoding, futures, terminal UI libs (ratatui, crossterm, console, dialoguer, inquire), anyhow, thiserror, dirs, home, chrono, base64, uuid, tokio-tungstenite, arboard
- **Issues spotted**: Clean manifest. No architectural issues.

---

### 2. `core/Cargo.toml`
- **Lines**: 49
- **Purpose**: Manifest for the mylm-core library crate
- **Key exports**: N/A (manifest file)
- **Dependencies**: Extensive list including tokio, reqwest, config, lancedb, fastembed, arrow-*, git2, serde family, toml, tera, tracing, lazy_static, uuid, regex, urlencoding, futures, shell-words, anyhow, thiserror, dirs, home, chrono, hostname, arboard, portable-pty, vt100, dialoguer, console, crossterm, ignore, glob
- **Issues spotted**: 
  - Heavy dependencies (lancedb, fastembed, arrow-*) suggest core is doing more than just agent logic (embedding/vector storage concerns)
  - Terminal-specific dependencies (portable-pty, vt100, crossterm) in **core** violate layer separation - these should be in terminal layer only

---

### 3. `core/src/lib.rs`
- **Lines**: 39
- **Purpose**: Core library public API surface
- **Key exports**: 
  - Modules: agent, error, pacore, terminal, config, context, executor, llm, memory, output, scheduler, state, protocol, factory, util, rate_limiter
  - Re-exports: `Agent`, `AgentV2`, `BuiltAgent`, `Config`, `MylmError`, `Result`, `MemoryStore`
- **Dependencies**: All internal modules
- **Issues spotted**:
  - **Core exports `terminal` module** - this is a major boundary violation. The terminal layer should depend on core, not vice versa.
  - The `terminal` module in core appears to be a duplicate implementation (see `core/src/terminal/` exists). This suggests confusion about where terminal code lives.

---

### 4. `core/src/agent/mod.rs`
- **Lines**: 57
- **Purpose**: Agent module root - re-exports all agent-related types
- **Key exports**: 
  - Submodules: tool, tools, core, v2, logger, protocol, event, event_bus, tool_registry, factory, toolcall_log, permissions, execution, context, role, workspace, traits, orchestrator, wait, budget, reasoning
  - Re-exports: `Tool`, `ToolKind`, `Agent`, `AgentDecision`, `AgentConfig`, `AgentV2`, `AgentRequest`, `AgentResponse`, `AgentError`, `CoreEvent`, `EventBus`, `ToolRegistry`, `BuiltAgent`, `AgentBuilder`, `AgentConfigs`, execution types, context types, `AgentRole`, `AgentOrchestrator`, `SmartWaitController`, `BudgetTracker`, reasoning engines
- **Dependencies**: All agent submodules
- **Issues spotted**:
  - The module structure is **extremely large** and lacks clear orthogonality. Too many concerns bundled together.
  - `execution` module exists in core/agent but is also partially implemented in `core/agent/core.rs` (the `run` method). DRY violation.
  - `traits` module defines `TerminalExecutor` - this is good for decoupling, but the trait lives in core while implementation is in terminal layer. However, core also imports terminal types (see below).

---

### 5. `core/src/agent/orchestrator.rs`
- **Lines**: 978
- **Purpose**: Centralized agent execution management (recently migrated from terminal layer)
- **Key exports**: `TaskId`, `TaskHandle`, `OrchestratorConfig`, `ExecutionState`, `AgentOrchestrator`
- **Dependencies**: 
  - From core: `agent::event_bus`, `agent::traits::TerminalExecutor`, `agent::v2::jobs`, `agent::{Agent, AgentDecision, ToolKind}`, `agent::v2::AgentV2`, `llm::chat::ChatMessage`, `llm::TokenUsage`
  - External: std::sync, tokio::sync, tokio::time, uuid
- **Issues spotted**:
  - **7 warnings about unused variables** (as mentioned in task context). Specifically: `_job_registry`, `_job_id`, `_task` parameters in `run_agent_loop_v1`; also `active_worker_count` is computed but only used in V2 loop.
  - The orchestrator handles **both V1 and V2** agents, leading to massive code duplication: `run_agent_loop_v1` and `run_agent_loop_v2` are nearly identical (80% similar). This is a major DRY violation.
  - Both loops duplicate the job polling logic, tool execution branching, and event publishing.
  - The orchestrator knows about `TerminalExecutor` delegate but also directly uses `ToolRegistry` for non-terminal tools. This is a mixed abstraction level.
  - Memory recording logic (`store.record_command`) is duplicated in both V1 and V2 branches.
  - The orchestrator is too large (978 lines) and should be split.

---

### 6. `core/src/agent/execution.rs`
- **Lines**: 170
- **Purpose**: Tool execution utilities for the legacy Agent
- **Key exports**: 
  - Functions: `process_tool_args`, `execute_tool_with_registry`, `handle_tool_approval`, `emit_tool_observation`, `check_interrupt`, `build_continue_nudge`, `build_format_nudge`, `is_final_response`
- **Dependencies**: 
  - From core: `agent::tool::ToolKind`, `agent::tool_registry::ToolRegistry`, `terminal::app::TuiEvent`
  - External: std::sync, tokio::sync::mpsc
- **Issues spotted**:
  - **Imports `TuiEvent` from `terminal::app`** - this is a **critical boundary violation**. Core agent code should NOT depend on terminal UI types.
  - `handle_tool_approval` uses `TuiEvent` to communicate with UI - this couples core to terminal's event system. The approval flow should use the `EventBus` or a trait instead.
  - `emit_tool_observation` also sends `TuiEvent` - same coupling issue.
  - The functions here are used by `core.rs` (V1 agent) but the orchestrator reimplements similar logic. DRY violation.
  - `process_tool_args` is duplicated in orchestrator (lines 520-532, 816-828) - exact same JSON extraction logic.

---

### 7. `core/src/agent/core.rs`
- **Lines**: 1009
- **Purpose**: Legacy Agent (V1) implementation - the original agent loop
- **Key exports**: 
  - Structs: `AgentConfig`, `Agent` (with many fields), `AgentDecision` (enum)
  - Methods: `new_with_iterations`, `new_with_config`, `has_pending_decision`, `reset`, `step`, `run`, `run_v2_bridge`, `inject_memory_context_internal`, `handle_approval_flow`, `generate_system_prompt`, `get_system_prompt`, `get_tools_description`, `auto_categorize`, `condense_history`, `inject_memory_context`
- **Dependencies**: 
  - From core: many agent submodules, memory, terminal::app::TuiEvent, llm, etc.
  - External: std::error, std::sync, tokio::sync
- **Issues spotted**:
  - **Imports `TuiEvent`** - another boundary violation. The `run` method and `handle_approval_flow` use terminal events.
  - `run_v2_bridge` is marked `#[allow(dead_code)]` and appears unused. This is dead code from the V2 integration attempt.
  - `inject_memory_context_internal` is also `#[allow(dead_code)]` - dead code.
  - The `step` method delegates to embedded `AgentV2` if version is V2, then converts the decision. This wrapper pattern is the "V1 wrapper" mentioned in docs.
  - The `run` method (lines 617-777) is the old entry point that contains tool execution logic. This logic has been moved to orchestrator, but `run` is still used by `src/main.rs` for one-shot queries. So there is **duplicate execution logic**: `Agent::run` and `AgentOrchestrator::run_agent_loop_v1/v2` both handle tool execution, approval, etc.
  - The agent stores both `history` and `pending_decision` - state management is complex.
  - The file is very long (1009 lines) and mixes concerns: protocol parsing, tool execution, memory injection, LLM calling.

---

### 8. `core/src/agent/traits.rs`
- **Lines**: 33
- **Purpose**: Define traits for core agent components to decouple from external systems
- **Key exports**: `TerminalExecutor` trait (async, with `execute_command` and `get_screen` methods)
- **Dependencies**: std::time::Duration, async_trait
- **Issues spotted**: 
  - This is good design - properly abstracts terminal operations.
  - However, the trait is in core, but the implementation (`TerminalDelegate`) is in `src/terminal/delegate_impl.rs`. This is correct direction of dependency (terminal depends on core).
  - No issues with this file itself.

---

### 9. `core/src/agent/factory.rs`
- **Lines**: 276
- **Purpose**: Agent factory with builder pattern and pre-configured agent types
- **Key exports**: 
  - `BuiltAgent` enum (V1/V2)
  - `AgentBuilder` struct with many configuration methods
  - `AgentConfigs` struct with pre-built configs: `basic`, `development`, `web_enabled`, `full_featured`, `minimal`
  - Helper functions: `create_basic_agent`, `create_development_agent`, `create_web_agent`, `create_full_agent`
- **Dependencies**: From core: agent, agent::v2, agent::tools, llm, config, memory::scribe
- **Issues spotted**:
  - The factory is clean and follows builder pattern correctly.
  - No direct terminal dependencies.
  - The `AgentBuilder` builds either V1 or V2 based on `AgentVersion`.
  - The pre-configured `AgentConfigs` are duplicated in `src/main.rs` where tools are manually listed again (see lines 177-280). **DRY violation**: `AgentConfigs::development` and `handle_one_shot` both list the same tools (shell, file ops, git, system, wait, list_jobs, etc.).
  - `src/main.rs` also duplicates the `full_featured` tool set.

---

### 10. `src/main.rs`
- **Lines**: 1253
- **Purpose**: Binary entry point - CLI handling and one-shot query setup
- **Key exports**: N/A (binary main)
- **Dependencies**: 
  - From crate: cli, terminal, server
  - From mylm-core: config, context, llm, output, agent::traits, agent::tools, executor, memory, state, pacore
  - External: anyhow, clap, console, std::sync
- **Issues spotted**:
  - **Massive duplication of agent/tool initialization** (lines 936-1068). This code builds all tools manually and creates a `DelegateTool`. It duplicates exactly what `AgentConfigs::development` and `AgentConfigs::full_featured` do.
  - The `handle_one_shot` function creates an `Agent` with `Agent::new_with_iterations` and then calls `agent.run`. This is the V1 execution path. But the agent is created with `AgentVersion::V2`, so it embeds an `AgentV2` and delegates. However, the `run` method contains its own loop and tool execution logic, duplicating the orchestrator's `run_agent_loop_v1`.
  - The global `JOB_REGISTRY` static is defined here and used by CLI commands. This is a singleton pattern that works but is not ideal for testing.
  - The `HeadlessTerminalExecutor` (in `src/server/mod.rs`) implements `TerminalExecutor` for headless mode. This is good.
  - The `handle_settings_dashboard` and hub code are very long (over 150 lines) but that's UI logic, not core.
  - **No use of `AgentOrchestrator` in one-shot mode** - it uses the old `Agent::run`. This means the orchestrator is only used in TUI mode. This is a **layer inconsistency**: the terminal layer has two ways to run agents (orchestrator for TUI, direct `Agent::run` for one-shot).

---

### 11. `src/terminal/mod.rs`
- **Lines**: 1758
- **Purpose**: Terminal UI layer - TUI setup, event loop, agent initialization for interactive mode
- **Key exports**: `run_tui` function, `TerminalGuard` struct
- **Dependencies**: 
  - From crate: terminal::app, terminal::pty, terminal::ui, terminal::session, terminal::delegate_impl
  - From mylm-core: many modules including agent, memory, llm, config, context, executor, event_bus, orchestrator
  - External: anyhow, clap, console, crossterm, ratatui, std::sync, tokio::sync
- **Issues spotted**:
  - This file is **extremely large** (1758 lines) and does too much: sets up LLM client, creates tools, builds agent, creates orchestrator, runs event loop. It violates Single Responsibility.
  - **Duplicate agent creation**: Lines 304-316 create a V1 `Agent` with embedded V2. Lines 322-349 create a separate `AgentV2` for the orchestrator. This is the **dual agent instantiation** that causes duplicate logs (as documented in DOCUMENTATION.md). The V1 wrapper is only for UI state, but it's wasteful.
  - The `run_tui` function mixes initialization, tool building, and event loop logic. It should be split.
  - The `run_loop` function (lines 515-1758) is a massive 1243-line event handler. It handles PTY data, input, ticks, core events, terminal commands, mouse, resize, paste. This is a **God function** that needs to be broken into smaller handlers.
  - The tool building code (lines 192-280) duplicates `AgentConfigs` from factory. DRY violation.
  - The `ConfigUpdate` event handler (lines 825-979) rebuilds all tools and the agent from scratch. This is complex and duplicates initialization logic.
  - The file imports both `agent::Agent` and `agent::v2::AgentV2` and manages both, showing the V1/V2 complexity.
  - The `TerminalDelegate` is created and passed to the orchestrator, which is correct use of the `TerminalExecutor` trait.
  - **No clear separation**: The terminal layer knows too much about core agent internals (e.g., accessing `agent.scribe`, `agent.scratchpad`, `agent.history` directly).

---

### 12. `src/terminal/session.rs`
- **Lines**: 165
- **Purpose**: Session persistence structures and statistics tracking
- **Key exports**: 
  - Structs: `Session`, `SessionMetadata`, `SessionStats`, `SessionMonitor`
  - Methods: `resume_stats`, `add_usage`, `set_max_context`, `set_pricing`, `get_context_ratio`, `get_stats`, `duration`
- **Dependencies**: std::time, mylm_core::llm, serde, chrono
- **Issues spotted**: 
  - Clean data structures. No terminal-specific dependencies beyond chrono/serde.
  - `SessionMonitor` is a simple wrapper around `SessionStats`.
  - No issues - this is well-designed.

---

### 13. `DOCUMENTATION.md`
- **Lines**: 251
- **Purpose**: Architecture documentation explaining orchestrator, V1/V2, job handling, event flow
- **Key content**: 
  - High-level architecture diagram
  - Explanation of how every chat message becomes a job
  - Background workers and smart wait
  - V1 vs V2 comparison
  - Explanation of duplicate logs (due to dual agent instantiation)
  - Event flow diagram
- **Issues spotted**: 
  - The documentation is **excellent** and accurately describes the current architecture.
  - It reveals that the dual agent instantiation is **known and intentional** (for UI compatibility), but it's still an architectural smell.
  - The docs also confirm that the orchestrator was migrated from terminal to core, and that V2 is the preferred execution model.

---

### 14. `debug.log`
- **Lines**: 52 (sample)
- **Purpose**: Recent debug output from a TUI session startup
- **Key observations**: 
  - Shows duplicate tool registration logs: lines 7-27 show AgentV2 adding tools (first instance), lines 28-48 show the same tools added again (second instance).
  - Confirms the dual instantiation: "V2 will use full system prompt prefix" appears once, but tool logs appear twice.
  - No errors or warnings in this sample.
- **Issues spotted**: 
  - The duplicate logs are confusing but not harmful.
  - The log shows 20 tools being added each time, indicating the tool set is fully duplicated.

---

## Summary: Architectural Analysis

### Layer Architecture: Core vs Terminal Separation

**Current State**: The boundary is **leaky and inconsistent**.

- **Core** (`core/src/`) should be pure business logic, independent of UI. However:
  - Core exports a `terminal` module (`core/src/lib.rs` line 19) - this is a **major violation**. The terminal layer should depend on core, not the reverse.
  - Core contains `core/src/terminal/` directory with `app.rs`, `mod.rs`, etc. This appears to be a duplicate or alternative terminal implementation? The file structure shows both `core/src/terminal/` and `src/terminal/`. This is **confusing and redundant**.
  - Core depends on terminal UI types: `core/src/agent/execution.rs` imports `terminal::app::TuiEvent`; `core/src/agent/core.rs` also imports `TuiEvent`. This creates a **cyclic dependency** at the module level (core → terminal → core?).

- **Terminal** (`src/terminal/`) is the actual TUI implementation. It:
  - Depends heavily on core (good)
  - Contains massive `mod.rs` (1758 lines) that should be split into multiple files
  - Duplicates agent/tool initialization logic that belongs in core/factory

**Conclusion**: The migration of `agent_runner` logic to core is **incomplete**. Core still imports terminal types, and terminal still contains agent loop logic (via orchestrator usage is good, but `Agent::run` is still used for one-shot). The layers are **tightly coupled** with bidirectional knowledge.

---

### DRY Violations

1. **Tool list duplication**: 
   - `core/src/agent/factory.rs` defines tool sets in `AgentConfigs` (basic, development, full_featured, etc.)
   - `src/main.rs` (lines 177-280) and `src/terminal/mod.rs` (lines 192-280) **duplicate these exact tool lists** manually.
   - This means adding a new tool requires updating 3 places.

2. **Agent loop duplication**:
   - `core/src/agent/orchestrator.rs` has `run_agent_loop_v1` and `run_agent_loop_v2` which are 80% similar (job polling, event publishing, tool execution branching).
   - `core/src/agent/core.rs::run` (V1 legacy) duplicates tool execution, approval flow, and observation handling that is also in orchestrator's V1 loop.

3. **Tool execution logic**:
   - `core/src/agent/execution.rs::execute_tool_with_registry` and the tool execution branches in `orchestrator.rs` (lines 571-602 for V1, 871-926 for V2) do similar things: call `tool_registry.execute_tool`, handle errors, record to memory.
   - `process_tool_args` is duplicated in `execution.rs` and `orchestrator.rs` (same JSON extraction).

4. **Approval flow**:
   - `core/src/agent/execution.rs::handle_tool_approval` and `core/src/agent/core.rs::handle_approval_flow` are nearly identical (both send `TuiEvent::SuggestCommand` and wait on channel). The orchestrator bypasses approval for auto-approve but the logic is scattered.

5. **Memory recording**:
   - Memory recording (`store.record_command`) appears in both `orchestrator.rs` V1 and V2 loops, and also in `core.rs::run`. Should be centralized.

6. **System prompt generation**:
   - `core/src/agent/core.rs::generate_system_prompt` and `src/terminal/mod.rs` both build system prompts using `build_system_prompt` from config, but terminal also does it differently for V2 (lines 959-966). The logic for including tools and scratchpad is duplicated.

---

### Dead Code

1. **`core/src/agent/core.rs`**:
   - `run_v2_bridge` (lines 779-852) - marked `#[allow(dead_code)]`, unused. This was an earlier attempt to bridge V1→V2.
   - `inject_memory_context_internal` (lines 854-880) - marked `#[allow(dead_code)]`, unused. There is a public `inject_memory_context` that uses the context module directly.
   - `pending_tool_call_id` field in `Agent` struct - used in `handle_native_tool_call` but never cleared or used elsewhere? Actually it's set and then used in `step` to route tool responses. Might be used, but check if still needed with V2 delegation.

2. **`core/src/agent/orchestrator.rs`**:
   - Unused parameters: `_job_registry`, `_job_id`, `_task` in `run_agent_loop_v1`. These are leftovers from the migration - they were needed in the terminal version but are not used in core version.
   - `active_worker_count` in V1 loop is computed but never used (V1 doesn't support smart wait). Could be removed.

3. **`src/main.rs`**:
   - The `handle_batch` and `handle_ask` functions use `mylm_core::pacore` which is experimental. They are used by CLI commands, so not dead, but the PaCoRe feature might be underutilized.

4. **`core/src/agent/execution.rs`**:
   - `build_continue_nudge` and `build_format_nudge` are used by `core.rs::run` but not by orchestrator. If V1 is deprecated, these may become dead.

---

### Migration Remnants: Terminal→Core

The orchestrator was recently migrated from `src/terminal/` to `core/src/agent/`. Evidence of incomplete migration:

1. **Unused parameters in orchestrator**: The `_job_registry`, `_job_id`, `_task` parameters in `run_agent_loop_v1` are relics from when the function signature matched the terminal version. They should be removed.

2. **Core still depends on terminal**: `execution.rs` and `core.rs` import `TuiEvent`. This suggests that the approval and observation emission code was **not** fully migrated to use the EventBus. The orchestrator uses `EventBus` correctly, but the legacy `Agent::run` still uses direct TUI events.

3. **Dual agent instantiation**: The terminal layer creates both a V1 wrapper and a V2 agent for the orchestrator. This is a workaround because the UI expects a V1 `Agent` to access fields like `history`, `scratchpad`, etc. A proper migration would either:
   - Move UI state into a separate struct that both versions can populate, or
   - Have the orchestrator expose the needed state via queries, not by sharing the agent directly.

4. **`Agent::run` still exists and is used**: The orchestrator is used in TUI mode, but one-shot queries in `main.rs` still call `Agent::run`. This means the old terminal-coupled execution path is still alive. The migration should have moved all execution to orchestrator, even for one-shot.

5. **`TerminalExecutor` trait in core but implementation in terminal**: This is actually good, but the fact that core needs to call terminal operations shows the boundary is necessary. However, the `execution.rs` module bypasses this trait and directly uses `TuiEvent`, which is wrong.

---

### Coupling Issues

1. **Core → Terminal coupling**:
   - `core/src/agent/execution.rs` imports `terminal::app::TuiEvent`
   - `core/src/agent/core.rs` imports `terminal::app::TuiEvent`
   - This creates a **direct dependency** from core to terminal UI, making core un-testable without terminal and violating clean architecture.

2. **Terminal → Core over-coupling**:
   - `src/terminal/mod.rs` reaches deep into core agent internals: accesses `agent.scribe`, `agent.scratchpad`, `agent.history`, `agent.session_id` directly (lines 861-878, 961-976). This couples the terminal to the agent's concrete fields.
   - The terminal rebuilds the entire agent on config change (lines 825-979) - this is a sign that the agent is not easily reconfigurable at runtime.

3. **Orchestrator and Agent share too much state**:
   - The orchestrator clones `agent_v1` or `agent_v2` and calls `step`. But the agent also has its own `job_registry`, `event_bus` references. The orchestrator manages job registry separately. This could lead to inconsistencies.
   - The agent's `v2_agent` field (in V1 wrapper) means the same agent can have two different execution engines. This is confusing.

4. **Factory and manual tool building**:
   - The factory exists but is ignored by `main.rs` and `terminal/mod.rs`. They manually construct tools, duplicating the factory's pre-configured sets. This indicates the factory is not trusted or not flexible enough.

5. **EventBus usage**:
   - The orchestrator uses `EventBus` to publish events to terminal. This is good decoupling.
   - However, the `Agent` itself (in `core.rs`) also has an optional `event_bus` field and publishes events directly (lines 659, 687, etc.). This creates **two sources of events**: the agent can publish on its own bus, and the orchestrator publishes on its bus. In TUI mode, the orchestrator's bus is used, but the agent's bus is set to `Some(event_bus.clone())` (line 338 in terminal/mod.rs). So both are the same bus. But the agent publishing directly is still a concern because it knows about `CoreEvent` types, which are UI-oriented? Actually `CoreEvent` is in core/agent/event.rs, so it's core-level events. This is okay if events are part of core domain.

6. **Memory store coupling**:
   - Many components take `Arc<VectorStore>` directly. This is fine for dependency injection, but the store is used by tools, agent, scribe, categorizer. The wiring is scattered.

---

### Top 5 Refactoring Priorities

Based on impact and feasibility:

1. **Remove core→terminal dependency** (Critical)
   - **Issue**: `core/src/agent/execution.rs` and `core/src/agent/core.rs` import `terminal::app::TuiEvent`.
   - **Fix**: Replace `TuiEvent` usage with `EventBus` publishing. The approval flow should publish a `CoreEvent::AwaitingApproval` event, and the terminal can listen and handle UI. The `handle_tool_approval` function should return a `Result<bool, String>` based on a channel that the terminal provides via a trait or callback, not via `TuiEvent`.
   - **Impact**: Makes core truly independent of terminal, enabling headless use and testing.

2. **Consolidate agent loop logic** (High)
   - **Issue**: `orchestrator.rs` has two nearly identical loops (V1/V2). `core.rs::run` duplicates loop logic too.
   - **Fix**: Extract a common loop framework that handles job polling, smart wait, interruption, event publishing. The version-specific parts should be small callbacks: `step_once`, `handle_action`, etc. Or, since V2 is preferred, deprecate V1 entirely and remove the V1 loop.
   - **Impact**: Reduces code duplication, makes maintenance easier.

3. **Eliminate dual agent instantiation** (High)
   - **Issue**: Terminal creates both a V1 wrapper (with embedded V2) and a separate V2 for orchestrator, causing duplicate logs and wasted resources.
   - **Fix**: The terminal should only create an `AgentOrchestrator` with an `AgentV2`. The UI state (history, scratchpad, etc.) should be extracted into a separate `AgentState` struct that the terminal can read from the orchestrator's agent, or the orchestrator should expose query methods. Alternatively, make the V1 wrapper a thin view over the V2 agent without creating a second instance.
   - **Impact**: Improves performance, reduces confusion, eliminates duplicate logs.

4. **Centralize tool configuration** (Medium)
   - **Issue**: Tool lists are duplicated in `AgentConfigs`, `main.rs`, and `terminal/mod.rs`.
   - **Fix**: Make the factory the single source of truth. Export the pre-configured tool vectors from `factory.rs` (e.g., `pub const DEVELOPMENT_TOOLS: &[ToolDesc]`). Then `main.rs` and `terminal/mod.rs` should use these constants to build tools. Or, provide a method on `AgentBuilder` that returns the built `Vec<Arc<dyn Tool>>` before creating the agent, so callers can reuse the same list.
   - **Impact**: Easier to add/remove tools, ensures consistency across CLI and TUI.

5. **Split `src/terminal/mod.rs`** (Medium)
   - **Issue**: The file is 1758 lines and contains initialization, event loop, and multiple concerns.
   - **Fix**: Extract into modules:
     - `terminal::init` - agent/orchestrator setup
     - `terminal::event_loop` or `terminal::runner` - the `run_loop` function
     - `terminal::tools` - tool building helpers
     - `terminal::config` - config change handling
   - **Impact**: Improves readability, maintainability, and testability.

---

### Additional Observations

- **Unused imports and variables**: The orchestrator has 7 warnings. These should be cleaned up to improve code quality.
- **V1/V2 complexity**: The codebase carries both V1 and V2, with V1 being a compatibility layer. This adds significant complexity. A migration plan to fully retire V1 should be considered.
- **Smart Wait**: The smart wait algorithm (polling workers) is a good design but is only in V2 loop. V1 doesn't support it.
- **EventBus**: The event bus is used well for core→terminal communication. However, some events like `InternalObservation` are sent as `PtyWrite` which mixes concerns.
- **Memory and context**: The memory system (VectorStore, Scribe, Journal) is well-separated into its own modules. The agent's memory integration is clean.
- **Safety and permissions**: The `permissions` module exists but is not deeply analyzed here. It appears to be used for tool allowlisting.

---

## Conclusion

The mylm project shows signs of a **recent partial refactor** (orchestrator migration) that improved the architecture by centralizing agent execution in core, but the refactor is **incomplete**. The core layer still depends on terminal UI types, creating a cyclic dependency. The terminal layer duplicates tool configuration and agent initialization logic. The dual agent instantiation is a known workaround for UI compatibility but is inefficient.

**The top priority** is to break the core→terminal dependency by routing all UI communication through `EventBus`. This will make core truly independent and allow the terminal to be swapped out (e.g., for a web UI). After that, consolidating the agent loops and removing the dual instantiation will simplify the runtime architecture. Finally, centralizing tool configuration and splitting the massive terminal module will improve maintainability.

The codebase is functional and compiles with 0 errors, but the technical debt is concentrated in the layer boundaries and duplication. Addressing the 5 priorities above would significantly improve orthogonality and reduce DRY violations.