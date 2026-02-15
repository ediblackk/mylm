# Auto-Restore on Reference Feature

## Overview
When a user references content that was previously pruned, the system automatically restores that context and notifies the user with a "Remembering..." message.

## How It Works

### 1. User Input Processing
When the user submits a message:
```rust
// In submit_message() - before processing
let auto_restore_result = self.context_manager.check_auto_restore(&input);
```

### 2. Keyword Extraction
The system extracts meaningful keywords from the user message:
- Filters out stop words (the, a, is, etc.)
- Keeps words 3+ characters
- Removes duplicates

### 3. Matching Against Pruned History
For each pruned segment, check if it contains the keywords:
```rust
// If segment matches 2+ keywords or 50% of keywords
if segment_matches >= threshold || match_ratio >= 0.5 {
    matching_segments.push(segment.clone());
}
```

### 4. Auto-Restore
If matching segments found:
```
[User] "What about that Rust web server we discussed?"
      ↓
[System] "Remembering... (restoring 1 context segment)"
      ↓
[Restored messages added to chat history]
      ↓
[Model now sees the restored context]
```

## Example Flow

### Before Pruning:
```
[User] Let's build a web server
[Assistant] What language/framework?
[User] Use Rust with Axum
[Assistant] Great choice! Here's the setup...
[User] Add SQLite for database
[Assistant] Here's the SQLite integration...
```

### After Pruning (context compressed):
```
[User] Let's build a web server
[Assistant] What language/framework?
💾 Context compressed: 3 messages summarized...
```

### Later - User References Pruned Content:
```
[User] What about that Rust web server we discussed?
      ↓
[System] Remembering... (restoring 1 context segment)
      ↓
[Restored] Use Rust with Axum
[Restored] Add SQLite for database
      ↓
[Assistant] Ah yes! The Axum web server with SQLite. Let me continue...
```

## Implementation Details

### Core Functions (`core/src/context/pruning.rs`)

```rust
/// Check if user message references pruned content
pub fn check_auto_restore(user_message: &str, history: &PrunedHistory) -> AutoRestoreResult;

/// Extract keywords from user message
fn extract_keywords(text: &str) -> Vec<String>;
```

### Integration (`src/tui/app/app.rs`)

```rust
// In submit_message()
let auto_restore_result = self.context_manager.check_auto_restore(&input);
if auto_restore_result.found {
    // Show "Remembering..." message
    self.chat_history.push(ChatMessage::assistant(
        format!("Remembering... (restoring {} context segment{})", ...)
    ));
    
    // Restore segments to chat history
    for segment in auto_restore_result.segments {
        for msg in segment.messages {
            self.chat_history.push(convert_to_chat_message(msg));
        }
    }
}
```

## Benefits

1. **Transparent** - User knows context was restored
2. **Automatic** - No manual `/restore` command needed
3. **Contextual** - Only restores when referenced
4. **Non-intrusive** - Just a brief "Remembering..." message
5. **Maintains Flow** - Conversation continues naturally

## Edge Cases Handled

1. **Multiple segments match** - Restores all matching segments
2. **No matches** - Proceeds normally without notification
3. **Partial matches** - Uses threshold (2 keywords or 50% match)
4. **Keyword extraction fails** - Returns empty result, no false positives

## Future Enhancements

1. **Smarter matching** - Use embeddings for semantic similarity
2. **Selective restore** - Only restore relevant parts of segments
3. **Summarize on restore** - Show condensed version first
4. **User confirmation** - "Should I restore the context about X?"
5. **Persistent pruning** - Save pruned segments to disk for recovery
