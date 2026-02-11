# Codebase Investigation Report

## Project Overview
- **Project**: mylm (My Local Model)
- **Language**: Rust
- **Architecture**: TUI-based AI agent with V1 (simple) and V2 (complex) execution paths

## Investigation Methodology
1. Read and analyze code structure
2. Document findings incrementally
3. Identify issues and improvement opportunities

---

## Section 1: V1 vs V2 Agent Architecture

### 1.1 V1 Agent (core/src/agent/core.rs)
**Purpose**: Simple agentic loop for lightweight usage

**Key Components**:
- `Agent` struct: Main V1 agent implementation
- Sequential execution loop
- Simple tool execution (no background jobs)
- JSON-based short-term memory

**Execution Flow**:
```
User Request -> Agent::step() -> Tool Execution -> Response
```

### 1.2 V2 Agent (core/src/agent/v2/core.rs)
**Purpose**: Complex event-driven agent with parallel execution

**Key Components**:
- `AgentV2` struct: Main V2 agent implementation
- `AgentV2Config`: Configuration struct
- Event-driven execution loop (`event_driven.rs`)
- Background job support via `delegate` tool
- LanceDB + FastEmbed vector memory

**Execution Flow**:
```
User Request -> AgentV2::run_event_driven() -> Event Loop -> 
  -> Tool Execution (parallel possible) -> Background Jobs -> Response
```

### 1.3 Key Differences

| Feature | V1 | V2 |
|---------|----|----|
| Execution | Sequential | Event-driven |
| Background Jobs | No | Yes |
| Memory | JSON short-term | LanceDB vector |
| Parallel Tools | No | Yes |
| Worker Model | N/A | Main + Workers |

---

## Section 2: Tool System

