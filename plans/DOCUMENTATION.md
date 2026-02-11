# Agent Architecture: Chat & Background Job Handling

## Overview

The myLM agent system uses a centralized **Orchestrator** pattern to manage chat interactions and background job execution. This document explains the architecture, the difference between V1 and V2 agents, and why certain behaviors (like duplicate logs) occur.

---

## 1. High-Level Architecture

```mermaid
graph TD
    UI[Terminal UI] -->|submit_message| Orch[Agent Orchestrator]
    Orch -->|1. Create Job| JobReg[Job Registry]
    Orch -->|2. Spawn Task| Loop[Agent Loop (V2)]
    
    Loop -->|Decision: Message| EventBus[Event Bus]
    Loop -->|Decision: Action| ToolExec[Tool Execution]
    
    EventBus -->|AgentResponse| UI
    
    subgraph "Background Jobs"
        Worker[Worker Agent]
        JobReg
    end
    
    ToolExec -->|spawn_worker| JobReg
    JobReg --> Worker
```

**Key Components:**

*   **Terminal UI**: The TUI interface that accepts user input and displays output.
*   **AgentOrchestrator**: Central execution manager. It receives tasks, spawns agent loops, and tracks active tasks.
*   **EventBus**: Unidirectional communication channel. Core components publish events (status updates, agent responses) that the UI subscribes to.
*   **JobRegistry**: Thread-safe registry that tracks all background jobs (including the main chat task) with their status, metrics, and lifecycle.
*   **Agent Loop**: The actual reasoning/execution cycle that runs the LLM and processes tool calls.

---

## 2. Chat Handling: Every Message is a Job

When you type a message and press Enter, the following happens:

1.  **Task Submission** (`src/terminal/app/app.rs:24`):
    ```rust
    pub async fn submit_message(&mut self, event_tx: UnboundedSender<TuiEvent>) {
        // ...
        let _task_handle = orchestrator.start_task(input_clone, history).await;
    }
    ```

2.  **Job Creation** (`core/src/agent/orchestrator.rs:229`):
    ```rust
    pub async fn start_task(&self, task: String, history: Vec<ChatMessage>) -> TaskHandle {
        let job_id = self.job_registry.create_job("orchestrator", &task);
        // Spawns a tokio task that runs the agent loop
    }
    ```
    *   A new job is registered with tool name `"orchestrator"` and the user's message as the description.
    *   This job tracks the entire lifecycle of this interaction.

3.  **Agent Loop Execution** (`core/src/agent/orchestrator.rs:631`):
    *   The orchestrator spawns a dedicated task running `run_agent_loop_v2`.
    *   This loop repeatedly calls `agent.step()` until:
        *   A final answer is detected.
        *   The agent asks a question to the user.
        *   The step budget is exceeded.
        *   An error occurs.

4.  **Response Delivery**:
    *   When the agent decides on a text message (`AgentDecision::Message`), it publishes a `CoreEvent::AgentResponse` to the EventBus.
    *   The UI receives this event and displays it in the chat window.
    *   The orchestrator task then completes, marking the job as `Completed`.

**Why does it look like "everything spawns a job"?**
Because the system treats **every user interaction** as a tracked job. This provides:
*   Consistent lifecycle management (start, poll, complete, cancel).
*   Visibility into what the agent is doing (via the Jobs panel).
*   Ability to interrupt and manage active conversations.

---

## 3. Background Jobs (Workers)

Background jobs are sub-agents spawned to perform long-running or parallel tasks.

### 3.1 Spawning a Worker

*   **Trigger**: The main agent calls the `delegate` tool with an objective.
*   **Implementation** (`core/src/agent/tools/delegate.rs` → `core/src/agent/orchestrator.rs:326`):
    ```rust
    pub async fn spawn_worker(&self, objective: String) -> Result<String, String> {
        let job_id = self.job_registry.create_job("worker", &objective);
        self.event_bus.publish(CoreEvent::WorkerSpawned { job_id, description: objective });
        Ok(job_id)
    }
    ```
