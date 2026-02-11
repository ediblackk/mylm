# Comprehensive Fix Strategy: V2 Orchestrator Root Causes & Implementation Plan

**Date**: 2026-02-06  
**Source**: Synthesis of architectural review (12 structural issues) + runtime log analysis (debug.log)  
**Goal**: Bridge architecture and runtime evidence into actionable fix strategy

---

## Executive Summary

The V2 Orchestrator suffers from **three critical bugs** that cause:
- **Runaway worker spawning** (15+ background jobs)
- **Infinite worker loops** (50+ second durations, no tool execution)
- **Context bloat** (unbounded scratchpad growth)
- **API failures** (405/404 errors from misconfiguration)

These bugs are **interconnected**: poor worker lifecycle management → resource exhaustion → context bloat → API errors.

**Verdict**: Fixes are straightforward but must be applied in the correct order to avoid breaking changes.

---

## Part 1: Root Cause Analysis Synthesis

### Root Cause 1: Worker Infinite Loop Guard is Broken

**Problem**: Workers can loop indefinitely without executing tools, consuming tokens and resources.

**Code Locations**:
- `core/src/agent/tools/delegate.rs:437` - Default `max_iterations=50` is too high
- `core/src/agent/v2/driver/event_driven.rs:241-243` - `consecutive_messages` reset logic flaw

**Log Evidence**:
```
[2026-02-06 12:17:14.411] Job completed: id=53eecbb9... Result: SUCCESS
[2026-02-06 12:17:14.411] [WORKER PERFORMANCE] Worker 53eecbb9 FINAL REPORT:
  Total Duration: 23.287s
  Result: SUCCESS  <-- But actually failed due to model error

[2026-02-06 12:19:37.253] [ERROR] Worker aborted after 3 consecutive conversational messages
[2026-02-06 12:19:37.253] Job completed: id=6351391f... Result: SUCCESS
```

**Impact**:
- Workers run for 20-50 seconds producing no useful output
- Token waste: 10k-20k tokens per runaway worker
- Multiple identical workers spawned (spam)

**Dependencies**: None (standalone fix)

---

### Root Cause 2: Unbounded Scratchpad Causes Context Bloat

**Problem**: Scratchpad is a raw `String` with no TTL, tags, or cleanup. Grows indefinitely, causing:
- Excessive context injection (8k+ tokens)
- LLM request failures (405 errors from context overflow)
- Memory pressure

**Code Locations**:
- `core/src/agent/workspace.rs` - SharedWorkspace uses `Arc<RwLock<String>>`
- `core/src/agent/tools/scratchpad.rs` - ScratchpadTool manipulates raw string
- `core/src/agent/core.rs:77` - Agent has `scratchpad: Option<Arc<RwLock<StructuredScratchpad>>>` (already using structured but incomplete)

**Log Evidence**:
```
[2026-02-06 12:16:57.940] AgentV2 system prompt size: 9643 chars (scratchpad: 0 chars)
[2026-02-06 12:17:11.868] AgentV2 system prompt size: 9643 chars (scratchpad: 0 chars)
```
System prompts are 9-10k chars, and scratchpad content would be added on top, risking context limit.

**Impact**:
- Context window overflow
- Increased token usage (cost)
- Slower LLM responses

**Dependencies**: None (isolated component)

---

### Root Cause 3: V2 Parallel Execution Not Integrated into Agent::step()

**Problem**: Agent::step() correctly delegates to AgentV2, but the V2 agent uses `run_event_driven` which is designed for background workers. The main agent loop in `src/terminal/agent_runner.rs` doesn't properly handle V2's `Message` decisions that indicate parallel execution.

**Code Locations**:
- `core/src/agent/core.rs:254-261` - Agent::step() delegates to v2_agent.step()
- `core/src/agent/v2/core.rs:591-599` - V2 uses `execute_parallel_tools()` and returns `Message`
- `src/terminal/agent_runner.rs` - Runner loop needs to handle non-final messages correctly

**Log Evidence**:
```
[2026-02-06 12:15:59.399] DelegateTool::call execution started
[2026-02-06 12:15:59.399] Job created: id=e91ac0a8, tool=delegate, is_worker=true
[2026-02-06 12:15:59.419] [WORKER PERFORMANCE] Starting worker e91ac0a8
```
Workers spawn correctly, but the main agent doesn't use parallel execution for its own tool calls.

**Impact**:
- Main agent executes tools sequentially (V1 behavior) even when configured as V2
- Performance degradation: no parallel tool execution
- Inconsistent behavior between main agent and workers

