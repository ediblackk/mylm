# Refactoring Plan: Approval System & Chat Session Architecture

## Goals
1. Fix approval bug (UserMessage treated as approval)
2. Separate concerns: chat, workers, approval, state
3. Unified approval logic with auto-approve + restricted/allowed patterns
4. Remove V1 agent and legacy code
5. Clear UI/Core separation

---

## Current Files to DELETE

| File | Reason |
|------|--------|
| `core/src/agent/v1/` (entire folder) | Legacy V1 agent, marked for deletion |
| `core/src/agent/v2/driver/legacy.rs` | Deprecated V2 run method |
| `core/src/agent/v2/orchestrator/loops.rs` | Split into separate modules |
| `core/src/agent/v2/orchestrator/helpers.rs` | Move functions to appropriate modules |

---

## Current Files to KEEP (but modify)

| File | Changes |
|------|---------|
| `core/src/agent/mod.rs` | Remove V1 exports |
| `core/src/agent/v2/mod.rs` | Remove driver export, update orchestrator |
| `core/src/agent/v2/orchestrator/mod.rs` | Simplify - remove V1 support, use new session |
| `core/src/agent/v2/orchestrator/types.rs` | Add approval-related types |
| `core/src/agent/v2/orchestrator/event_bus.rs` | Keep as-is (clean) |
| `core/src/agent/v2/driver/mod.rs` | Keep only event_driven.rs for workers |
| `core/src/agent/v2/driver/event_driven.rs` | Keep for worker agents |

---

## NEW FILES to CREATE

### 1. `core/src/agent/v2/session/mod.rs`
**Replaces:** `orchestrator/loops.rs::run_chat_session_loop_v2()`

**Purpose:** Main chat session coordinator. Receives events, routes to appropriate handler.

**Structure:**
```rust
pub struct ChatSession {
    agent: Arc<Mutex<AgentV2>>,
    config: SessionConfig,
    state: SessionState,
    approval_handler: ApprovalHandler,
    chat_handler: ChatHandler,
    worker_handler: WorkerHandler,
}

impl ChatSession {
    pub async fn run(&mut self, receiver: Receiver<SessionEvent>) -> Result<()>
    
    // Routes events to appropriate handler
    fn handle_event(&mut self, event: SessionEvent) -> Option<String>
}
```

**Receives events from:**
- UI: User messages, approval responses (Y/N keys)
- Workers: WorkerCompleted, WorkerSpawned, WorkerStalled
- Internal: Interrupt, Tick

**Returns:** Observation string for agent.step()

---

### 2. `core/src/agent/v2/session/approval.rs`
**Replaces:** Approval logic scattered in loops.rs

**Purpose:** Handle ALL approval decisions. Unified logic for auto-approve, restricted, allowed.

**Structure:**
```rust
pub struct ApprovalHandler {
    auto_approve: SharedAutoApprove,
    always_allowed: Vec<String>,     // From config: auto_approve_commands
    restricted: Vec<String>,         // From config: forbidden_commands
    pending_approval: Option<PendingApproval>,
}

pub struct PendingApproval {
    tool: String,
    args: String,
    response_tx: oneshot::Sender<ApprovalResponse>,
}

pub enum ApprovalResponse {
    Approved,
    Rejected { reason: String },
}

impl ApprovalHandler {
    /// Main entry: tool wants to execute
    pub fn check_tool(&mut self, tool: &str, args: &str) -> ApprovalDecision
    
    /// UI calls this when user presses Y/N
    pub fn respond(&mut self, approved: bool) -> Option<PendingApproval>
    
    /// Check if tool matches always_allowed patterns
    fn is_always_allowed(&self, tool: &str, args: &str) -> bool
    
    /// Check if tool matches restricted patterns  
    fn is_restricted(&self, tool: &str, args: &str) -> bool
}

pub enum ApprovalDecision {
    AutoApprove,           // auto_approve ON and not restricted
    AlwaysAllowed,         // Matches whitelist pattern
    RequiresApproval(PendingApproval), // Need user input
    Restricted,            // Forbidden - reject immediately
}
```

**Logic Flow:**
```
Tool Request
    │
    ▼
┌─────────────────┐
│ ALWAYS_ALLOWED? │──Yes──► Execute
│ (whitelist)     │
└────────┬────────┘
         │ No
         ▼
┌─────────────────┐
│ RESTRICTED?     │──Yes──► Reject
│ (blacklist)     │
└────────┬────────┘
         │ No
         ▼
┌─────────────────┐
│ AUTO_APPROVE?   │──Yes──► Execute
└────────┬────────┘
         │ No
         ▼
    Ask User (Y/N)
```

---

### 3. `core/src/agent/v2/session/chat.rs`
**Replaces:** User message handling in loops.rs

**Purpose:** Handle user chat messages, prepare observations for agent.

**Structure:**
```rust
pub struct ChatHandler {
    history: Vec<ChatMessage>,
}

impl ChatHandler {
    /// Process user message
    pub fn handle_message(&mut self, msg: String) -> String
    
    /// Add assistant response to history
    pub fn add_response(&mut self, msg: String)
}
```