*   A new `AgentV2` instance is created and runs its own independent event loop in a separate tokio task.
*   The worker registers itself in the `JobRegistry` with status `Running`.

### 3.2 Smart Wait: Main Agent vs. Workers

The main agent does **not** block while workers run. Instead, it uses a "Smart Wait" algorithm (`core/src/agent/orchestrator.rs:743`):

```rust
// SMART WAIT: If no new observations and workers are running, wait
if last_observation.is_none() && active_worker_count > 0 && !has_new_observations {
    smart_wait_iterations += 1;
    if smart_wait_iterations >= config.max_smart_wait_iterations {
        // Timeout: inform user and return
        return Ok(());
    }
    tokio::time::sleep(...).await;
    continue; // Loop again to poll for worker completion
}
```

*   The main agent pauses for short intervals (default 1 second) and polls the `JobRegistry` for updates.
*   When a worker completes, its result is injected as an "observation" into the main agent's next step.
*   This allows the main agent to remain responsive and efficiently manage multiple concurrent workers.

### 3.3 Job Lifecycle & States

The `JobRegistry` (`core/src/agent/v2/jobs.rs`) manages job states:

*   `Running`: Actively executing.
*   `Completed`: Finished successfully.
*   `Failed`: Terminated with an error.
*   `Cancelled`: User cancelled.
*   `TimeoutPending`: Worker timed out, waiting for grace period before final cleanup.
*   `Stalled`: Exceeded action budget without progress (requires main agent intervention).

Jobs are automatically cleaned up after a grace period (15 seconds) using a two-phase claim system to prevent race conditions.

---

## 4. V1 vs. V2: The Architectural Shift

The codebase contains two agent versions because of a major refactor.

### 4.1 V1 (Legacy)

*   **Model**: Sequential, single-threaded loop.
*   **Execution**: One step at a time. After each tool call, the agent waits for the result before continuing.
*   **Limitation**: Cannot handle background tasks. The entire agent is blocked during long operations.
*   **Location**: `core/src/agent/core.rs` (the `Agent` struct).

### 4.2 V2 (Current)

*   **Model**: Event-driven, asynchronous.
*   **Execution**: Uses `run_event_driven` (`core/src/agent/v2/driver/event_driven.rs`).
    *   Non-blocking heartbeat.
    *   Can spawn and manage background workers.
    *   Uses `JobRegistry` for all task tracking.
*   **Key Features**:
    *   **Background Jobs**: True parallelism via sub-agents.
    *   **Stuck Detection**: Automatically detects workers that have been idle too long.
    *   **Budget Management**: Can request approval when step limits are reached.
*   **Location**: `core/src/agent/v2/`.

### 4.3 Why Both Exist

The UI layer (`src/terminal/app/`) was built for V1. Migrating it fully to V2 is a large effort. The current solution:

1.  Create a V1 `Agent` wrapper.
2.  Inside that wrapper, initialize an embedded `AgentV2` (if version = V2).
3.  The wrapper's `step()` method delegates to the internal V2 agent.
4.  The UI continues to talk to the V1 wrapper, unaware of V2 underneath.

**Result**: The system works, but requires initializing the V2 engine twice:
*   Once inside the V1 wrapper (for UI compatibility).
*   Once for the `AgentOrchestrator` (for actual execution).

---

## 5. Why Logs Are Duplicated

You may see duplicate log entries like:

```
[INFO] AgentV2::new_with_config: adding tool 'git_diff'
[INFO] AgentV2::new_with_config: adding tool 'git_diff'
```

**Cause**: During startup (`src/terminal/mod.rs`), the code creates **two separate `AgentV2` instances**.

1.  **First Instance** (lines 304-316):
    ```rust
    let mut agent = Agent::new_with_iterations(...).await;
    ```
    This creates the V1 wrapper with an embedded V2 agent. The V2 initialization logs appear here.

2.  **Second Instance** (lines 322-349):
    ```rust
    let agent_v2 = AgentV2::new_with_config(agent_v2_config);
    let orchestrator = AgentOrchestrator::new_with_agent_v2(...);
    ```
    This creates a standalone V2 agent specifically for the orchestrator. It goes through the same initialization, producing identical logs.

