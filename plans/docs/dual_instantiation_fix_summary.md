# Dual Instantiation Fix - Summary

## What Was Fixed

The TUI was creating **two agent instances** when using V2:
1. A V1 `Agent` struct (for "UI compatibility")
2. A separate `AgentV2` for the orchestrator

This caused duplicate logs, wasted memory, and confusion.

## Solution: AgentWrapper

Created `AgentWrapper` enum in `core/src/agent/wrapper.rs` that holds either:
- `V1(Arc<Mutex<Agent>>)` 
- `V2(Arc<Mutex<AgentV2>>)`

**Key insight:** The wrapper holds `Arc<Mutex<>>` internally, so the SAME agent instance can be shared between:
- `AppState` (for UI field access)
- `AgentOrchestrator` (for execution)

## Files Changed

### Core Changes
1. **`core/src/agent/wrapper.rs`** (NEW)
   - `AgentWrapper` enum with V1/V2 variants
   - Async methods for common field access: `history()`, `session_id()`, `scratchpad()`, etc.
   - `as_v1_arc()` / `as_v2_arc()` to extract Arc for orchestrator

2. **`core/src/agent/mod.rs`**
   - Added `pub mod wrapper`
   - Exported `AgentWrapper`

3. **`core/src/agent/v2/core.rs`**
   - Fixed pre-existing bug in `manage_scratchpad()` (`try_write()` returns `Result`, not `Option`)

### TUI Changes
4. **`src/terminal/app/state.rs`**
   - Changed `agent` field from `Arc<Mutex<Agent>>` to `AgentWrapper`
   - Updated `new_with_orchestrator()` constructor
   - Fixed legacy `new()` constructor

5. **`src/terminal/mod.rs`**
   - Complete rewrite of agent creation logic (lines ~253-326)
   - Now creates ONE agent (V1 or V2 based on config)
   - Wraps in `AgentWrapper` and shares Arc with orchestrator
   - Updated all field accesses to use wrapper methods

6. **`src/terminal/app/input.rs`**
   - Updated `save_session()` to use wrapper methods

7. **`src/terminal/app/session.rs`**
   - Updated `trigger_manual_condensation()` to use wrapper

8. **`src/terminal/app/commands.rs`**
   - Updated command handlers to use wrapper methods

## Key Code Changes

### Before (Dual Instantiation)
```rust
// 1. Create V1 agent
let agent = builder.build().await;  // Always V1
let tools = agent.tool_registry.get_all_tools().await;
let agent_arc = Arc::new(Mutex::new(agent));

// 2. If V2 mode, create SECOND agent
if version == V2 {
    let agent_v2 = AgentV2::new_with_config(...);
    let agent_v2_arc = Arc::new(Mutex::new(agent_v2));
    orchestrator = AgentOrchestrator::new_with_agent_v2(agent_v2_arc);
}
```

### After (Single Agent)
```rust
// Create ONE agent based on version
let (agent_wrapper, orchestrator) = if version == V2 {
    let agent_v2 = AgentV2::new_with_config(...);
    let wrapper = AgentWrapper::new_v2(agent_v2);
    let arc = wrapper.as_v2_arc().unwrap();
    let orch = AgentOrchestrator::new_with_agent_v2(arc, ...).await;
    (wrapper, orch)
} else {
    let agent = Agent::new_with_config(...);
    let wrapper = AgentWrapper::new_v1(agent);
    let arc = wrapper.as_v1_arc().unwrap();
    let orch = AgentOrchestrator::new_with_agent_v1(arc, ...).await;
    (wrapper, orch)
};
```

## Benefits

1. **Eliminates duplicate logs** - Only one agent instance logs
2. **Reduces memory usage** - No duplicate scribe, scratchpad, history
3. **Cleaner architecture** - Single source of truth for agent state
4. **Easier debugging** - No confusion about which agent has the "real" state
5. **Sets up for feature flags** - AgentWrapper can be feature-gated later

## Testing Recommendations

1. Test V2 mode thoroughly - this is where the biggest change occurred
2. Verify session save/restore works correctly
3. Check that memory operations (scribe) work properly
4. Test config reload (changing settings mid-session)
5. Verify scratchpad content persists correctly

## Known Limitations

1. **Config reload rebuilds agent** - The current code still rebuilds the agent on config change, but now it properly uses the wrapper pattern
2. **Tool registry access** - V2 uses HashMap, V1 uses ToolRegistry; wrapper exposes both via separate methods
3. **Some methods require pattern matching** - Advanced operations may need to match on V1/V2 variant

## Next Steps

1. **Test the changes** - Verify V2 execution works correctly
2. **Stabilize V2** - Fix any remaining V2 logic bugs
3. **Add feature flags** - Once V2 is stable, add `v1`/`v2`/`memory` features to Cargo.toml
