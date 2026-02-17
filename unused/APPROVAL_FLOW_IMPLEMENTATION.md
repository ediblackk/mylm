# Approval Flow Implementation Status

## Phase 1: Core Infrastructure (DONE)

### 1. TuiApprovalCapability (`src/tui/approval.rs`)
- Created new approval capability for TUI
- Uses oneshot channels for request/response pattern
- Supports pending approval queue
- Methods: `approve()`, `deny()`, `has_pending()`, `get_pending()`

### 2. ContractRuntime Integration (`core/src/agent/runtime/contract_runtime.rs`)
- Added `approval: Arc<dyn ApprovalCapability>` field
- Updated `new()` and `with_tools()` to use `AutoApproveCapability` by default
- Added `with_approval()` method for custom approval
- Updated `RequestApproval` handling to use approval capability
- Fixed `Clone` implementation

### 3. Factory Integration (`core/src/agent/factory.rs`)
- Added `approval: Option<Arc<dyn ApprovalCapability>>` field
- Added `with_approval()` method
- Updated `create_session()` to wire approval if provided

### 4. Agent Setup (`src/tui/agent_setup.rs`)
- Updated `create_session_factory()` to accept terminal and approval

## What's Left (Phase 2 & 3)

### Phase 2: UI Integration
1. **Add approval state to AppStateContainer**
   - Track pending approval
   - Store ApprovalHandle

2. **Handle ApprovalRequested event in TUI**
   ```rust
   OutputEvent::ApprovalRequested { intent_id, tool, args } => {
       app.state = AppState::PendingApproval { intent_id, tool, args };
   }
   ```

3. **Create approval UI dialog**
   - Show tool name and arguments
   - [Approve] [Deny] buttons
   - Or use chat input: user types "yes" or "no"

4. **Wire up in main.rs**
   ```rust
   let (approval_capability, mut approval_rx) = TuiApprovalCapability::new();
   let approval_handle = ApprovalHandle::new(Arc::new(approval_capability));
   
   let factory = create_session_factory(&config, None, Some(approval_arc));
   
   // Spawn task to handle approval events
   tokio::spawn(async move {
       while let Some(pending) = approval_rx.recv().await {
           // Send TuiEvent to show approval dialog
       }
   });
   ```

### Phase 3: Chat-First Enhancement
1. **Update LLMBasedEngine to explain before requesting**
   - Generate reasoning text
   - Include in ApprovalRequest context

2. **Conversational UI**
   ```
   💭 I'd like to execute: shell "rm -rf /tmp/old_logs"
   
   Reasoning: The temp directory is taking up 5GB of space...
   
   [Proceed] [Cancel] [Edit...]
   ```

## Usage Example

### Basic Approval (Auto-approve by default)
```rust
// No approval needed - uses AutoApproveCapability
let factory = AgentSessionFactory::new(config);
```

### Interactive Approval
```rust
let (capability, mut rx) = TuiApprovalCapability::new();
let capability_arc = Arc::new(capability);

let factory = create_session_factory(&config, terminal, Some(capability_arc));

// In UI task
while let Some(pending) = rx.recv().await {
    // Show approval dialog
    // On user action:
    handle.approve().await; // or handle.deny(reason).await
}
```

## Configuration

Future configuration options:
```toml
[approval]
mode = "chat_first"  # auto | prompt | chat_first

auto_approve = ["read_file *", "list_dir *"]
always_confirm = ["shell rm *", "write_file *"]
```

## Files Modified
1. `src/tui/approval.rs` - New
2. `core/src/agent/runtime/contract_runtime.rs`
3. `core/src/agent/factory.rs`
4. `src/tui/agent_setup.rs`
5. `src/tui/mod.rs`

## Build Status
✅ Compiles successfully
