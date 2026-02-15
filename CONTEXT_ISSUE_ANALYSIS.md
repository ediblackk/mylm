# Context Loss Issue Analysis

## Problem Statement
The model forgets previous messages on subsequent interactions. This is a critical bug where conversation context is lost between turns.

## Root Cause Analysis

### Current Architecture Flow

```
User Input → Session.run() → engine.step() → LLMBasedEngine
                              ↓
                     Reads: state.history (ALWAYS EMPTY!)
                              ↓
                     Returns: Transition { next_state, decision }
                              ↓
                     Session stores to: self.history (correct)
                     But: state.history is NOT updated
```

### The Bug

**Location**: `core/src/agent/cognition/llm_engine.rs` in `LLMBasedEngine::step()`

The engine has access to:
1. `state` - Contains `history: Vec<Message>` (empty, never populated)
2. `input` - The current `InputEvent` (UserMessage, ToolResult, etc.)

**Problem**: The engine reads `state.history` when building prompts (line 225), but `state.history` is **never updated** with the messages from input events.

```rust
// In LLMBasedEngine::build_full_prompt() - line 224-244
fn build_full_prompt(&self, state: &AgentState) -> String {
    let history = format_history(&state.history);  // ← ALWAYS EMPTY!
    // ... rest of prompt building
}
```

### Where History IS Tracked (but not used)

In `Session::run()` (session.rs:300-304):
```rust
let transition = self.engine.step(&self.state, last_observation.clone())?;
self.state = transition.next_state.clone();  // ← Replaces state, history is still empty!
```

The session has its own `self.history` that gets persisted, but this is NOT the same as `state.history` used by the engine.

### Where History Should Be Updated

In `LLMBasedEngine::step()`, when handling different input events:

1. **UserMessage** - Should add user message to history
2. **LLMResponse** - Should add assistant response to history
3. **ToolResult** - Should add tool result to history

Currently, none of these update `state.history`.

## Evidence from Code

### 1. Empty History Initialization
`AgentState::new()` creates state with empty history:
```rust
pub fn new(max_steps: usize) -> Self {
    Self {
        history: Vec::new(),  // ← Empty!
        // ...
    }
}
```

### 2. No History Updates in Engine Step

In `llm_engine.rs`, the `step()` method handles:
- `UserMessage` - Creates `RequestLLM` but doesn't update history
- `LLMResponse` - Parses response but doesn't add to history
- `ToolResult` - Creates new `RequestLLM` but doesn't update history

The state is only incremented via `state.clone().increment_step()`, which doesn't touch history.

### 3. Unused with_message Method

`AgentState::with_message()` exists but is never called:
```rust
pub fn with_message(mut self, message: Message) -> Self {
    self.history.push(message);
    self
}
```

## Proposed Fix

### Phase 1: Immediate Fix (Critical)

Modify `LLMBasedEngine::step()` to update history before returning transitions:

1. On `UserMessage(msg)`:
   ```rust
   let next_state = state.clone()
       .with_message(Message::user(&msg))
       .increment_step();
   ```

2. On `LLMResponse`:
   ```rust
   let next_state = state.clone()
       .with_message(Message::assistant(&response.content))
       .increment_step();
   ```

3. On `ToolResult`:
   ```rust
   let next_state = state.clone()
       .with_message(Message::tool(&output))
       .increment_step();
   ```

### Phase 2: Context Manager Integration (Important)

Add token-aware context management:

1. Integrate `ContextManager` from `core/src/context/manager.rs` into the session layer
2. Add context pruning before LLM calls
3. Add condensation when approaching token limits

### Phase 3: Worker Context Sharing (Future)

Design a shared context system for workers:
1. Workers inherit context from parent
2. Workers return summaries that get merged
3. Token budget allocation between parent and workers

## Test Verification

After the fix, verify:
1. First user message: `state.history.len() == 1`
2. After LLM response: `state.history.len() == 2`
3. After tool execution: `state.history.len() == 3`
4. Second turn: `state.history.len() >= 3` (includes previous context)

## Files to Modify

1. `core/src/agent/cognition/llm_engine.rs` - Add history updates
2. `core/src/agent/session/session.rs` - Optional: sync session.history with state.history
3. `core/src/context/manager.rs` - Integrate into session layer

## Current Context Manager (Good Foundation)

The existing `ContextManager` in `core/src/context/manager.rs` has:
- Token counting with character-based estimation
- Pruning (keeps recent, preserves system prompt)
- Condensation (summarizes middle messages)
- Byte size limits for API safety
- Action stamps for tracking

This should be integrated at the session layer before sending to LLM.
