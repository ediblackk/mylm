# Refactoring Plan: Eliminate stub.rs

## Executive Summary

The `src/tui/stub.rs` file (688 lines) contains a mix of:
1. Types that already exist in `mylm_core` (duplicates)
2. Types that already exist in `src/tui/types.rs` (duplicates)
3. TUI-specific types that belong in `types.rs`
4. A large `App` struct that duplicates `AppStateContainer` in `state.rs`

**Key Finding**: `src/tui/types.rs` already contains most of the same types as `stub.rs`. The duplication exists because `mod.rs` imports from `stub` instead of `types`.

## Current Architecture Problems

```
src/tui/stub.rs ─────┬──> Duplicates mylm_core types (ChatMessage, TokenUsage, etc.)
                     ├──> Duplicates types.rs types (AppState, Focus, Job, etc.)
                     └──> Contains App struct (duplicates AppStateContainer)

src/tui/types.rs ────└──> Has same types but is not used!

src/tui/app/state.rs ──> Has AppStateContainer (similar to App in stub.rs)

src/tui/mod.rs ────────> Re-exports from stub.rs (wrong source!)
```

## Layer Classification

### 1. TUI Layer (belongs in `src/tui/types.rs`)
| Type | Current Location | Action |
|------|-----------------|--------|
| `AppState` | stub.rs, types.rs | Keep in types.rs |
| `Focus` | stub.rs, types.rs, state.rs | Keep in types.rs |
| `TuiEvent` | stub.rs, types.rs, state.rs | Consolidate in types.rs |
| `HelpSystem` | stub.rs, types.rs | Keep in types.rs |
| `JobStatus` | stub.rs, types.rs | Keep in types.rs |
| `ActionType` | stub.rs, types.rs | Keep in types.rs |
| `Job` | stub.rs, types.rs | Keep in types.rs |
| `JobMetrics` | stub.rs, types.rs | Keep in types.rs |
| `ActionLogEntry` | stub.rs, types.rs | Keep in types.rs |
| `JobRegistry` | stub.rs, types.rs | Keep in types.rs |
| `SessionStats` | stub.rs, types.rs | Keep in types.rs |
| `SessionMonitor` | stub.rs, types.rs | Keep in types.rs (stub impl) |
| `SessionMetadata` | stub.rs, types.rs | Keep in types.rs |
| `StructuredScratchpad` | stub.rs, types.rs | Keep in types.rs |

### 2. Agent/Business Logic Layer (belongs in `mylm_core`)
| Type | Current Location | Real Location | Action |
|------|-----------------|---------------|--------|
| `MessageRole` | stub.rs | `core/src/llm/chat.rs` | Use re-export |
| `ChatMessage` | stub.rs | `core/src/llm/chat.rs` | Use re-export |
| `TokenUsage` | stub.rs | `core/src/llm/mod.rs` | Use re-export |
| `MemoryGraph` | stub.rs | `core/src/memory/graph.rs` | Use re-export |
| `Memory` | stub.rs | `core/src/memory/` | Create if needed |
| `MemoryNode` | stub.rs | `core/src/memory/` | Create if needed |
| `ContextManager` | stub.rs | `core/src/context/manager.rs` | Use re-export |
| `ActionStamp` | stub.rs | `core/src/context/action_stamp.rs` | Use re-export |
| `ActionStampType` | stub.rs | `core/src/context/action_stamp.rs` | Use re-export |

### 3. Connection/Networking Layer (already exists)
| Type | Current Location | Real Location | Action |
|------|-----------------|---------------|--------|
| `PtyManager` | stub.rs (stub!) | `src/tui/pty.rs` | Use real impl |
| `spawn_pty` | stub.rs (stub!) | `src/tui/pty.rs` | Use real impl |

### 4. App Struct Analysis

The `App` struct in `stub.rs` (lines 420-645) overlaps with `AppStateContainer` in `state.rs`:

**In stub.rs App but NOT in state.rs AppStateContainer:**
- `stream_state: Option<StreamState>` - streaming parser state
- `stream_escape_next: bool` - streaming parser state
- `stream_key_buffer: String` - streaming parser state
- `stream_lookback: String` - streaming parser state
- `stream_thought: Option<String>` - streaming parser state

**In state.rs AppStateContainer but NOT in stub.rs App:**
- Many additional fields for full session management
- Proper integration with mylm_core types

**Decision**: `AppStateContainer` in `state.rs` is the authoritative version. The streaming parser fields need to be added to it.

## Execution Plan

### Step 1: Update `src/tui/types.rs`
- Ensure all TUI-specific types are present
- Add missing streaming parser types (`StreamState`, `StreamField`) from `state.rs`
- Verify re-exports from mylm_core are correct

### Step 2: Update `src/tui/app/state.rs`
- Add missing streaming parser fields if not present
- Remove duplicate type definitions (`Focus`, `AppState`, `TuiEvent`)
- Import from `types.rs` instead

### Step 3: Update `src/tui/mod.rs`
- Change all re-exports from `stub` to `types`
- Remove `mod stub;` declaration
- Keep event loop code (it's not stub code, it's real TUI logic)

### Step 4: Update `src/tui/app/mod.rs`
- Ensure proper re-exports from `state.rs` and `types.rs`

### Step 5: Delete `src/tui/stub.rs`
- After all imports are updated and code compiles

## File Changes Summary

```
CREATE/MODIFY:
  src/tui/types.rs        - Add StreamState, StreamField; verify all types present
  src/tui/app/state.rs    - Remove duplicate types; import from types.rs
  src/tui/mod.rs          - Change re-exports from stub to types; remove mod stub
  src/tui/app/mod.rs      - Update re-exports

DELETE:
  src/tui/stub.rs         - Remove entirely after migration
```

## Verification Checklist

- [ ] All types in types.rs compile
- [ ] state.rs imports from types.rs
- [ ] mod.rs re-exports from types.rs
- [ ] No imports of stub module remain
- [ ] `cargo build` succeeds
- [ ] `cargo test` passes
- [ ] stub.rs deleted

## Risk Assessment

**Low Risk**: This is primarily an import reorganization. The types already exist in the right places; we just need to use them correctly.

**Key Insight**: `types.rs` was created as the intended home for these types but was never connected. The refactoring is mostly about removing the indirection through `stub.rs`.