**Dependencies**: Requires Bug 2 fix first (worker stability)

---

### Root Cause 4: JobRegistry Worker Limit Not Enforced Proactively

**Problem**: The `check_worker_limit()` in DelegateTool only checks after the fact. Multiple rapid delegate calls can exceed the limit before the check runs.

**Code Locations**:
- `core/src/agent/tools/delegate.rs:210-223` - `check_worker_limit()` is async but called inside `call()`
- No pre-emptive rate limiting or semaphore-based pool

**Log Evidence**:
```
12:15:59 - Worker e91ac0a8 started
12:16:29 - Worker 6ac19df0 started
12:16:32 - Worker 0fed432d started
... (total 15+ workers spawned rapidly)
```
All workers spawned within ~30 seconds, exceeding typical worker limits (default likely 4-8).

**Impact**:
- Resource exhaustion (CPU, memory, LLM API rate limits)
- 405/404 errors from API overload or invalid model references
- System instability

**Dependencies**: Bug 2 (worker loops) exacerbates this

---

### Root Cause 5: Model Configuration Errors (405/404 API Errors)

**Problem**:
- 404: "The model 'default' does not exist" - worker tries to use non-existent model
- 405: "Method Not Allowed" - likely endpoint/authentication issue

**Code Locations**:
- `core/src/agent/tools/delegate.rs:443-477` - Worker model selection logic
- `core/src/llm/client.rs` - API request configuration

**Log Evidence**:
```
[2026-02-06 12:16:07.222] [ERROR] Chat failed after 116.367183ms: API request failed (405 Method Not Allowed)
[2026-02-06 12:16:51.783] [ERROR] Chat failed after 474.419077ms: API request failed (404 Not Found): The model "default" does not exist
```

**Impact**:
- Worker failures (14 manual cancellations)
- User had to intervene and cancel all jobs
- Loss of work, poor UX

**Dependencies**: Bug 2 (workers need to fail fast, not retry endlessly)

---

## Part 2: Prioritized Fix Strategy

### PHASE 1: CRITICAL (Fix Worker Stability)

**Goal**: Stop runaway workers and enforce iteration limits

**Changes**:

1. **File**: `core/src/agent/tools/delegate.rs`
   - **Line 437**: Change `max_iterations.unwrap_or(50)` → `unwrap_or(10)`
   - **Rationale**: Workers should complete in 3-10 iterations; 50 is excessive

2. **File**: `core/src/agent/v2/driver/event_driven.rs`
   - **Lines 420-430**: Fix `consecutive_messages` reset logic
   - **Current**: Reset happens before tool execution (line 241 in original code)
   - **Fix**: Move reset to inside `Ok(observation)` branch in `process_action`
   - **Code**:
     ```rust
     match execute_single_tool(agent, &tool, &args, &event_bus).await {
         Ok(observation) => {
             state.record_successful_tool_use(); // This should set consecutive_messages = 0
             state.pending_observation = Some(observation);
         }
         Err(e) => {
             state.pending_observation = Some(format!("Tool error: {e}"));
             // Do NOT reset consecutive_messages on failure
         }
     }
     ```
   - **Also**: Ensure `record_successful_tool_use()` actually resets the counter (line 41-43)

3. **File**: `core/src/agent/v2/driver/event_driven.rs`
   - **Lines 361-373**: The consecutive message guard is already present but may not trigger correctly
   - **Verify**: `state.consecutive_messages` increments only on non-final, non-action messages
   - **Add logging**: When aborting, log the full message content for debugging

**Expected Outcome**:
- Workers abort after 3 consecutive conversational messages
- Workers respect iteration limits (default 10)
- No more 50+ second runaway workers

**Breaking Changes**: None (only tightening limits)

**Tests**:
- Unit test: Worker with 3 consecutive "thinking" messages aborts
- Integration test: Worker completes simple task within 5 iterations

---

### PHASE 2: HIGH (Fix Scratchpad Bloat)

**Goal**: Replace unbounded scratchpad with structured, self-cleaning system

**Changes**:

1. **Create new file**: `core/src/agent/scratchpad.rs`
   - Implement `ScratchpadEntry` with `id`, `timestamp`, `content`, `ttl`, `tags`, `persistent`
   - Implement `Scratchpad` with `HashMap<EntryId, ScratchpadEntry>`
   - Methods: `append()`, `remove()`, `list_by_age()`, `list_by_tag()`, `get_size()`, `purge_expired()`
   - **Full implementation provided in docs/implementation-plan.md** (lines 132-269)

