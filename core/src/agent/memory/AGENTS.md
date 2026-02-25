# Memory Module

**Purpose:** Agent memory integration with the core memory store.

Integrates `crate::memory` (core storage) with the agent system.

## Files

| File | Purpose | Key Items |
|------|---------|-----------|
| `mod.rs` | Module exports | `AgentMemoryManager`, `MemoryProvider` trait |
| `manager.rs` | Memory manager | `AgentMemoryManager` - main interface |
| `context.rs` | Context building | `MemoryContextBuilder`, injection strategies |
| `extraction.rs` | Memory extraction | `MemoryExtractor`, `extract_memories()` |

## Memory Types

- **Hot memory**: Recent activity from journal
- **Cold memory**: Semantic search via vector store
- **Context injection**: Automatic memory inclusion in prompts

## Usage

```rust
use mylm_core::agent::memory::{AgentMemoryManager, MemoryMode};

let manager = AgentMemoryManager::new(config).await?;
manager.add_user_note("User prefers dark mode").await?;
let results = manager.search_memories("dark mode", 5).await?;
```

## MemoryProvider Trait

```rust
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    async fn get_context(&self, user_message: &str) -> String;
    fn remember(&self, content: &str);
    async fn build_context(&self, history: &[Message], scratchpad: &str, system_prompt: &str) -> String;
}
```

## Dependencies

- Uses `crate::memory` (core storage)
- Uses `crate::agent::types`
- Used BY `runtime::capabilities::memory`