---

### 4. `core/src/agent/v2/session/workers.rs`
**Replaces:** Worker event handling in loops.rs

**Purpose:** Handle worker lifecycle events.

**Structure:**
```rust
pub struct WorkerHandler {
    job_registry: JobRegistry,
    active_workers: HashMap<String, WorkerInfo>,
}

pub enum WorkerEvent {
    Spawned { job_id: String, description: String },
    Completed { job_id: String, result: String },
    Stalled { job_id: String, reason: String },
    StatusUpdate { job_id: String, message: String },
}

impl WorkerHandler {
    pub fn handle_event(&mut self, event: WorkerEvent) -> Option<String>
    
    /// Format observation for agent when worker completes
    pub fn format_completion_observation(&self, job_id: &str, result: &str) -> String
}
```

---

### 5. `core/src/agent/v2/session/state.rs`
**Replaces:** State tracking scattered in loops.rs

**Purpose:** Track session state (step count, rejections, etc.)

**Structure:**
```rust
pub struct SessionState {
    step_count: usize,
    consecutive_rejections: usize,
    max_consecutive_rejections: usize,
    auto_confirm_count: usize,
    max_auto_confirm: usize,
    shutdown_requested: bool,
}

impl SessionState {
    pub fn new(config: &SessionConfig) -> Self
    
    pub fn increment_step(&mut self) -> Result<(), String> // Error if max exceeded
    pub fn record_rejection(&mut self) -> bool // Returns true if max reached
    pub fn reset_rejections(&mut self)
    pub fn request_shutdown(&mut self)
}
```

---

### 6. `core/src/agent/v2/session/events.rs`
**NEW:** Unified event types for session

**Purpose:** Single event type for all session inputs.

**Structure:**
```rust
pub enum SessionEvent {
    // From UI
    UserMessage(String),
    ApprovalResponse { approved: bool },
    Interrupt,
    
    // From workers
    WorkerSpawned { job_id: String, description: String },
    WorkerCompleted { job_id: String, result: String },
    WorkerStalled { job_id: String, reason: String },
    
    // Internal
    Heartbeat,
}
```

---

### 7. `core/src/agent/v2/orchestrator/session_bridge.rs`
**Replaces:** `start_chat_session()` logic in mod.rs

**Purpose:** Bridge between orchestrator and new session. Spawns session task.

**Structure:**
```rust
pub struct SessionBridge;

impl SessionBridge {
    pub async fn start(
        orchestrator: &AgentOrchestrator,
        history: Vec<ChatMessage>,
    ) -> (TaskHandle, SessionHandle)
}
```

---

## UI Side Changes

### `src/terminal/event_loop.rs`
**Changes:**
- Remove `ToolAwaitingApproval` → `AwaitingApproval` state conversion
- Remove key handler for Y/N in `handle_chat_keys()`
- Simplify: All input goes to `submit_message()`
- `submit_message()` checks `AppState` and routes to orchestrator

**New behavior:**
- UI receives `ToolAwaitingApproval` event → shows "Approve? (Y/n)" 
- User presses Y/N (any key) → sends to core via `ApprovalResponse` event
- Core decides what Y/N means (only 'y'/'Y' = approve)

---

## Migration Steps

### Phase 1: Create new session module (no changes to old code)
1. Create `session/` folder
2. Create stub files (approval.rs, chat.rs, workers.rs, state.rs, events.rs)
3. Implement `ApprovalHandler` with logic

### Phase 2: Replace chat session loop
1. Implement `ChatSession::run()` using new handlers
2. Update `orchestrator/mod.rs` to use new session
3. Keep old `loops.rs` as backup (don't delete yet)

### Phase 3: Delete legacy
1. Delete V1 agent folder
2. Delete `loops.rs` (old)
3. Delete `helpers.rs` (move needed functions)
4. Delete `driver/legacy.rs`

### Phase 4: UI cleanup
1. Simplify `event_loop.rs` approval handling
2. Test all scenarios

---

## Configuration Changes

### `core/src/config/v2/types.rs`
**Add to `AgentPermissions`:**
```rust
pub struct AgentPermissions {
    // ... existing fields ...
    
    /// Commands that are ALWAYS auto-approved even when auto_approve is OFF
    /// Examples: ["ls *", "cat *", "pwd", "echo *"]
    pub always_allowed_commands: Option<Vec<String>>,
    
    /// Commands that are NEVER auto-approved even when auto_approve is ON
    /// Examples: ["rm -rf *", "dd *", "mkfs *"]
    pub restricted_commands: Option<Vec<String>>,
}
```

---

## File Summary

| Status | Count | Files |
|--------|-------|-------|
| DELETE | 4 | v1/, legacy.rs, loops.rs, helpers.rs |
| MODIFY | 5 | agent/mod.rs, v2/mod.rs, orchestrator/mod.rs, types.rs, event_loop.rs |
| CREATE | 7 | session/mod.rs, approval.rs, chat.rs, workers.rs, state.rs, events.rs, session_bridge.rs |

**Net change:** +3 files, but much cleaner separation.
