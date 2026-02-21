# TODO: Implement Commonboard Claim Enforcement

## Background
The commonboard `claim` action currently exists but is purely advisory - workers can claim resources, but tools don't check claims before operating. This creates false confidence in coordination.

## Goal
Make `claim` actually prevent conflicting operations by enforcing claims in file-modifying tools.

## Implementation Checklist

### Phase 1: Core Infrastructure
- [ ] Add `release` action to commonboard tool (currently missing, causes errors)
- [ ] Add claim expiration/timeout mechanism (prevent stuck claims on worker crash)
- [ ] Consider atomic check-and-claim operation (reduce race window)

### Phase 2: Tool Enforcement
Modify these tools to check claims before write operations:

#### `write_file` tool (`core/src/agent/tools/fs.rs`)
```rust
// Before writing:
if let Some(claimed_by) = commonbox.is_resource_claimed(path).await {
    if claimed_by != current_agent_id {
        return ToolResult::Error {
            message: format!("File '{}' is claimed by {}", path, claimed_by),
            code: Some("FILE_CLAIMED".to_string()),
            retryable: true, // Can retry after other worker completes
        };
    }
}
```

#### `shell` tool (`core/src/agent/tools/shell.rs`)
- Parse command to detect write operations (rm, mv, cp, >, etc.)
- Check if target files are claimed by other workers
- Block or require override flag

#### `worker_shell` tool (`core/src/agent/tools/worker_shell.rs`)
- Same enforcement as shell tool
- Respect escalation policies

### Phase 3: Worker Behavior Updates
- [ ] Update worker prompt to teach proper claim-check workflow:
  1. `check` if resource is available
  2. If available, `claim` it
  3. Do work
  4. `release` when done
- [ ] Consider auto-claim feature (workers auto-claim files they write to)

### Phase 4: Edge Cases
- [ ] **Claim expiration**: Claims should timeout (e.g., 5 minutes) to prevent deadlock on worker crash
- [ ] **Nested claims**: Worker A claims file.txt, Worker B (spawned by A) tries to write to file.txt
  - Should child workers inherit parent's claims?
  - Or should parent release before spawning child?
- [ ] **Directory claims**: If Worker A claims "src/", does that include "src/main.rs"?
- [ ] **Read operations**: Should `read_file` respect claims? (Probably not - reading is safe)

### Phase 5: Testing
- [ ] Unit test: Worker A claims → Worker B tries to write → should fail
- [ ] Unit test: Worker A claims → Worker A writes → should succeed
- [ ] Integration test: Concurrent workers, same file, verify no data loss
- [ ] Timeout test: Claim expires, other worker can claim

## Design Decisions Needed

### 1. Claim Scope
```rust
// Option A: Exact match only
claim_resource("src/main.rs")  // Only blocks writes to "src/main.rs"

// Option B: Directory prefix
claim_resource("src/")  // Blocks all writes under src/
```

### 2. Inheritance
```rust
// Should child workers inherit parent's claims?
Worker A claims "file.txt"
Worker A spawns Worker B
Worker B tries to write "file.txt" → Should this work?
```

### 3. Auto-Release
```rust
// Option A: Explicit release required
worker calls claim → work → release

// Option B: Auto-release on complete
worker calls claim → work → complete → auto-release all claims
```

### 4. Claim vs Lock
```rust
// Option A: Soft claim (advisory, can override)
Tool warns but allows override with force flag

// Option B: Hard claim (enforced)
Tool completely blocks until released
```

## Files to Modify

1. `core/src/agent/tools/commonboard.rs` - Add release action, timeout logic
2. `core/src/agent/tools/fs.rs` - Add claim check to write_file
3. `core/src/agent/tools/shell.rs` - Add claim check for write commands
4. `core/src/agent/tools/worker_shell.rs` - Add claim check
5. `core/src/agent/tools/delegate/prompt.rs` - Update worker instructions
6. `core/src/agent/commonbox.rs` - Add claim expiration, inheritance tracking

## Success Criteria

- [ ] Two workers cannot write the same file simultaneously
- [ ] Worker can release claim when done (no "unknown action: release" error)
- [ ] Claims expire after timeout (no permanent locks)
- [ ] TUI shows claimed files and who claimed them
- [ ] No data loss in concurrent write scenarios

## Current Behavior (for reference)

From debug.log:
```
Worker 1000: claim "list_current_directory" → SUCCESS
Worker 1000: list_files "." → SUCCESS (no check)
Worker 1000: release "list_current_directory" → ERROR (action doesn't exist)
```

Worker claimed a directory for listing files (unnecessary), then couldn't release it.

## Priority

**Medium** - Not blocking basic functionality, but important for multi-worker scenarios.

## Related Issues

- Commonboard "release" action missing (causes errors)
- Token metrics not populated in TUI
- Context window hardcoded (0/8192)
