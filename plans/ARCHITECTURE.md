# MYLM Architecture - Component Flow & Structure

## 1. SYSTEM PROMPT CONSTRUCTION FLOW

```
terminal/mod.rs::run_tui_session()
    │
    ▼
mylm_core::config::build_system_prompt()
    │
    ▼
core/src/config/v2/prompts.rs::render_config()
    │
    ├──> EMBEDDED_DEFAULT_CONFIG (JSON with sections)
    │       ├── Section: identity (priority 1)
    │       ├── Section: behavior (priority 2)
    │       ├── Section: user_instructions (priority 3)
    │       └── Section: tools (priority 4, DYNAMIC)
    │
    └──> generate_capabilities_prompt()
            │
            ├──> Categorize tools by kind:
            │       Internal: memory, scratchpad, consolidate_memory, delegate
            │       Terminal: execute_command, grep, tail, list_files, etc.
            │       Web: web_search, crawl
            │
            └──> Format: "## Operational Workflow" (lines 88-93)
                    1. Recall (memory)
                    2. Search (grep) - WAS: codebase_search (DOESN'T EXIST!)
                    3. Plan (scratchpad)
                    4. Act (execute_command)
                    5. Record (consolidate_memory)
```

### ISSUE: Prompt Structure
```
CURRENT ORDER (WRONG):
  1. User Instructions (line 1-2) - SHOULD NOT BE IN SYSTEM PROMPT
  2. Available Tools (lines 5-78) - TOO EARLY, NO CONTEXT
  3. Short-Key JSON Format (lines 79-117) - TOO LATE!

CORRECT ORDER SHOULD BE:
  1. Identity & Role
  2. Response Format (MANDATORY FIRST!)
  3. Available Tools
  4. Examples
```

---

## 2. AGENT V2 EXECUTION FLOW

```
src/terminal/mod.rs::run_tui_session()
    │
    ▼
AgentV2::new_with_config(AgentV2Config)
    │
    ├──> AgentV2Config Fields:
    │       - client: LLM client
    │       - tools: HashMap<String, Arc<dyn Tool>>
    │       - max_iterations: usize
    │       - max_actions_before_stall: usize  [NEW CONFIG]
    │       - max_consecutive_messages: u32   [NEW CONFIG]
    │       - max_recovery_attempts: u32      [NEW CONFIG]
    │       - execute_tools_internally: bool
    │       └──> ... other fields
    │
    └──> AgentV2 Fields:
            - tools: HashMap (populated from config)
            - max_steps: usize (from max_iterations)
            - job_registry: JobRegistry
            - execute_tools_internally: bool = FALSE (set in terminal/mod.rs:272)

    ▼
AgentOrchestrator::new_with_agent_v2(agent_v2_arc, event_bus, config)
    │
    └──> Stores: agent_v2, event_bus, terminal_delegate (NONE initially!)

    ▼
AppStateContainer::new_with_orchestrator(..., orchestrator, terminal_delegate, ...)
    │
    └──> SETS: orchestrator.set_terminal_delegate(terminal_delegate) [FIXED]

    ▼
Orchestrator Loop: run_chat_session_loop_v2()
    │
    ├──> Step 1: agent.step(observation)
    │       │
    │       └──> AgentV2::step()
    │               │
    │               ├──> Call LLM
    │               ├──> Parse Short-Key JSON response
    │               └──> Return: AgentDecision::Message | Action | MalformedAction | Error
    │
    ├──> Step 2: handle_decision()
    │       │
    │       ├──> If Message with final_answer → Done
    │       │
    │       ├──> If Action with tool:
    │       │       │
    │       │       ├──> If tool == "execute_command" AND Terminal kind:
    │       │       │       └──> Use terminal_delegate.execute_command() [REQUIRES terminal_delegate!]
    │       │       │
    │       │       └──> If tool == "delegate":
    │       │               └──> DelegateTool::call()
    │       │                       │
    │       │                       ├──> spawn_worker()
    │       │                       │       └──> tokio::spawn(async { worker.run_event_driven() })
    │       │                       │
    │       │                       └──> Return: ToolOutput::Background { job_id, description }
    │       │
    │       └──> Update last_observation
    │
    └──> Step 3: poll_jobs() - Check for completed workers
```

---

## 3. TOOL REGISTRY FLOW

