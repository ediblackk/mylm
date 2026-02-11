# Implementation: Non-Blocking Approval Flow

This document explains how Codex implements a non-blocking approval system for sensitive actions (like command execution or file changes). This architecture allows the agent to request user confirmation without freezing the system or blocking UI responsiveness.

## 1. Problem Statement

Traditional agent implementations often use a blocking `confirm_action()` call. This is problematic because:
- **UI Freeze**: The entire application or thread becomes unresponsive while waiting for user input.
- **Async Mismatch**: Mixing blocking calls in an `async` environment can lead to thread pool exhaustion or deadlocks.
- **Interruption Difficulty**: It's hard to cancel a pending request or handle system shutdown gracefully if a thread is blocked.
- **Batching Support**: A blocking model makes it difficult to queue multiple approval requests or process them in batch.

## 2. Solution Architecture

Codex solves this using an **event-based, asynchronous correlation** pattern.

### High-Level Architecture (Mermaid)

```mermaid
sequence_flow
    participant Agent as Agent Core
    participant Session as Session/TurnState
    participant UI as Frontend/TUI
    participant User as User

    Agent->>Session: request_command_approval(command)
    Session->>Session: Create oneshot channel (tx, rx)
    Session->>Session: Store tx in TurnState[sub_id]
    Session-->>UI: Emit ExecApprovalRequestEvent
    Session->>Session: await rx (non-blocking)
    
    UI->>UI: Enqueue request in ApprovalOverlay
    UI->>User: Display modal
    User->>UI: Approve/Deny
    UI->>Agent: Submit Op::ExecApproval(sub_id, decision)
    
    Agent->>Session: notify_approval(sub_id, decision)
    Session->>Session: Remove tx from TurnState
    Session->>Session: Send decision to rx
    Session-->>Agent: rx completes, resume execution
```

## 3. Core Components

### 3.1 ApprovalRequest Data Structures
Located in [`codex-rs/protocol/src/approvals.rs`](codex-rs/protocol/src/approvals.rs).

```rust
pub struct ExecApprovalRequestEvent {
    pub call_id: String,
    pub turn_id: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub reason: Option<String>,
    // ...
}
```

### 3.2 TurnState (Correlation Registry)
Located in [`codex-rs/core/src/state/turn.rs`](codex-rs/core/src/state/turn.rs). It maps sub-task IDs to `oneshot::Sender` handles.

```rust
pub(crate) struct TurnState {
    pending_approvals: HashMap<String, oneshot::Sender<ReviewDecision>>,
    // ...
}
```

### 3.3 ApprovalOverlay (Queue Manager)
Located in [`codex-rs/tui/src/bottom_pane/approval_overlay.rs`](codex-rs/tui/src/bottom_pane/approval_overlay.rs). It manages a client-side queue of pending requests.

```rust
pub(crate) struct ApprovalOverlay {
    current_request: Option<ApprovalRequest>,
    queue: Vec<ApprovalRequest>,
    // ...
}
```

## 4. Message Flow & Concurrency

### 4.1 Initiating Request
The `Session` creates a correlation point and yields control back to the executor using `await` on a `oneshot::Receiver`.

```rust
// codex-rs/core/src/codex.rs
pub async fn request_command_approval(
    &self,
    turn_context: &TurnContext,
    // ...
) -> ReviewDecision {
    let (tx_approve, rx_approve) = oneshot::channel();
    
    // Store the sender for later correlation
    {
        let mut active = self.active_turn.lock().await;
        let mut ts = active.turn_state.lock().await;
        ts.insert_pending_approval(sub_id, tx_approve);
    }

    // Emit the event to the UI
    self.send_event(turn_context, event).await;
    
    // Non-blocking wait for the response
    rx_approve.await.unwrap_or_default()
}
```

### 4.2 Handling Responses
The UI sends an `Op` (Operation) back to the agent loop.

```rust
// codex-rs/core/src/codex.rs
pub async fn notify_approval(&self, sub_id: &str, decision: ReviewDecision) {
    let entry = {
        let mut active = self.active_turn.lock().await;
        let mut ts = active.turn_state.lock().await;
        ts.remove_pending_approval(sub_id)
    };
    if let Some(tx_approve) = entry {
        tx_approve.send(decision).ok(); // Resumes the request_command_approval awaiter
    }
}
```

### 4.3 Async Interruption (`select!`)
When waiting for approval, the system must remain interruptible (e.g., user hits Ctrl-C).

```rust
// codex-rs/core/src/codex_delegate.rs
async fn await_approval_with_cancel<F>(
    fut: F,
    cancel_token: &CancellationToken,
) -> ReviewDecision 
where F: Future<Output = ReviewDecision> 
{
    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => {
            ReviewDecision::Abort
        }
        decision = fut => {
            decision
        }
    }
}
```

## 5. UI Queue Management

The TUI implements an `ApprovalOverlay` that can handle multiple incoming requests without losing track.

1. **`enqueue_request`**: When a new event arrives, if a modal is already showing, the new request is pushed to `self.queue`.
2. **`advance_queue`**: When the user makes a decision, the current modal is dismissed, and the next item from `self.queue` is popped and displayed.
3. **Correlation ID**: Every request/response pair uses a unique `id` (usually the `sub_id` or `call_id`) to ensure the decision maps back to the correct agent task.

## 6. Frontend Integration (TypeScript SDK)

On the frontend (VSCode/Web), the same pattern applies:
1. Listen for `ExecApprovalRequest` events.
2. Render a UI notification or modal.
3. Call the backend API (e.g., `codex.submit(Op.ExecApproval(...))`) with the user's choice.

## 7. Best Practices & Lessons

- **Correlation IDs**: Always use a unique identifier for requests to avoid "late arrivals" from previous turns causing incorrect approvals.
- **Timeouts**: Consider adding a timeout to `rx_approve.await` to prevent the agent from hanging indefinitely if the UI crashes.
- **Default Actions**: If a channel is closed or a task is cancelled, default to `Denied` or `Abort` for safety.
- **Backpressure**: If the approval queue grows too large, the UI should provide an "Abort All" option.
- **State Cleaning**: Ensure `TurnState` is cleared when a turn completes or is aborted to prevent memory leaks from unused `oneshot` senders.

## 8. Migration Path

To move from blocking to non-blocking:
1. Replace `fn confirm()` with `async fn confirm()`.
2. Introduce a registry (HashMap) to store pending request handles.
3. Decouple the request (Event) from the response (API call/Operation).
4. Use `oneshot` or `watch` channels to bridge the gap between the agent loop and the correlation registry.