**Impact**: Purely cosmetic. The second instance (orchestrator) is the one that actually executes tasks. The first instance is kept for UI state display. No functional duplication occurs at runtime.

---

## 6. Event Flow: From Input to Output

```
User types "Hello" → Enter
    ↓
AppStateContainer::submit_message()
    ↓
orchestrator.start_task("Hello", history)
    ↓
JobRegistry::create_job("orchestrator", "Hello")
    ↓
tokio::spawn(run_agent_loop_v2)
    ↓
Loop:
    agent.step() → LLM call
    ↓
AgentV2 returns AgentDecision::Message("Hi there!")
    ↓
event_bus.publish(CoreEvent::AgentResponse { content: "Hi there!" })
    ↓
TUI run_loop receives event
    ↓
app.start_streaming_final_answer() → displays in chat
    ↓
JobRegistry::complete_job(job_id, result)
```

---

## 7. Key Files Reference

| Component | File |
|-----------|------|
| Orchestrator | `core/src/agent/orchestrator.rs` |
| V2 Agent Core | `core/src/agent/v2/core.rs` |
| V2 Event Loop | `core/src/agent/v2/driver/event_driven.rs` |
| Job Registry | `core/src/agent/v2/jobs.rs` |
| Legacy Agent (V1) | `core/src/agent/core.rs` |
| Terminal UI Integration | `src/terminal/mod.rs` |
| App State (submit_message) | `src/terminal/app/app.rs` |
| Event Handling | `src/terminal/mod.rs` (run_loop) |

---

## 8. Recent Architecture Changes

### 8.1 Tool Failure Tracking & Worker Stall Detection

**Problem**: Workers could retry failed tools indefinitely, wasting tokens.

**Solution**: Added configurable `max_tool_failures` (default: 5):
- Tracked in `AgentV2::consecutive_tool_failures`
- When limit exceeded, agent returns `AgentDecision::Stall { reason, tool_failures }`
- Worker stops and reports failure instead of looping

**Configuration**: `mylm.toml` → `[agent]` → `max_tool_failures = 5`

### 8.2 Escalation Channel for Worker Security

**Problem**: Workers executing shell commands need permission controls.

**Solution**: Added escalation channel (`WorkerShellTool` → `AgentOrchestrator`):
- Workers classify commands as: `Allowed`, `Restricted`, or `Forbidden`
- Restricted commands trigger escalation requests to main agent
- Channel uses `tokio::mpsc` with oneshot responses
- Currently auto-rejects with explanation (UI approval TODO)

**Files**: `core/src/agent/tools/worker_shell.rs`, `core/src/agent/v2/orchestrator/mod.rs`

### 8.3 Event-Driven Loop Fixes

**Problem**: Main loop was busy-waiting, causing UI freezes and excessive logging.

**Changes**:
1. **Removed proactive job status injection** - Agent no longer gets job status on every iteration; only receives `WorkerCompleted` events
2. **Fixed EventBus event handling** - Informational events (`ToolExecuting`, `StatusUpdate`) no longer wake up the main loop
3. **Reduced log verbosity**:
   - AgentV2 initialization: per-tool logs → single total count
   - `process_terminal_data`: removed per-byte logging
   - Worker loop: logs every 10 iterations instead of every iteration
   - `poll_jobs`: only logs when jobs found

### 8.4 Post-Delegate Yield Fix

**Problem**: After spawning workers via `delegate`, main agent immediately called `step()` again instead of waiting.

**Fix**: After `delegate` tool execution:
- Sets `last_observation = None`
- Forces loop into idle wait state
- Agent waits for user input or worker events before continuing

## 9. Conclusion

The myLM agent system is in a transitional state, supporting both V1 (legacy) and V2 (modern) architectures. The V2 design enables powerful background job processing and non-blocking execution, while the V1 wrapper maintains compatibility with the existing UI. Understanding this split is key to navigating the codebase.
