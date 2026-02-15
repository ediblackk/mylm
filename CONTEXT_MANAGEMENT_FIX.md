# Complete Context Management Fix

## Problem Summary
The model was forgetting previous messages because the conversation history was never being passed to the LLM. The `Context.history` was always empty (`[]`) in the logs.

## Root Causes Found & Fixed

### 1. **State History Not Updated** (`llm_engine.rs`)
**Problem**: The `LLMBasedEngine::step()` method was not adding messages to `state.history`.

**Fix**: Added `with_message()` calls to track history:
```rust
// Before
let next_state = state.clone().increment_step();

// After  
let state_with_user_msg = state.clone()
    .with_message(Message::user(&msg));
let next_state = state_with_user_msg.increment_step();
```

### 2. **Context History Not Set** (`llm_engine.rs`)
**Problem**: The `Context` was created with `Context::new(scratchpad)` which sets `history: Vec::new()`.

**Fix**: Added `with_history()` to pass the state's history:
```rust
let context = crate::agent::types::intents::Context::new(scratchpad)
    .with_system(enhanced_system_prompt)
    .with_history(state_with_user_msg.history.iter().map(|m| { ... }).collect());
```

Also added `with_history()` method to `Context` in `types/intents.rs`.

### 3. **LLM Client Ignored History** (`llm_client.rs`)
**Problem**: `LlmClientCapability::complete()` and `complete_stream()` built messages from scratchpad but **completely ignored `req.context.history`**.

**Fix**: Added history to message building:
```rust
// Add system prompt first
if !req.context.system_prompt.is_empty() {
    messages.push(ChatMessage::system(req.context.system_prompt.clone()));
}

// Add conversation history (NEW!)
for msg in &req.context.history {
    let chat_msg = match msg.role {
        Role::System => ChatMessage::system(msg.content.clone()),
        Role::User => ChatMessage::user(msg.content.clone()),
        Role::Assistant => ChatMessage::assistant(msg.content.clone()),
        Role::Tool => ChatMessage::tool("unknown", "unknown", &msg.content),
    };
    messages.push(chat_msg);
}

// Add current scratchpad as the final user message
messages.push(ChatMessage::user(req.context.scratchpad.clone()));
```

### 4. **Session History Not Synced** (`session.rs`)
**Problem**: The session's history wasn't being updated from state.

**Fix**: Added history sync:
```rust
self.state = transition.next_state.clone();
self.history = self.state.history.clone(); // NEW!
```

## Files Modified

1. `core/src/agent/cognition/llm_engine.rs` - Track history in state, pass to context
2. `core/src/agent/types/intents.rs` - Added `with_history()` method to `Context`
3. `core/src/agent/runtime/impls/llm_client.rs` - Include history in LLM requests
4. `core/src/agent/session/session.rs` - Sync history for persistence

## Verification

Before fix (from debug logs):
```
Context { history: [], system_prompt: "...", scratchpad: "..." }
```

After fix:
```
Context { history: [Message { role: User, content: "hello" }, ...], system_prompt: "...", scratchpad: "..." }
```

## Message Flow After Fix

```
User: "Hello"
  ↓
LLMBasedEngine::step(UserMessage)
  → state.with_message(Message::user("Hello"))
  → Context::new(scratchpad).with_history(state.history)
  → RequestLLM { context: Context { history: [User: "Hello"], ... } }
    ↓
LlmClientCapability::complete(req)
  → messages = [System, User: "Hello", User: scratchpad]
  → LLM API Call
    ↓
LLM Response: "Hi there!"
  ↓
LLMBasedEngine::step(LLMResponse)
  → state.with_message(Message::assistant("Hi there!"))
  → state.history = [User: "Hello", Assistant: "Hi there!"]
    ↓
Next Turn: User: "What's 2+2?"
  → LLM sees: [User: "Hello", Assistant: "Hi there!", User: "What's 2+2?"]
```

## Future: Token Management

The current fix ensures context is preserved. Future work should integrate `ContextManager` for:

- **Token counting** - Track usage vs limits
- **Pruning** - Remove old messages when approaching limit
- **Condensation** - Summarize middle messages for long conversations
- **Byte limits** - Enforce API safety limits

The `core/src/context/manager.rs` has this infrastructure ready to integrate.
