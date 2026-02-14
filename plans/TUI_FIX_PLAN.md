# TUI Compilation Error Fix Plan

## Error Analysis Summary

Total errors: 87 compilation errors across multiple categories.

## Category 1: Missing Modules (4 modules)

### 1.1 `tui::draw` module
- **Location**: `src/tui/mod.rs:401`
- **Usage**: `crate::tui::draw::draw_ui(f, app)`
- **Status**: Module does not exist
- **Fix**: Create `src/tui/draw.rs` with `draw_ui` function

### 1.2 `tui::event_loop` module
- **Location**: `src/tui/mod.rs:382,426,461`
- **Usage**: `handle_key_event()`, `handle_mouse_event()`
- **Status**: Module does not exist
- **Fix**: Create `src/tui/event_loop.rs` with event handlers

### 1.3 `tui::stub` module
- **Location**: `src/main.rs:82,101,118,136`
- **Usage**: `tui::stub::ChatMessage`
- **Status**: Module does not exist
- **Fix**: Either create stub module OR update main.rs to use `mylm_core::llm::chat::ChatMessage`

### 1.4 `tui::session` module
- **Location**: `src/tui/app/input.rs:440,451`
- **Usage**: `crate::tui::session::Session`
- **Status**: Module does not exist
- **Fix**: Session type exists in `state.rs` as stub, update imports

## Category 2: Missing Types (2 types)

### 2.1 `TuiEvent` not found in `app::state`
- **Location**: `src/tui/app/session.rs:52`
- **Status**: `TuiEvent` is defined in `types.rs`, not `state.rs`
- **Fix**: Update re-exports in `app/mod.rs`

### 2.2 `Session` missing fields
- **Location**: `src/tui/app/input.rs:458-462`
- **Expected fields**: `total_tokens`, `input_tokens`, `output_tokens`
- **Actual fields**: `cost`, `active_context_tokens`, `max_context_tokens`
- **Fix**: Update `SessionStats` struct or update usage

## Category 3: Missing AppStateContainer Fields (5 fields)

### 3.1 `orchestrator` field
- **Location**: `src/tui/mod.rs:68`, `src/tui/app/app.rs:108`, `src/tui/app/commands.rs:234`
- **Status**: Field does not exist in `AppStateContainer`
- **Fix**: Add `orchestrator: Option<AgentOrchestrator>` field

### 3.2 `agent` field
- **Location**: `src/tui/app/input.rs:465,466`, `src/tui/app/session.rs:56`, `src/tui/app/commands.rs:38,72`
- **Status**: Field does not exist
- **Fix**: Either add field or use `agent_session_factory` instead

### 3.3 `pending_approval` field
- **Location**: `src/tui/mod.rs:299,338`
- **Status**: Similar field `pending_approval_tx` exists
- **Fix**: Add `pending_approval: Option<(String, String, String)>` field

### 3.4 `stream_in_final` field
- **Location**: `src/tui/mod.rs:332`
- **Status**: Field does not exist
- **Fix**: Add `stream_in_final: bool` field

### 3.5 `save_session_request` field
- **Location**: `src/tui/mod.rs:393,397`
- **Status**: Field does not exist
- **Fix**: Add `save_session_request: bool` field

## Category 4: Config Method/Field Mismatches (8 issues)

### 4.1 `profile_names()` method
- **Location**: `src/tui/app/commands.rs:147`
- **Status**: Method does not exist
- **Fix**: Use `providers.keys()` or add method to Config

### 4.2 `profile` field
- **Location**: `src/tui/app/commands.rs:158,182,285,308`
- **Status**: Field is `active_profile` not `profile`
- **Fix**: Replace `config.profile` with `config.active_profile`

### 4.3 `set_profile_model_override()` method
- **Location**: `src/tui/app/commands.rs:188,313,326`
- **Status**: Method does not exist
- **Fix**: Implement in Config or update profile.model directly

### 4.4 `set_profile_max_iterations()` method
- **Location**: `src/tui/app/commands.rs:199`
- **Status**: Method does not exist
- **Fix**: Implement in Config or update profile.max_iterations directly

### 4.5 `get_effective_endpoint_info()` method
- **Location**: `src/tui/app/commands.rs:286`
- **Status**: Method does not exist
- **Fix**: Remove or implement

