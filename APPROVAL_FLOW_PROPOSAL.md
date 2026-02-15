# Approval Flow Reintegration Proposal

## Current State Analysis

### Old Implementation (unused/agent_old/)
**Pattern:** Execute-first, approval-before-action
```rust
// In orchestrator loops
if !config.auto_approve {
    event_bus.publish(CoreEvent::ToolAwaitingApproval { tool, args, approval_id });
    
    // Wait for approval from UI channel
    match approval_rx.recv().await {
        Some(true) => { /* execute tool */ }
        Some(false) => { /* skip tool */ }
    }
}
```

**Pros:** Simple, blocks until decision
**Cons:** No context, just raw tool/args shown to user

### New Architecture (core/)
**Components exist:**
1. `ApprovalCapability` trait - for requesting approval
2. `AgentDecision::RequestApproval(ApprovalRequest)` - chat-first pattern
3. `UserInput::Approval { intent_id, approved }` - user response
4. `OutputEvent::ApprovalRequested` - UI notification
5. `ApprovalPolicy` - configurable policies

**Current Gap:** `contract_runtime.rs` just auto-approves:
```rust
Intent::RequestApproval(_req) => {
    // For now, auto-approve (TODO: wire to terminal approval)
    let outcome = ApprovalOutcome::Granted;  // <-- Always approves!
}
```

## Proposed Solution: Chat-First, Act-After-Approval

### Pattern
```
1. Model wants to execute: shell "rm -rf /"
   ↓
2. Engine emits: RequestApproval {
       thought: "I need to clean up the temp directory",
       tool: "shell",
       args: "rm -rf /"
   }
   ↓
3. UI shows: "💭 I need to clean up the temp directory
              
              🔧 Proposed action: shell rm -rf /
              
              [Proceed] [Cancel] [Modify]"
   ↓
4. User clicks [Proceed] or says "yes"
   ↓
5. Intent converted to: CallTool { shell "rm -rf /" }
   ↓
6. Tool executes
```

### Benefits
1. **Contextual** - User understands WHY the action is needed
2. **Educational** - Model explains its reasoning
3. **Conversational** - Feels natural, not intrusive
4. **Flexible** - User can ask for modification

## Implementation Plan

### Phase 1: Wire Up Existing Infrastructure

**1. Create TUI Approval Capability**
```rust
// src/tui/approval.rs
pub struct TuiApprovalCapability {
    event_tx: UnboundedSender<TuiEvent>,
}

#[async_trait]
impl ApprovalCapability for TuiApprovalCapability {
    async fn request(&self, ctx: &RuntimeContext, req: ApprovalRequest) 
        -> Result<ApprovalOutcome, ApprovalError> {
        // Send ApprovalRequested event to UI
        self.event_tx.send(TuiEvent::ApprovalRequested(req));
        
        // Wait for user response via channel
        // (Need to add response channel)
    }
}
```

**2. Update ContractRuntime to Use Approval**
```rust
// core/src/agent/runtime/contract_runtime.rs
Intent::RequestApproval(req) => {
    // Use approval capability instead of auto-approving
    match self.approval.request(&ctx, req).await {
        Ok(ApprovalOutcome::Granted) => {
            // Convert to CallTool and execute
            let tool_intent = Intent::CallTool(req.into_tool_call());
            self.execute_intent(tool_intent).await
        }
        Ok(ApprovalOutcome::Denied { reason }) => {
            Ok(Observation::ApprovalCompleted { 
                outcome: ApprovalOutcome::Denied { reason } 
            })
        }
        Err(e) => Err(RuntimeError::ApprovalFailed(e)),
    }
}
```

**3. Add UI Event Handling**
```rust
// src/tui/mod.rs - handle_agent_event
OutputEvent::ApprovalRequested { intent_id, tool, args } => {
    app.state = AppState::PendingApproval { 
        intent_id, tool, args 
    };
    // Show approval UI
}
```

### Phase 2: Chat-First Enhancement

**Update LLMBasedEngine to explain before requesting:**
```rust
// When tool needs approval
let scratchpad = format!(
    "I'd like to execute: {} {}\n\n\
     Reasoning: {}\n\n\
     Should I proceed?",
    tool_name, args, reasoning
);

AgentDecision::RequestApproval(ApprovalRequest {
    tool: tool_name,
    args,
    context: scratchpad,  // User sees this
})
```

**UI shows conversational approval:**
```
💭 I'd like to execute: shell "rm -rf /tmp/old_logs"

Reasoning: The /tmp/old_logs directory is taking up 5GB of space 
and hasn't been accessed in 30 days. Cleaning it up will free 
space for the new build.

🔧 Proposed action: Delete old logs

[y/N]: 
```

### Phase 3: Advanced Features

1. **Modify Before Execute**
   ```
   [Proceed] [Cancel] [Edit...]
   
   User clicks Edit → Can modify args → Then approve
   ```

2. **Auto-Approve Patterns**
   ```rust
   // Config-driven auto-approval
   auto_approve: [
       "ls *",           // Always approve ls
       "read_file *",    // Always approve reading
       "!rm -rf /"       // Never approve root delete
   ]
   ```

3. **Approval Memory**
   ```
   "You approved similar command 3 times, 
    auto-approving this time. [Don't auto-approve]"
   ```

## Files to Modify

### Core Library
1. `core/src/agent/runtime/contract_runtime.rs` - Wire approval capability
2. `core/src/agent/runtime/capability.rs` - Add response channel support
3. `core/src/agent/cognition/llm_engine.rs` - Generate thought for approval

### TUI
4. `src/tui/approval.rs` - New: TUI approval capability
5. `src/tui/mod.rs` - Handle ApprovalRequested event
6. `src/tui/ui.rs` - Render approval UI
7. `src/tui/app/state.rs` - Add pending approval state
8. `src/tui/app/commands.rs` - Add /approve, /deny commands

### Factory
9. `core/src/agent/factory.rs` - Wire approval capability in builder

## Configuration

```rust
// config.toml
[approval]
mode = "chat_first"  # Options: auto, prompt, chat_first

# Auto-approve these patterns
auto_approve = [
    "read_file *",
    "list_dir *",
    "search *",
]

# Never auto-approve these (always ask)
always_confirm = [
    "shell rm *",
    "write_file *",
    "shell sudo *",
]
```

## Migration from Old Code

**Reusable from unused/:**
- Pattern matching for command safety (permissions.rs)
- Auto-approve logic
- Event publishing pattern

**New approach needed:**
- Channel-based response (old used broadcast, new uses oneshot)
- Chat-first messaging (old was execute-first)
- TUI integration (old was generic event bus)

## Priority

**P0 (Critical):** Wire up basic approval flow
- TuiApprovalCapability
- ContractRuntime integration
- Basic approve/deny UI

**P1 (Important):** Chat-first pattern
- Model explains before requesting
- Conversational UI

**P2 (Nice):** Advanced features
- Modify before execute
- Approval memory
- Config-driven policies