```
Factory: create_agent_for_session() OR terminal/mod.rs
    │
    ▼
Tools Vec Created:
    ├── ShellTool (execute_command)
    ├── WebSearchTool
    ├── MemoryTool
    ├── CrawlTool
    ├── FileReadTool, FileWriteTool
    ├── ListFilesTool
    ├── GitStatusTool, GitLogTool, GitDiffTool
    ├── StateTool
    ├── SystemMonitorTool
    ├── TerminalSightTool
    ├── WaitTool
    └── ShellUtils (grep, tail, wc, du)
    │
    ▼
DelegateTool ADDED SEPARATELY:
    terminal/mod.rs:280
    agent_v2.tools.insert("delegate", Arc::new(delegate_tool));

    ▼
Tools stored in: AgentV2.tools: HashMap<String, Arc<dyn Tool>>
```

### Tool Lookup Flow:
```
Orchestrator needs to execute tool:
    │
    ▼
agent_lock.tools.get(tool_name)  // HashMap lookup
    │
    ├──> Found? → tool.call(args).await
    │
    └──> Not Found? → Error: Tool 'X' not found
```

---

## 4. BACKGROUND JOB LIFECYCLE

```
1. CREATION (DelegateTool::spawn_worker)
    │
    ├──> job_registry.create_job_with_options("delegate", description, is_worker=true)
    │       └──> JobStatus::Created
    │
    ├──> tokio::spawn(async move { worker.run_event_driven(...) })
    │       │
    │       └──> Worker Loop (event_driven.rs)
    │               ├──> Iteration 1: agent.step(history)
    │               │       └──> LLM returns action
    │               ├──> Execute tool
    │               ├──> Check if task complete
    │               │       ├──> Yes → Return final answer → StepOutcome::Done
    │               │       └──> No → Continue loop
    │               └──> On Done: job_registry.complete_job(job_id, result)
    │
    └──> Return to Main Agent: ToolOutput::Background { job_id, description }

2. MONITORING (JobRegistry)
    JobRegistry {
        jobs: Arc<RwLock<HashMap<String, BackgroundJob>>>,
    }
    
    Methods:
    - create_job() → JobID
    - complete_job(job_id, result) → JobStatus::Completed
    - stall_job(job_id, reason) → JobStatus::Stalled
    - poll_updates() → Vec<BackgroundJob> (completed/updated)

3. ORCHESTRATOR POLLING
    poll_jobs() called every iteration:
        │
        ├──> job_registry.poll_updates()
        │       └──> Returns completed jobs
        │
        └──> Format observations for main agent
                "Background job 'X' completed with result: Y"
```

---

## 5. COMPONENT LABELS & FILES

### A. System Prompt Components
| Component | File | Function |
|-----------|------|----------|
| Prompt Builder | `core/src/config/v2/prompts.rs` | `build_system_prompt()`, `render_config()` |
| Default Config | `core/src/config/v2/prompts.rs` | `EMBEDDED_DEFAULT_CONFIG` (JSON string) |
| Capabilities Gen | `core/src/config/v2/prompts.rs` | `generate_capabilities_prompt()` |

### B. Agent Components
| Component | File | Struct/Function |
|-----------|------|-----------------|
| Agent V2 Core | `core/src/agent/v2/core.rs` | `AgentV2`, `AgentV2Config` |
| V2 Execution | `core/src/agent/v2/driver/event_driven.rs` | `run_event_driven()` |
| V1 Agent | `core/src/agent/core.rs` | `Agent`, `AgentConfig` |
| Tool Trait | `core/src/agent/tool.rs` | `Tool` trait, `ToolOutput`, `ToolKind` |

### C. Tool Components
| Tool | File | Name |
|------|------|------|
| Delegate | `core/src/agent/tools/delegate.rs` | "delegate" |
| Shell | `core/src/agent/tools/shell.rs` | "execute_command" |
| Memory | `core/src/agent/tools/memory.rs` | "memory" |
| Jobs | `core/src/agent/tools/jobs.rs` | "list_jobs" |
| Grep | `core/src/agent/tools/shell_utils.rs` | "grep" |

