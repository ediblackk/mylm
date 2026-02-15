# Approval Flow Implementation - Complete

## Overview
Implemented a complete approval flow for tool execution that allows users to approve or deny potentially dangerous operations before they execute.

## Architecture

### Flow
```
1. Model wants to execute tool (e.g., shell "rm -rf /")
   ↓
2. Engine emits: AgentDecision::RequestApproval { tool, args }
   ↓
3. ContractRuntime receives Intent::RequestApproval
   ↓
4. Emits OutputEvent::ApprovalRequested to UI
   ↓
5. Blocks on approval_capability.request()
   ↓
6. UI shows: "Approve: shell rm -rf /? (Y/N)"
   ↓
7. User presses Y or N
   ↓
8. UI sends UserInput::Approval via input channel
   ↓
9. Session publishes KernelEvent::ApprovalGiven
   ↓
10. Runtime receives outcome, unblocks
   ↓
11. If approved: executes tool, else: returns error
```

## Files Modified

### Core Library

1. **core/src/agent/runtime/contract_runtime.rs**
   - Added `approval: Arc<dyn ApprovalCapability>` field
   - Updated constructor to include approval capability
   - Modified `RequestApproval` handling to emit `ApprovalRequested` event
   - Updated `Clone` implementation

2. **core/src/agent/factory.rs**
   - Added `approval: Option<Arc<dyn ApprovalCapability>>` field
   - Added `with_approval()` method
   - Updated `create_session()` to wire approval if provided

3. **core/src/agent/runtime/impls/mod.rs**
   - Already had `AutoApproveCapability` and `TerminalApprovalCapability`

### TUI

4. **src/tui/approval.rs** (New)
   - `TuiApprovalCapability` - Implements `ApprovalCapability` trait
   - `PendingApproval` - Holds request + response channel
   - `ApprovalHandle` - For responding to approvals
   - Uses oneshot channels for request/response pattern

5. **src/tui/app/state.rs**
   - Added `ApprovalHandle` import
   - Replaced `pending_approval_tx/rx` with `approval_handle: Option<ApprovalHandle>`

6. **src/tui/event_loop.rs**
   - Updated `AwaitingApproval` key handling
   - Y key: sends `UserInput::Approval { approved: true }`
   - N key: sends `UserInput::Approval { approved: false }`

7. **src/tui/mod.rs**
   - Added `approval` module
   - Already had `ApprovalRequested` event handling (sets `AwaitingApproval` state)

8. **src/tui/agent_setup.rs**
   - Updated `create_session_factory()` signature to accept terminal and approval

9. **src/main.rs**
   - Added `Arc` import
   - Created `TuiApprovalCapability` instance
   - Wired approval into `AgentSessionFactory`

## UI Experience

### Approval Request Display
```
┌─────────────────────────────────────────────────────┐
│  💭 Agent is thinking...                            │
│                                                     │
│  [Chat history...]                                  │
│                                                     │
│  💾 Context compressed: 3 messages summarized...    │
│                                                     │
├─────────────────────────────────────────────────────┤
│  ⚠️  Approve: shell? (Y/N)                          │
├─────────────────────────────────────────────────────┤
│  > _                                                │
└─────────────────────────────────────────────────────┘
```

### User Controls
- **Y** - Approve and execute the tool
- **N** or **Esc** - Deny and cancel the tool execution

## Configuration

Default behavior:
- `AutoApproveCapability` is used by default (approves all)
- When TUI is running, `TuiApprovalCapability` is used instead

Future configuration options:
```toml
[approval]
mode = "interactive"  # auto | interactive

# Auto-approve these patterns
auto_approve = [
    "read_file *",
    "list_dir *",
]

# Always confirm these
always_confirm = [
    "shell rm *",
    "write_file *",
]
```

## Key Design Decisions

1. **Event-Driven**: Uses existing `OutputEvent` system for UI notification
2. **Async-Await**: Approval capability blocks until user responds
3. **Input Channel**: User response sent via session's `UserInput::Approval`
4. **Backward Compatible**: Defaults to auto-approve for non-interactive use

## Testing

Build successful:
```bash
cargo build  # ✓ Compiles
```

### Manual Test Steps
1. Start mylm TUI
2. Ask agent to execute a shell command (e.g., "list files")
3. Agent should show approval prompt if tool requires it
4. Press Y to approve or N to deny
5. Agent should proceed based on user choice

## Future Enhancements

1. **Chat-First Pattern**: Model explains reasoning before requesting approval
2. **Visual Dialog**: Better UI with [Approve] [Deny] buttons
3. **Edit Before Execute**: Allow user to modify command before approval
4. **Auto-Approve Memory**: Remember user choices for similar commands
5. **Dangerous Command Detection**: Highlight high-risk operations