### 2.1 Tool Trait (core/src/agent/tool.rs)
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn usage(&self) -> &str;
    async fn call(&self, args: &str) -> Result<ToolOutput, ...>;
    fn kind(&self) -> ToolKind;
}
```

### 2.2 Tool Categories
- **Internal**: Memory, web_search, delegate (silent execution)
- **Terminal**: execute_command, shell tools (visible in terminal)
- **Web**: crawl (web-based)

### 2.3 Available Tools
| Tool | Category | Purpose |
|------|----------|---------|
| delegate | Internal | Spawn background workers |
| execute_command | Terminal | Run shell commands |
| memory | Internal | Store/retrieve memories |
| web_search | Internal | Search the web |
| grep/tail/wc | Terminal | Unix utilities |
| read_file/write_file | Terminal | File operations |

---

## Section 3: Background Jobs System

### 3.1 Delegate Tool (core/src/agent/tools/delegate.rs)
**Purpose**: Spawn worker agents for parallel execution

**Key Issue Identified**: 
- Main agent confused about how to use background jobs
- Tries to use `/jobs` bash command (doesn't exist)
- Should use `delegate` tool and continue with own work

**Worker Spawn Flow**:
1. DelegateTool::call() receives objective
2. spawn_worker() creates AgentV2 for subtask
3. Worker runs in background tokio task
4. JobRegistry tracks job status

### 3.2 Job Registry (core/src/agent/v2/jobs.rs)
**Purpose**: Track active and completed background jobs

**Key Methods**:
- `create_job()` - Register new job
- `complete_job()` - Mark job complete
- `stall_job()` - Mark job stalled
- `poll_updates()` - Check for completed jobs

---

## Section 4: Configuration System

### 4.1 Config Structure (core/src/config/v2/config.rs)
```rust
pub struct ConfigV2 {
    pub endpoint: EndpointConfig,      // LLM connection
    pub agent: AgentConfig,            // Agent behavior
    pub features: FeaturesConfig,      // Feature toggles
    pub profiles: HashMap<String, Profile>, // Profile overrides
}
```

### 4.2 AgentConfig
```rust
pub struct AgentConfig {
    pub max_iterations: usize,          // Default: 10
    pub max_actions_before_stall: usize, // Default: 15 (NEW)
    pub max_consecutive_messages: u32,   // Default: 3 (NEW)
    pub max_recovery_attempts: u32,      // Default: 3 (NEW)
    pub main_model: String,
    pub worker_model: String,
    pub worker_limit: usize,            // Default: 20
}
```

---

## Section 5: Issues Identified

### Issue 1: Missing Tool in Prompt
**Location**: `core/src/config/v2/prompts.rs:90`
**Problem**: Prompt mentions `codebase_search` tool which doesn't exist
**Tools Available**: `grep`, `find` for code search
**Fix**: Changed to `grep`

### Issue 2: Main Agent Confusion About Background Jobs
**Location**: Orchestrator chat session loop
**Problem**: Main agent tries to use `/jobs` bash command instead of delegate tool
**Root Cause**: Insufficient documentation in system prompt about delegate workflow

### Issue 3: Terminal Delegate Not Set on Orchestrator
**Location**: `src/terminal/app/state.rs`
**Problem**: Orchestrator created without terminal delegate, causing "Terminal delegate not available" errors
**Fix**: Added `set_terminal_delegate()` call in `new_with_orchestrator`

### Issue 4: Workers Not Returning Final Answers
**Location**: `core/src/agent/v2/driver/event_driven.rs`
**Problem**: Workers execute tools but don't recognize task completion
**Root Cause**: System prompt doesn't emphasize completion criteria
**Fix**: Added "TASK COMPLETION RULE" to worker system prompt

### Issue 5: Hardcoded Values
**Location**: Multiple files
**Problem**: Hardcoded defaults instead of config-driven values
**Fix**: Added new config fields and propagated them through AgentV2Config

---

## Section 6: Code Duplication

### 6.1 Config Structs
- `AgentConfig` (V1) vs `AgentV2Config` (V2)
- Similar fields but separate structs
- Both need updating when adding new config options

### 6.2 Factory Patterns
- `AgentBuilder` for V1
- Direct `AgentV2::new_with_config()` for V2
- `create_agent_for_session()` in `factory.rs`
- `create_agent_v2_for_session()` in `factory.rs`

### 6.3 Execution Loops
- `run_agent_loop_v1()` - V1 sequential loop
- `run_event_driven()` - V2 event-driven loop
- `run_chat_session_loop_v2()` - V2 chat session loop

---

## Section 7: Potential Improvements

### Improvement 1: Unified Config
**Idea**: Single `AgentConfig` that works for both V1 and V2
**Benefit**: Less duplication, easier maintenance
**Challenge**: V1 and V2 have different capabilities

### Improvement 2: Better Tool Discovery
**Idea**: Generate tool documentation from actual tool implementations
**Benefit**: Prompts always match available tools
**Current**: Tools listed in prompts.rs may not exist (e.g., codebase_search)

### Improvement 3: Worker Lifecycle Visualization
**Idea**: Better UI for tracking worker progress
**Current**: Jobs panel shows basic status
**Desired**: Real-time progress, action history, result preview

### Improvement 4: Simplified Background Job UX
**Idea**: Main agent should understand delegate workflow better
**Current**: Tries to manage jobs via bash commands
**Desired**: Natural language job management

---

## Ongoing Investigation...

---

## Section 8: Root Cause Analysis - Background Job Confusion

### Problem
Main agent tries to use `/jobs` bash command instead of `delegate` tool.

### Evidence
From screenshot:
```
bash: /jobs: No such file or directory
```

### Expected Behavior
Agent should call `delegate` tool with objective to spawn workers.

### Actual Behavior
Agent calls `execute_command` with `/jobs cancel ...` which fails.

### Code Analysis

**Orchestrator has delegate handling** (loops.rs:742-745):
```rust
if tool == "delegate" {
    event_bus.publish(CoreEvent::StatusUpdate {
        message: "Workers spawned".to_string(),
    });
}
```

**Delegate tool is registered** (terminal/mod.rs:280):
```rust
agent_v2.tools.insert(delegate_tool.name().to_string(), Arc::new(delegate_tool));
```

### Root Cause
The LLM doesn't understand the relationship between:
1. `delegate` tool (spawns workers)
2. Background jobs panel (shows status)
3. How to interact with them

It sees jobs mentioned in UI/context and tries to use bash commands.

### Why Previous Fixes Didn't Work
- Added "Background Jobs" section to system prompt
- Added "TASK COMPLETION RULE" to worker prompt
- But the main agent still doesn't connect "delegate" with "background jobs"

### Missing Concept
The agent doesn't understand that:
- Background jobs are CREATED by calling `delegate` tool
- Once created, they run automatically
- No bash commands needed to manage them

### Potential Solutions

#### Solution 1: Better Prompt Engineering
Add explicit examples to system prompt:
```
EXAMPLE - Spawning Background Workers:
User: "Check 3 files in parallel"
You: Call delegate tool with objective "Check file 1"
      Call delegate tool with objective "Check file 2"  
      Call delegate tool with objective "Check file 3"
      Continue chatting while workers run
      System will notify when workers complete
```

#### Solution 2: Remove Jobs References from Context
Don't mention `/jobs` or job IDs to the agent.
Let the system handle worker lifecycle transparently.

#### Solution 3: Create `jobs` Tool
Create an actual `jobs` tool that wraps job management:
- `jobs list` - List active jobs
- `jobs cancel <id>` - Cancel job
- `jobs status <id>` - Get job status

This would give the agent a proper tool instead of bash commands.

### Recommendation
Implement Solution 3 (Create `jobs` tool) as it provides:
- Clear interface for job management
- Consistent with tool-based architecture
- LLM can understand and use properly

