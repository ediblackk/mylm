# Tools Module

**Purpose:** Tool implementations for the agent runtime.

All tools implement the `ToolCapability` trait from `runtime::core`.

## Files

| File | Purpose | Tool |
|------|---------|------|
| `mod.rs` | Tool registry | `ToolRegistry` - aggregates all tools |
| `shell.rs` | Shell execution | `ShellTool` |
| `worker_shell.rs` | Worker shell | `WorkerShellTool` - restricted shell for workers |
| `read_file/mod.rs` | File reading | `ReadFileTool` - with chunking support |
| `read_file/chunker.rs` | Chunk management | `ChunkPool` for large files |
| `read_file/search.rs` | Chunk search | Search within chunks |
| `read_file/types.rs` | Read types | Type definitions |
| `read_file/pdf.rs` | PDF reading | PDF text extraction |
| `read_file/pool.rs` | Pool management | Worker pool for chunking |
| `write_file.rs` | File writing | `WriteFileTool` |
| `list_files.rs` | Directory listing | `ListFilesTool` |
| `fs.rs` | Filesystem utils | Helper functions |
| `git.rs` | Git operations | `GitStatusTool`, `GitLogTool`, `GitDiffTool` |
| `web_search.rs` | Web search | `WebSearchTool`, `WebSearchConfig` |
| `search_files.rs` | File search | `SearchFilesTool` - full-text search |
| `memory.rs` | Memory tool | `MemoryTool` - store/retrieve memories |
| `delegate/mod.rs` | Worker spawning | `DelegateTool` - spawn sub-agents |
| `delegate/creator.rs` | Worker creation | Worker instantiation |
| `delegate/filter.rs` | Worker filtering | Worker selection |
| `delegate/permissions.rs` | Permissions | Worker capability restrictions |
| `delegate/prompt.rs` | Worker prompts | Prompt generation for workers |
| `delegate/runner.rs` | Worker runner | Worker execution |
| `delegate/types.rs` | Worker types | Type definitions |
| `scratchpad.rs` | Scratchpad | `ScratchpadTool` - agent notes |
| `commonboard.rs` | Coordination | `CommonboardTool` - inter-agent coordination |
| `restricted.rs` | Restrictions | Tool restriction helpers |

## Tool Registry

`ToolRegistry` aggregates all tools and implements `ToolCapability`:

```rust
let registry = ToolRegistry::new()
    .with_memory(vector_store)
    .with_delegate(delegate_tool)
    .with_scratchpad(scratchpad);
```

## Dependencies

- Uses `crate::agent::runtime::core` (traits, errors)
- Uses `crate::agent::types` (primitives)
- Used BY `runtime::capabilities`