### 4.6 `get_endpoint_info()` method
- **Location**: `src/tui/app/commands.rs:287`
- **Status**: Method does not exist
- **Fix**: Remove or implement

### 4.7 `get_profile_info()` method
- **Location**: `src/tui/app/commands.rs:288`
- **Status**: Method does not exist
- **Fix**: Use `profiles.get()` directly

### 4.8 `save(None)` signature
- **Location**: `src/tui/app/commands.rs:397,419`
- **Status**: `save()` requires path parameter
- **Fix**: Use `save_default()` instead

## Category 5: JobRegistry Missing Methods (3 methods)

### 5.1 `list_active_jobs()`
- **Location**: `src/tui/app/commands.rs:444`
- **Status**: Only `list_all_jobs()` exists
- **Fix**: Implement or filter `list_all_jobs()` results

### 5.2 `cancel_job()`
- **Location**: `src/tui/app/commands.rs:516`
- **Status**: Method does not exist
- **Fix**: Implement method

### 5.3 `cancel_all_jobs()`
- **Location**: `src/tui/app/commands.rs:536`
- **Status**: Method does not exist
- **Fix**: Implement method

## Category 6: SessionMonitor Missing Methods (2 methods)

### 6.1 `duration()`
- **Location**: `src/tui/app/input.rs:462`
- **Status**: Method does not exist
- **Fix**: Implement method

### 6.2 `add_usage()`
- **Location**: `src/tui/app/input.rs:397`
- **Status**: Method does not exist
- **Fix**: Implement method

## Category 7: Type Mismatches (5 issues)

### 7.1 `output_rx` type mismatch
- **Location**: `src/tui/mod.rs:69`
- **Expected**: `broadcast::Receiver<OutputEvent>`
- **Found**: `UnboundedReceiver<OutputEvent>`
- **Fix**: Change field type or convert

### 7.2 `run_tui_session` return type
- **Location**: `src/main.rs:86-144`
- **Expected**: `Result<TuiResult, io::Error>`
- **Actual**: `Result<(), io::Error>`
- **Fix**: Update function signature to return `TuiResult`

### 7.3 `JoinHandle` result type
- **Location**: `src/tui/mod.rs:483-489`
- **Issue**: Pattern matching on `Result<(), JoinError>` incorrectly
- **Fix**: Update pattern matching

### 7.4 `pacore_rounds` type
- **Location**: `src/tui/app/commands.rs:395-396`
- **Expected**: `usize`
- **Found**: `String`
- **Fix**: Parse string to usize or change field type

### 7.5 `mylm_core::agent::v2::jobs::JobStatus`
- **Location**: `src/tui/app/commands.rs:481-486`
- **Status**: `v2` module does not exist
- **Fix**: Use `crate::tui::JobStatus` instead

## Implementation Priority

### Phase 1: Core Infrastructure (Critical)
1. Create `src/tui/draw.rs` with `draw_ui` function
2. Create `src/tui/event_loop.rs` with event handlers
3. Fix `run_tui_session` signature and return type
4. Add missing fields to `AppStateContainer`

### Phase 2: Type Fixes (High)
1. Fix `TuiEvent` re-exports
2. Fix `SessionStats` fields
3. Fix `output_rx` type
4. Fix `JobStatus` imports

### Phase 3: Config Compatibility (Medium)
1. Add missing Config methods or update usage
2. Fix field name mismatches

### Phase 4: Stub Implementations (Low)
1. Implement `JobRegistry` methods
2. Implement `SessionMonitor` methods
3. Create `stub` module or update main.rs

## Files to Modify

1. `src/tui/mod.rs` - Add modules, fix function signatures
2. `src/tui/draw.rs` - CREATE: UI rendering
3. `src/tui/event_loop.rs` - CREATE: Event handling
4. `src/tui/types.rs` - Add missing methods to stubs
5. `src/tui/app/state.rs` - Add missing fields
6. `src/tui/app/mod.rs` - Fix re-exports
7. `src/tui/app/commands.rs` - Fix Config usage
8. `src/tui/app/input.rs` - Fix Session/SessionStats usage
9. `src/main.rs` - Fix stub imports
