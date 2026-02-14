# Capability Implementations

**Purpose**: Concrete implementations of runtime capability traits.

## Available Implementations

### Tools
| File | Implementation | Description |
|------|----------------|-------------|
| `tool_registry.rs` | `ToolRegistry` | Dynamic tool registry with 8 built-in tools |
| `simple_tool.rs` | `SimpleToolExecutor` | Basic shell/file tools |

### LLM
| File | Implementation | Description |
|------|----------------|-------------|
| `llm_client.rs` | `LlmClientCapability` | Bridge to existing LlmClient |

### Approval
| File | Implementation | Description |
|------|----------------|-------------|
| `terminal_approval.rs` | `TerminalApprovalCapability` | Interactive terminal prompts |
| `terminal_approval.rs` | `AutoApproveCapability` | Auto-approve all (for testing) |

### Workers
| File | Implementation | Description |
|------|----------------|-------------|
| `local_worker.rs` | `LocalWorkerCapability` | Spawns tokio tasks |

### Telemetry
| File | Implementation | Description |
|------|----------------|-------------|
| `console_telemetry.rs` | `ConsoleTelemetry` | Logs to console/file |

### Web Search
| File | Implementation | Description |
|------|----------------|-------------|
| `web_search.rs` | `WebSearchCapability` | Kimi/SerpAPI/Brave search |
| `web_search.rs` | `StubWebSearch` | Stub for testing |

### Memory
| File | Implementation | Description |
|------|----------------|-------------|
| `memory.rs` | `MemoryCapability` | Long-term memory storage |

### Wrappers
| File | Implementation | Description |
|------|----------------|-------------|
| `retry.rs` | Retry wrappers | Add retry logic to any capability |
| `local.rs` | Local implementations | Placeholder for local-only capabilities |

## Built-in Tools (ToolRegistry)

| Tool | Description | Example |
|------|-------------|---------|
| `shell` | Execute shell commands | `shell ls -la` |
| `read_file` / `cat` | Read file contents | `read_file path/to/file` |
| `write_file` | Write to file | `write_file path "content"` |
| `list_dir` / `ls` | List directory | `ls ./` |
| `search` | Search files | `search pattern ./dir` |
| `pwd` | Print working directory | `pwd` |

## Adding a New Implementation

### Step 1: Create the file

Create `impls/my_capability.rs`:

```rust
//! My capability description

use crate::agent_v3::runtime::{
    capability::{Capability, MyCapabilityTrait},
    context::RuntimeContext,
    error::MyError,
};

/// My implementation
pub struct MyCapability;

impl MyCapability {
    pub fn new() -> Self { Self }
}

impl Capability for MyCapability {
    fn name(&self) -> &'static str { "my-capability" }
}

#[async_trait::async_trait]
impl MyCapabilityTrait for MyCapability {
    async fn my_method(&self, ctx: &RuntimeContext) -> Result<String, MyError> {
        // Implementation
        Ok("result".to_string())
    }
}
```

### Step 2: Export in mod.rs

Add to `impls/mod.rs`:

```rust
pub mod my_capability;
pub use my_capability::MyCapability;
```

### Step 3: Add to CapabilityGraph (if needed)

Update `graph.rs` to include your capability.

### Step 4: Add stub implementation

For testing, add a stub version in `graph.rs`:

```rust
pub struct StubMyCapability;
impl Capability for StubMyCapability { ... }
#[async_trait]
impl MyCapabilityTrait for StubMyCapability { ... }
```

## Testing Your Implementation

```rust
#[tokio::test]
async fn test_my_capability() {
    let cap = MyCapability::new();
    let ctx = RuntimeContext::new();
    
    let result = cap.my_method(&ctx).await;
    assert!(result.is_ok());
}
```