### D. Orchestrator Components
| Component | File | Function |
|-----------|------|----------|
| Orchestrator | `core/src/agent/orchestrator/mod.rs` | `AgentOrchestrator` |
| V2 Loop | `core/src/agent/orchestrator/loops.rs` | `run_chat_session_loop_v2()` |
| Helpers | `core/src/agent/orchestrator/helpers.rs` | `execute_terminal_tool()`, `poll_jobs()` |

### E. Job Management
| Component | File | Struct/Function |
|-----------|------|-----------------|
| Job Registry | `core/src/agent/v2/jobs.rs` | `JobRegistry`, `BackgroundJob` |
| Job Status | `core/src/agent/v2/jobs.rs` | `JobStatus` enum |

### F. Terminal/App Components
| Component | File | Function |
|-----------|------|----------|
| TUI Entry | `src/terminal/mod.rs` | `run_tui_session()` |
| App State | `src/terminal/app/state.rs` | `AppStateContainer` |
| Terminal Delegate | `src/terminal/terminal_delegate.rs` | `TerminalDelegate` |

---

## 6. ISSUE ISOLATION

### Issue A: System Prompt Order
**Location**: `core/src/config/v2/prompts.rs::EMBEDDED_DEFAULT_CONFIG`
**Problem**: Response format explained LAST instead of FIRST
**Impact**: Model doesn't know it MUST use JSON until after reading all tools
**Fix**: Reorder sections: Identity → Response Format → Tools → Examples

### Issue B: "User Instructions" in System Prompt
**Location**: `core/src/config/v2/prompts.rs::EMBEDDED_DEFAULT_CONFIG` line 129
**Problem**: Section titled "User Instructions" but this is the SYSTEM prompt
**Impact**: Confuses model about who is instructing whom
**Fix**: Rename to "Your Purpose" or "Your Role"

### Issue C: Tool Priority
**Location**: `core/src/config/v2/prompts.rs::generate_capabilities_prompt()`
**Problem**: Tools listed alphabetically, not by importance
**Impact**: Model sees git_status first, not execute_command or delegate
**Fix**: Order by importance: execute_command, delegate, memory, THEN others

### Issue D: Missing Tool Documentation
**Location**: `core/src/config/v2/prompts.rs::generate_capabilities_prompt()` line 90
**Problem**: References `codebase_search` which DOESN'T EXIST
**Actual Tool**: `grep` exists for code search
**Fix**: Change "codebase_search" to "grep" (already done)

### Issue E: Background Jobs Confusion
**Location**: Main agent behavior
**Problem**: Agent tries `/jobs` bash command instead of `delegate` tool
**Root Cause**: No clear link between "background jobs" concept and "delegate" tool
**Fix Options**:
  1. Add explicit examples to prompt
  2. Create `jobs` management tool
  3. Remove job IDs from context shown to agent

---

## 7. FLOW DIAGRAM: Tool Execution Decision

```
Agent Decision: Use Tool
    │
    ▼
Tool Kind Check:
    │
    ├──> Terminal Kind (execute_command, grep, etc.)
    │       │
    │       ├──> orchestrator has terminal_delegate?
    │       │       ├──> YES → terminal_delegate.execute_command()
    │       │       └──> NO  → "Error: Terminal delegate not available"
    │       │
    │       └──> [WAS BROKEN: terminal_delegate not set]
    │
    ├──> Internal Kind (memory, delegate, etc.)
    │       └──> Execute directly via tool.call()
    │
    └──> Web Kind (web_search, crawl)
            └──> Execute directly via tool.call()
```

---

## 8. UNAPPROVED CHANGES AUDIT

| Change | File | Line | Status | Approved? |
|--------|------|------|--------|-----------|
| Added max_actions_before_stall | `AgentConfig` | - | New field | NO |
| Added max_consecutive_messages | `AgentConfig` | - | New field | NO |
| Added max_recovery_attempts | `AgentConfig` | - | New field | NO |
| Changed unwrap_or(50) | `delegate.rs` | 372 | Uses config | NO |
| Added Background Jobs section | `prompts.rs` | - | Section added | NO |
| Added TASK COMPLETION RULE | `delegate.rs` | - | Worker prompt | NO |
| Fixed terminal_delegate set | `state.rs` | 401-404 | Bug fix | IMPLIED |
| Changed codebase_search→grep | `prompts.rs` | 90 | Bug fix | IMPLIED |

---

Document Version: 1.0
Last Updated: 2026-02-08
