# Smart Pruning Design: Preventing the "Disappearing Message" Problem

## Problem Statement
When context is pruned silently:
- User: "I told you to use Rust, why are you using Python?"
- Model: "I don't recall you mentioning Rust..."
- User: *doubt intensifies* "I definitely said that!"

This creates a gaslighting effect where users doubt their own memory.

## Solution Overview

### 1. Smart Preservation (Before Pruning)
```rust
pub struct PruningStrategy {
    /// Always preserve messages with these markers
    preserve_patterns: Vec<String>, // ["remember", "important", "critical"]
    
    /// Extract and save to memory before pruning
    auto_extract_memories: bool,
    
    /// Keep first N and last M messages (conversation boundaries)
    keep_first: usize,  // Keep initial context/setup
    keep_last: usize,   // Keep recent conversation
}
```

**Smart Detection:**
- Messages containing `{"r": "..."}` or `remember` → Extract to memory before pruning
- Tool calls with side effects (write_file, shell) → Keep or summarize
- User corrections ("No, I said...") → High priority keep

### 2. Visual Pruning Indicator (UI)
```rust
pub struct PrunedSegment {
    /// When it was pruned
    pub timestamp: DateTime<Utc>,
    
    /// How many messages were pruned
    pub message_count: usize,
    
    /// Summary of what was pruned
    pub summary: String,
    
    /// Token savings
    pub tokens_saved: usize,
    
    /// Full content (can be expanded)
    pub full_content: Vec<Message>,
    
    /// Whether user has viewed this
    pub acknowledged: bool,
}
```

**UI Display:**
```
[User] Let's build a web server
[Assistant] I'll help you build a web server...
[User] Use Rust with Axum
[Assistant] Great choice! Let me set up...

💾 [Context Compressed] 12 messages summarized (saving 4,200 tokens)
   "Set up Axum project, configured routes, added error handling,
    discussed middleware options..."
   [View Full] [Restore] [Dismiss]

[User] Now add authentication
[Assistant] I'll add auth to the Axum server...
```

### 3. Recovery Mechanism
```rust
pub struct PrunedHistory {
    /// Archive of pruned segments (FIFO, max N segments)
    segments: VecDeque<PrunedSegment>,
    
    /// Max segments to keep in archive
    max_segments: usize,
}

impl PrunedHistory {
    /// Restore a pruned segment back to active context
    pub fn restore_segment(&mut self, segment_id: usize) -> Vec<Message>;
    
    /// View pruned content without restoring
    pub fn view_segment(&self, segment_id: usize) -> &PrunedSegment;
    
    /// Search pruned history
    pub fn search(&self, query: &str) -> Vec<&PrunedSegment>;
}
```

**User Commands:**
- `/pruned` - Show all pruned segments
- `/restore <id>` - Restore a specific segment
- `/search "Rust"` - Search pruned content
- `/expand` - Expand the last pruned segment in place

### 4. Overview Before Pruning
```rust
pub struct PruningPreview {
    /// What's going to be pruned
    pub to_prune: Vec<Message>,
    
    /// What's going to be kept
    pub to_keep: Vec<Message>,
    
    /// Summary of pruned section
    pub summary: String,
    
    /// Memories extracted before pruning
    pub extracted_memories: Vec<String>,
}
```

**UI Flow:**
```
⚠️ Context approaching limit (92%)

┌─ Messages to be preserved ───────────────────┐
│ [System] You are MYLM...                      │
│ [User] Let's build a web server              │
│ [Assistant] I'll help...                     │
│ [User] Use Rust with Axum                    │ → Kept (recent)
└───────────────────────────────────────────────┘

┌─ Messages to be compressed ──────────────────┐
│ 12 messages (4,200 tokens)                   │
│ • Set up project structure                   │
│ • Configured routes                          │
│ • Added error handling                       │
│ • Discussed middleware                       │
└───────────────────────────────────────────────┘

💾 Auto-saved memories:
   • "User wants Axum, not Actix"
   • "Project uses SQLite for database"

[Compress Now] [Wait] [View All Messages]
```

## Implementation Plan

### Phase 1: Smart Preservation
1. Detect important messages (remember markers, tool side effects)
2. Extract to memory before pruning
3. Keep conversation boundaries

### Phase 2: Visual Indicator
1. Add `PrunedSegment` to chat history
2. Collapsible UI component
3. Show summary + expand option

### Phase 3: Recovery
1. Archive pruned segments
2. `/pruned` command
3. Click to restore

### Phase 4: Proactive Preview
1. Show warning at 80%
2. Preview what will be pruned
3. Let user intervene

## Edge Cases Handled

1. **Rapid pruning**: Batch multiple prunes into one segment
2. **Nested pruning**: Don't prune the pruning indicator itself
3. **Memory overflow**: Archive old pruned segments to disk
4. **Restore conflicts**: Handle restore when context still full
5. **Multi-turn references**: Keep messages referenced by recent messages

## Key UX Principles

1. **Visibility**: User always knows when pruning happens
2. **Agency**: User can view, restore, or override pruning
3. **Trust**: Important info is preserved (memory) or visible (summary)
4. **Recovery**: Nothing is truly lost, just archived

Would you like me to implement this? I can start with:
1. Smart preservation (extract memories before pruning)
2. Pruning indicator in chat history
3. Basic archive/recovery