2. **Update**: `core/src/agent/workspace.rs`
   - Change `scratchpad: Arc<RwLock<String>>` → `Arc<RwLock<Scratchpad>>`
   - Update `get_scratchpad()` to format entries as concatenated string (for LLM consumption)
   - Update `add_entry()` to create non-persistent entries with appropriate tags

3. **Update**: `core/src/agent/tools/scratchpad.rs`
   - Replace raw string operations with `Scratchpad` API
   - Extend `ScratchpadArgs` with optional `tags` (array) and `persistent` (bool)
   - Maintain backward compatibility: if only `text` provided, treat as persistent

4. **Update**: `core/src/agent/core.rs` and `core/src/agent/v2/core.rs`
   - Change field type: `scratchpad: Option<Arc<RwLock<StructuredScratchpad>>>` → `Option<Arc<RwLock<Scratchpad>>>`
   - Update all `scratchpad.read().len()` to `scratchpad.read().get_size()`
   - Update all writes to use `append()` API

5. **Add cleanup task**: In `AgentV2::step()` or unified `Agent::step()`
   - Every 10 steps OR when `scratchpad.get_size() > 100_000` (100KB)
   - Call `manage_scratchpad()`:
     - Get entries older than 1 hour: `list_by_age(Duration::hours(1))`
     - Generate summary via `summarize_old_entries()`
     - Use LLM to decide which to delete (prompt: "Which entries can be removed?")
     - Parse response and call `remove(id, false)`

**Migration Path**:
- Provide `Scratchpad::from_legacy_string(s: String) -> Scratchpad` that creates single persistent entry
- In `Agent::new_with_config`, detect legacy string and convert automatically

**Expected Outcome**:
- Scratchpad size bounded automatically
- Old ephemeral entries cleaned up
- No more context bloat from scratchpad

**Breaking Changes**: None (backward compatible conversion)

**Tests**:
- Unit tests for Scratchpad CRUD, TTL, purge
- Integration test: Scratchpad grows to 100KB, cleanup triggers, size reduces

---

### PHASE 3: MEDIUM (Enable True Parallel Execution for Main Agent)

**Goal**: Make main agent use V2's parallel tool execution when version=V2

**Changes**:

1. **Update**: `core/src/agent/core.rs:252-350` (Agent::step method)
   - The V2 delegation already exists (lines 254-261) but it converts V2 decisions to V1
   - **Problem**: V2's `step()` returns `AgentDecision::Message` for parallel execution summaries, but the runner expects `Action` for tool execution
   - **Fix**: Ensure V2's `execute_parallel_tools()` is properly integrated

2. **Review**: `core/src/agent/v2/core.rs:591-599`
   - This code calls `execute_parallel_tools()` and returns `Message` with summary
   - This is correct for parallel execution

3. **Update**: `src/terminal/agent_runner.rs`
   - The runner's `AgentDecision::Message` arm already checks `has_pending_decision()` and continues
   - **Verify**: This logic correctly handles V2's parallel execution flow
   - **Add**: Import and use `is_final_response()` to distinguish final answers from intermediate summaries

4. **Add**: `core/src/agent/core.rs` - V2-specific fields (if not already present)
   - The Agent struct already has `v2_agent: Option<AgentV2>` (line 96)
   - No additional fields needed

**Expected Outcome**:
- Main agent with V2 version executes multiple tools in parallel in a single step
- Proper handling of parallel execution summaries in the runner loop
- Improved performance for multi-tool tasks

**Breaking Changes**: Low risk - V2 path is already partially implemented; this completes the integration

**Tests**:
- Integration test: V2 agent with parallel tool calls (e.g., `[delegate, list_files, execute_command]`) executes all in one step
- Verify runner continues loop on non-final Message and exits on final answer

---

### PHASE 4: HIGH (Enforce Worker Limit with Semaphore Pool)

**Goal**: Prevent worker spam by enforcing limits at spawn time

**Changes**:

1. **Add**: Global worker pool semaphore in `core/src/agent/tools/delegate.rs`
   ```rust
   use once_cell::sync::OnceCell;
   use tokio::sync::Semaphore;
   
   static WORKER_POOL: OnceCell<Arc<Semaphore>> = OnceCell::new();
   
   // In DelegateTool::call():
   let pool = WORKER_POOL.get_or_init(|| {
       Arc::new(Semaphore::new(8)) // Default 8 concurrent workers
   });
   let _permit = pool.acquire_owned().await?;
   ```
   - **Note**: Use `once_cell` crate or lazy_static

