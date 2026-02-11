# Investigation & Fix Plan: Worker Stall + Context Bloat Issues

## Issues Identified from debug.log

### Issue 1: Workers Show "Completed" Instead of "Stalled" in UI
**Evidence:**
- Line 569: `Job stalled: id=9a999fea...` - Log says "Job stalled"
- Line 570-571: But then logs `Worker [config_audit] completed` and `Job completed`
- Line 596, 613, 641, 664: Same pattern for other workers

**Root Cause:** The worker's main loop continues to run after `stall_job()` is called. The stall status is set, but the worker doesn't actually pause/wait - it just continues and eventually returns, which marks it as "completed".

### Issue 2: Main Agent Context Bloat (5.6M tokens!)
**Evidence:**
- Line 672: `estimated_tokens=5646848` (5.6 million tokens!)
- Lines 673-694: Repeated 400 errors "total message size 16940499 exceeds limit 4194304"

**Root Cause:** 
1. Worker results are being added to main agent context without summarization/truncation
2. No pre-flight token check before sending to provider
3. Context management (condensation) isn't working or isn't aggressive enough

### Issue 3: Workers Don't Request Extension (Expected Behavior?)
**Current Behavior:** Workers hit 16 actions → Get marked as STALLED → But then complete anyway
**Expected Behavior:** Workers should pause and wait for main agent approval to continue

---

## Step-by-Step Fix Plan (Tracing Bullets Methodology)

### Phase 1: Context Bloat Prevention (DONE ✅)

#### Step 1.1: Add Pre-Flight Token Check with Context Logging ✅
**File:** `core/src/llm/client.rs` - `chat()` method
**Change:** Added check at 80% of max_context_tokens threshold

**Behavior:**
- When context exceeds 80% of max: Logs full context to `/tmp/mylm_context_bloat_{agent_type}_{timestamp}.txt`
- When context exceeds 100% of max: Returns error immediately (no API call)

**Debug log includes:**
- Agent type, job info, model
- Max context, threshold, estimated tokens
- Full message breakdown with role and content preview

This allows you to see exactly what's bloating the context.

---

### Phase 2: Fix Worker Stall Detection (DONE ✅)

#### Step 2.1: Fixed Action Count Check Timing ✅
**File:** `core/src/agent/v2/driver/event_driven.rs` - `process_action()`
**Problem:** Action count was incremented BEFORE stall check, allowing 16 actions instead of 15.

**Fix:** Moved stall check BEFORE action count increment:
```rust
// Check BEFORE incrementing
if state.action_count >= agent.max_actions_before_stall {
    // stall and return
}
// Then increment
state.action_count += 1;
```

#### Step 2.2: Fixed Worker Completion vs Stall Status ✅
**File:** `core/src/agent/tools/delegate.rs` - worker result handler
**Problem:** Worker always called `complete_job()` even when stalled.

**Fix:** Check job status in registry after worker returns:
```rust
let is_stalled = job.as_ref().map(|j| j.status == JobStatus::Stalled).unwrap_or(false);
if is_stalled {
    // Don't call complete_job - keep stalled status
} else {
    job_registry.complete_job(...);
}
```

#### Step 2.3: Verify UI Shows Correct Status
**Check:** After testing, verify F4 panel shows "Stalled" not "Completed"

---

### Phase 3: Fix Context Bloat (Critical)

#### Step 3.1: Add Worker Result Summarization
**Problem:** Worker results (which can be huge) are being added to main agent context verbatim.

**Solution:** 
- Summarize worker results before adding to main context
- Or truncate to a reasonable size (e.g., first 4000 chars)
- Or use a separate "worker results" scratchpad that doesn't go into main context

#### Step 3.2: Fix Context Condensation/Summarization
**File:** `core/src/agent/v2/memory.rs` or context management
**Question:** Why isn't condensation triggering at 80% threshold?

**Investigation Points:**
- Check if `condensation_threshold` is being read from config
- Check if condensation is actually being triggered
- Check if condensed context is being used properly

---

### Phase 4: Fix Worker Action Extension (Optional/Enhancement)

#### Step 3.1: Design Extension Mechanism
**Problem:** Workers should be able to request more actions from main agent.

**Design:**
1. Worker hits action limit
2. Worker returns partial results + "needs more actions" flag
3. Main agent decides: grant more actions OR use partial results
4. If granted, worker resumes with increased budget

#### Step 3.2: Implement Extension Request
**File:** `core/src/agent/tools/delegate.rs`
**Action:** Modify worker spawn to handle extension requests

---

## Implementation Order (Priority)

1. **✅ DONE:** Step 1.1 - Pre-flight token check with context logging
2. **✅ DONE:** Step 2.1 - Action count check timing fix
3. **✅ DONE:** Step 2.2 - Worker stall status handling
4. **HIGH (Prevent bloat):** Step 3.1 - Worker result summarization
5. **MEDIUM:** Step 3.2 - Context condensation fix
6. **LOW:** Step 4.x - Extension mechanism

---

## Current Observations

### Token Metrics Are Wrong
Line 569 shows: `tokens=1208805909/30799748/1239605657`
- These numbers are impossibly high (1.2 billion tokens?)
- Token counting is clearly broken
- This likely breaks context management that relies on token counts

### Workers Are Looping Forever
Looking at worker `ab613a1d` (security_audit):
- Line 507: iteration=10
- Line 512: iteration=10 (checking limits)
- Line 516: iteration=10 (checking limits)
- Line 528: Chat request
- Line 560: Chat completed
- Line 605: Chat request
- Line 617: Chat completed
- Line 653: Chat request
- Line 662: Chat completed
- Line 664: FINALLY stalled at action_count=16

The worker did 16+ actions but the limit was supposed to be 15 (`max_actions_before_stall: 15`).

**BUG:** The action count check is happening AFTER the action is taken, not before.
