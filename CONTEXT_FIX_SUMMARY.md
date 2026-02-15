# Context Loss Fix Summary

## Problem
The model was forgetting previous messages on subsequent interactions. This was a critical bug where conversation context was lost between turns.

## Root Cause
The `LLMBasedEngine` in `core/src/agent/cognition/llm_engine.rs` was reading `state.history` when building prompts, but `state.history` was **never being updated** with the messages from input events.

The engine's `step()` method handled input events (`UserMessage`, `LLMResponse`, `ToolResult`) but only incremented the step counter without recording the actual messages to the state.

## Fix Applied

### 1. Fixed `LLMBasedEngine::step()` in `llm_engine.rs`

Added proper history tracking for each input type:

**UserMessage:**
```rust
let state_with_user_msg = state.clone()
    .with_message(Message::user(&msg));
// ... use state_with_user_msg instead of state
let next_state = state_with_user_msg.increment_step();
```

**LLMResponse:**
```rust
let state_with_assistant_msg = state.clone()
    .with_message(Message::assistant(&llm_resp.content));
// ... use state_with_assistant_msg instead of state
let next_state = state_with_assistant_msg.increment_step();
```

**ToolResult:**
```rust
let state_with_tool_msg = state.clone()
    .with_message(Message::tool(tool_content));
// ... use state_with_tool_msg instead of state  
let next_state = state_with_tool_msg.increment_step();
```

### 2. Synced Session History in `session.rs`

Added synchronization between `state.history` and `session.history` for persistence:
```rust
// After: self.state = transition.next_state.clone();
self.history = self.state.history.clone();
```

## Files Modified

1. `core/src/agent/cognition/llm_engine.rs` - Added history updates to step() method
2. `core/src/agent/session/session.rs` - Added history sync for persistence

## How It Works Now

```
User Input → Session.run() → engine.step() 
                              ↓
                     Adds message to state.history
                              ↓
                     Returns: Transition { next_state (with history), decision }
                              ↓
                     Session syncs: self.history = self.state.history
                              ↓
                     Next turn: state.history has all previous messages
```

## Verification

After the fix:
1. First user message: `state.history.len() == 1`
2. After LLM response: `state.history.len() == 2`  
3. After tool execution: `state.history.len() == 3`
4. Second turn: `state.history.len() >= 3` (includes previous context)

## Future Improvements

The current fix ensures basic context preservation. Future work should integrate:

1. **Token Management**: Integrate `ContextManager` for:
   - Token counting with proper estimation
   - Automatic pruning when approaching limits
   - Condensation for very long conversations
   - Byte size limits for API safety

2. **Worker Context**: Design shared context for workers:
   - Workers inherit context from parent
   - Workers return summaries that get merged
   - Token budget allocation between parent and workers

3. **Context Persistence**: Save/restore full conversation state