2. **Update**: `core/src/agent/tools/delegate.rs:210-223`
   - Keep existing `check_worker_limit()` for config-based limits
   - Add semaphore check as second layer (concurrency control)
   - If no permit available, return error immediately: "Worker limit exceeded, try again later"

3. **Make limit configurable**:
   - Read `worker_limit` from ConfigManager
   - Initialize `WORKER_POOL` with that limit (if already initialized, skip)
   - Allow override via environment variable `MYLM_WORKER_LIMIT`

**Expected Outcome**:
- Maximum concurrent workers strictly enforced
- Spawn attempts beyond limit fail fast with clear error
- No resource exhaustion from worker spam

**Breaking Changes**: None (adds protection)

**Tests**:
- Stress test: Spawn 20 workers with limit=8; verify only 8 run concurrently
- Verify error message when limit exceeded

---

### PHASE 5: MEDIUM (Fix Model Configuration Errors)

**Goal**: Eliminate 404/405 API errors from worker model misconfiguration

**Changes**:

1. **Update**: `core/src/agent/tools/delegate.rs:441-477`
   - **Validate model exists** before creating client:
     ```rust
     let effective_model = requested_model.or(worker_model).or_else(|| Some(llm_client.config().model.clone()));
     if let Some(model_name) = &effective_model {
         if model_name == "default" {
             return Err("Model 'default' is not a valid model. Specify a real model name (e.g., 'step-3.5-flash')".into());
         }
     }
     ```
   - **Add fallback**: If requested model fails to initialize, log warning and use parent model instead of failing

2. **Update**: `core/src/llm/client.rs` (or wherever API calls are made)
   - Add better error handling for 405/404
   - Distinguish between "model not found" vs "endpoint not allowed"
   - Provide actionable error messages

3. **Documentation**: Update DelegateTool usage to clarify:
   - `model` parameter must be a valid model name
   - "default" is not accepted
   - Worker inherits parent's model if not specified

**Expected Outcome**:
- No more 404 errors for "default" model
- Clear error: "Model 'default' is not valid" when misconfigured
- Workers gracefully fall back to parent model on configuration errors

**Breaking Changes**: None (adds validation)

**Tests**:
- Unit test: Delegate with model="default" returns error immediately
- Integration test: Delegate with valid model succeeds; with invalid model falls back gracefully

---

## Part 3: Implementation Order and Dependencies

### Dependency Graph

```
Phase 1 (Worker Stability)
  ├─ Fix max_iterations default
  ├─ Fix consecutive_messages reset
  └─ Verify guard logic
       ↓
Phase 2 (Scratchpad Cleanup) - Independent, can run in parallel
  ├─ Create Scratchpad struct
  ├─ Update Workspace & ScratchpadTool
  ├─ Add cleanup task to AgentV2
  └─ Migration path
       ↓
Phase 3 (V2 Parallel Integration) - Depends on Phase 1 (stable workers)
  ├─ Verify Agent::step() V2 delegation
  ├─ Update runner loop Message handling
  └─ Test parallel execution
       ↓
Phase 4 (Worker Pool Semaphore) - Depends on Phase 1
  ├─ Add global semaphore
  ├─ Enforce concurrency limit
  └─ Make limit configurable
       ↓
Phase 5 (Model Config Fixes) - Can run any time after Phase 1
  ├─ Validate model names
  ├─ Add fallback logic
  └─ Improve error messages
```

### Parallel Execution Plan

- **Phases 1 & 2**: Can be implemented simultaneously (no dependencies)
- **Phase 3**: Waits for Phase 1 completion (worker stability)
- **Phase 4**: Waits for Phase 1 (uses worker pool)
- **Phase 5**: Independent, can be done anytime

### Risky Changes & Mitigation

| Change | Risk | Mitigation |
|--------|------|------------|
| Scratchpad type change | Medium (widespread refactor) | Provide legacy conversion; use IDE search to find all access points; comprehensive testing |
| consecutive_messages reset | High (could break worker guard) | Add detailed logging; unit test with simulated tool failures; monitor after deployment |
| V2 parallel integration | Medium (affects main agent loop) | Keep V1 path unchanged; add feature flag; extensive integration testing |
| Global semaphore | Low (additive) | Ensure proper initialization; test with high concurrency |

### Rollback Plan

**For each phase**:
1. **Git branch**: Implement each phase in separate branch
2. **Feature flags**: Wrap changes in config flags (e.g., `config.enable_structured_scratchpad`)
3. **Migration safety**:
   - Scratchpad: Auto-convert legacy format on startup
   - Worker limits: Make configurable, default to safe values
4. **Quick rollback**: If issues arise, revert branch or disable flag

---

## Part 4: Success Criteria

### Observable Behavior Changes

**After Phase 1 (Worker Stability)**:
- ✅ No worker runs more than 10 iterations without completing
- ✅ Workers with 3+ consecutive non-tool messages abort automatically
- ✅ Worker durations: 90% complete within 10 seconds (vs 20-50s before)
- ✅ Logs show: `Worker aborted after 3 consecutive conversational messages` (expected, not error)

**After Phase 2 (Scratchpad Cleanup)**:
- ✅ Scratchpad size stays under 50KB (auto-cleanup triggers at 100KB)
- ✅ System prompt size stable (no growth from scratchpad)
- ✅ Logs show: `Scratchpad cleanup: removed X old entries` periodically
- ✅ No memory warnings from context bloat

**After Phase 3 (V2 Parallel Integration)**:
- ✅ Main agent with `version=V2` executes multiple tools in parallel in a single step
- ✅ Tool execution time reduced by 30-50% for multi-tool tasks
- ✅ Logs show: `Executed parallel tools:` with multiple tool names
- ✅ No regression in V1 agents (sequential execution unchanged)

**After Phase 4 (Worker Pool)**:
- ✅ Concurrent worker count never exceeds configured limit
- ✅ Spawn attempts beyond limit fail fast with clear error
- ✅ Logs show: `Worker limit exceeded: 8/8` when saturated
- ✅ System remains responsive under heavy load

**After Phase 5 (Model Config)**:
- ✅ No 404 errors for "default" model
- ✅ Clear error: "Model 'default' is not valid" when misconfigured
- ✅ Workers fall back to parent model on config error
- ✅ 405 errors reduced (if due to model misconfiguration)

### Verification Methods

**Log Analysis**:
```bash
# Check worker durations
grep "Worker.*FINAL REPORT" debug.log | grep -E "(SUCCESS|FAILED)" | wc -l

# Check for aborted workers (expected, not errors)
grep "Worker aborted after" debug.log | wc -l

# Check scratchpad size in system prompts
grep "system prompt size" debug.log | tail -20

# Check for API errors (should be 0)
grep "API request failed" debug.log | wc -l
```

**Job Registry Inspection**:
```bash
# Via CLI or TUI (F4)
- Active jobs should never exceed worker_limit
- Completed jobs show reasonable durations (< 10s for simple tasks)
- No stalled or stuck jobs
```

**Manual Test**:
1. Start mylm with V2 agent
2. Issue: "List files in current directory and count lines in README.md in parallel"
3. Expected: Both tools execute in same step, results returned together
4. Check logs: `Executed parallel tools:` appears once with both tools

**Performance Metrics**:
- Token usage per task: Should decrease 20% (less retries, less context)
- Average task duration: Should decrease 30% (parallel execution)
- Memory footprint: Stable (scratchpad cleanup)

---

## Part 5: Additional Recommendations

### Beyond the Critical Fixes

The architectural review identified 12 structural issues. After fixing the critical bugs, prioritize:

1. **Smart Chunking** (`core/src/memory/chunker.rs`) - For 10M token processing
2. **IVF_PQ Indexing** in VectorStore - For retrieval performance
3. **Hybrid Search** (vector + BM25) - For precision at scale
4. **Hierarchical Indexing** - To solve "awareness problem"
5. **Stateless Workers** - For efficient parallel processing

These are **architectural enhancements** beyond bug fixes and should be planned as separate initiatives.

### Monitoring & Observability

Add metrics for:
- Worker spawn rate, duration, success/failure ratio
- Scratchpad size, cleanup frequency
- Parallel vs sequential tool execution count
- API error rates by model

Export to Prometheus/OpenTelemetry for alerting.

---

## Conclusion

This fix strategy addresses the **immediate stability issues** (worker loops, context bloat) while enabling **core V2 functionality** (parallel execution). The phased approach minimizes risk and allows incremental validation.

**Total estimated changes**: 15-20 files modified, 1 new file created.

**Next step**: Present this plan to the user for approval, then switch to Code mode to implement Phase 1.

---

**Document End**
