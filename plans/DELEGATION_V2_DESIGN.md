# Delegation V2 Design: Differentiated Workers with Shared Coordination

## Problem Statement

Current `delegate` tool limitations:
1. **Same objective for all workers**: When spawning N workers, they all get identical tasks
2. **No per-worker customization**: Cannot restrict tools/commands per worker
3. **No coordination mechanism**: Workers are isolated, cannot share intermediate results
4. **Limited visibility**: Main agent doesn't have visibility into worker progress

## Solution Overview

### 1. Enhanced API: `workers` vs `worker_configs`

```json
// Simple mode (backward compatible) - same task, parallel execution
{
  "a": "delegate",
  "i": {
    "objective": "Run cargo check on all packages",
    "workers": 3
  }
}

// Advanced mode - differentiated workers with coordination
{
  "a": "delegate",
  "i": {
    "coordination_mode": "scratchpad",
    "shared_context": "Refactoring auth module - coordinate to avoid conflicts",
    "worker_configs": [
      {
        "id": "models",
        "objective": "Update User and Session models to use new auth types",
        "tools": ["read_file", "write_file", "grep"],
        "allowed_commands": ["cargo check --lib"],
        "scratchpad_tags": ["models", "progress"]
      },
      {
        "id": "handlers",
        "objective": "Update login/logout handlers to use new auth middleware",
        "tools": ["read_file", "write_file", "grep"],
        "allowed_commands": ["cargo check --lib"],
        "scratchpad_tags": ["handlers", "progress"]
      },
      {
        "id": "tests",
        "objective": "Update auth tests to match new implementation",
        "tools": ["read_file", "write_file", "execute_command"],
        "allowed_commands": ["cargo test", "cargo check *"],
        "scratchpad_tags": ["tests", "progress"],
        "depends_on": ["models", "handlers"]
      }
    ]
  }
}
```

### 2. Shared Scratchpad Coordination

All workers and main agent share a **coordination scratchpad** with structured entries:

```rust
// Coordination entry types
enum CoordinationEntry {
    // Worker announces what it's working on
    Claim {
        worker_id: String,
        file_or_module: String,
        estimated_completion: Option<DateTime>,
    },
    // Worker reports progress
    Progress {
        worker_id: String,
        completed_steps: Vec<String>,
        current_step: String,
        blocked_on: Option<String>,
    },
    // Worker publishes intermediate result
    Result {
        worker_id: String,
        output_files: Vec<String>,
        summary: String,
        status: "success" | "partial" | "failed",
    },
    // Worker signals completion
    Complete {
        worker_id: String,
        final_result: String,
        files_modified: Vec<String>,
    },
    // Dependency notification
    DependencyMet {
        worker_id: String,
        dependency_id: String,
        summary: String,
    },
}
```

### 3. Worker Scratchpad Integration

Workers get a **read-write view** of the shared scratchpad:

```rust
// In worker's system prompt (injected automatically)
const COORDINATION_INSTRUCTIONS: &str = r#"
## Coordination Protocol

You share a scratchpad with the main agent and other workers.

### Writing to Scratchpad (REQUIRED)

After each significant action, update the scratchpad:

```json
{"a": "scratchpad", "i": {"action": "append", "text": "CLAIM: Working on src/models/user.rs - updating Auth methods", "tags": ["models", "claim"], "persistent": true}}
```

Entry types to use:
- `CLAIM: <what you're working on>` - Before starting work on a file/module
- `PROGRESS: <what you just completed>` - After completing a subtask
- `RESULT: <key findings or outputs>` - Intermediate results others might need
- `BLOCKED: <why you're stuck>` - If waiting for another worker or need help
- `COMPLETE: <final summary>` - When your task is done

### Reading from Scratchpad

Check scratchpad before starting work:
```json
{"a": "scratchpad", "i": {"action": "list"}}
```

Respect other workers' claims. If another worker claimed a file, work on something else.

### Coordination Tags

Your assigned tags: {worker_tags}
Watch for these tags from other workers: {watch_tags}
"#;
```

### 4. Data Structures

```rust
// New DelegateArgs - supports both modes
#[derive(Deserialize)]
#[serde(untagged)]
enum DelegateInput {
    // Simple mode (backward compatible)
    Simple {
        objective: String,
        #[serde(default)]
        context: Option<serde_json::Value>,
        #[serde(default)]
        system_prompt: Option<String>,
        #[serde(default)]
        tools: Option<Vec<String>>,
        #[serde(default)]
        max_iterations: Option<usize>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        workers: Option<usize>,
    },
    // Advanced mode with differentiated workers
    Advanced {
        #[serde(default)]
        coordination_mode: Option<String>, // "scratchpad" (default) | "none"
        #[serde(default)]
        shared_context: Option<String>,
        worker_configs: Vec<WorkerConfig>,
        #[serde(default)]
        max_iterations: Option<usize>,
        #[serde(default)]
        model: Option<String>,
    },
}

#[derive(Deserialize)]
struct WorkerConfig {
    /// Unique identifier for this worker
    id: String,
    /// Specific objective for this worker
    objective: String,
    /// Optional custom system prompt addition
    #[serde(default)]
    system_prompt: Option<String>,
    /// Tools this worker is allowed to use (subset of parent's tools)
    #[serde(default)]
    tools: Option<Vec<String>>,
    /// Command patterns that are auto-approved
    #[serde(default)]
    allowed_commands: Option<Vec<String>>,
    /// Command patterns that are forbidden
    #[serde(default)]
    forbidden_commands: Option<Vec<String>>,
    /// Tags this worker should use for scratchpad entries
    #[serde(default)]
    scratchpad_tags: Vec<String>,
    /// Other worker IDs this worker depends on
    #[serde(default)]
    depends_on: Vec<String>,
    /// Optional model override for this specific worker
    #[serde(default)]
    model: Option<String>,
    /// Optional max iterations override
    #[serde(default)]
    max_iterations: Option<usize>,
    /// Optional context specific to this worker
    #[serde(default)]
    context: Option<serde_json::Value>,
}
```

### 5. Implementation Flow

```
Main Agent calls delegate with worker_configs
                |
                v
+---------------------------------------------+
|  DelegateTool::call()                       |
|  - Parse worker_configs                     |
|  - Create shared coordination scratchpad    |
|  - Build dependency graph                   |
+---------------------------------------------+
                |
                v
+---------------------------------------------+
|  For each worker (in dependency order):    |
|  - Filter tools based on allowed list      |
|  - Build custom AgentPermissions           |
|  - Inject coordination instructions        |
|  - Spawn with shared scratchpad            |
+---------------------------------------------+
                |
                v
+---------------------------------------------+
|  Workers run with coordination:            |
|  - Read shared scratchpad before work      |
|  - Write CLAIM before modifying files      |
|  - Write PROGRESS after milestones         |
|  - Write COMPLETE when done                |
+---------------------------------------------+
                |
                v
+---------------------------------------------+
|  Main agent monitors via:                  |
|  - scratchpad list                         |
|  - list_jobs                               |
|  - EventBus events                         |
+---------------------------------------------+
```

### 6. System Prompt Updates for Main Model

Add new section to main system prompt:

```markdown
## Parallel Worker Delegation

Spawn multiple differentiated workers for complex tasks that can be parallelized.

### Simple Parallel Execution (Same Task)

Use when you want multiple workers doing the same thing (e.g., retry with different approaches):

```json
{"a": "delegate", "i": {"objective": "Search for API documentation", "workers": 3}}
```

### Coordinated Parallel Execution (Differentiated Workers)

Use when you need workers with different objectives that should coordinate:

```json
{
  "a": "delegate",
  "i": {
    "coordination_mode": "scratchpad",
    "shared_context": "Refactoring authentication system",
    "worker_configs": [
      {
        "id": "worker-1",
        "objective": "Update User model with new auth fields",
        "tools": ["read_file", "write_file"],
        "scratchpad_tags": ["models"]
      },
      {
        "id": "worker-2", 
        "objective": "Update auth middleware",
        "tools": ["read_file", "write_file", "grep"],
        "scratchpad_tags": ["middleware"],
        "depends_on": ["worker-1"]
      }
    ]
  }
}
```

### Worker Configuration Fields

| Field | Description | Example |
|-------|-------------|---------|
| `id` | Unique identifier | `"auth-refactor"` |
| `objective` | Specific task | `"Update login handler"` |
| `tools` | Allowed tools (subset of yours) | `["read_file", "write_file"]` |
| `allowed_commands` | Auto-approved patterns | `["cargo check *", "cargo test auth"]` |
| `forbidden_commands` | Blocked patterns | `["git push *", "rm -rf *"]` |
| `scratchpad_tags` | Tags for coordination | `["auth", "handlers"]` |
| `depends_on` | Wait for other workers | `["models-worker"]` |
| `system_prompt` | Additional instructions | `"Focus on error handling"` |

### Coordination Best Practices

1. **Assign distinct objectives**: Each worker should have a clear, non-overlapping scope
2. **Use meaningful IDs**: e.g., `"models"`, `"handlers"`, `"tests"` not `"worker-1"`
3. **Set appropriate tool limits**: Don't give `execute_command` unless needed
4. **Use scratchpad_tags**: Tag entries so workers can filter relevant updates
5. **Set dependencies**: Use `depends_on` to sequence work correctly
6. **Monitor progress**: Use `list_jobs` and `scratchpad` to track worker status

### Coordination via Scratchpad

Workers automatically use the scratchpad to coordinate:

- **CLAIM**: Workers announce files/modules they're working on
- **PROGRESS**: Workers report milestones  
- **RESULT**: Workers share intermediate findings
- **COMPLETE**: Workers signal completion

Monitor coordination:
```json
{"a": "scratchpad", "i": {"action": "list"}}
```

Get filtered view:
```json
{"a": "scratchpad", "i": {"action": "list", "tags": ["progress"]}}
```
```

### 7. Worker System Prompt Updates

Inject coordination section into worker system prompt:

```rust
fn build_worker_system_prompt(
    config: &WorkerConfig,
    shared_context: &str,
    coordination_scratchpad: &Arc<RwLock<StructuredScratchpad>>,
    parent_tools: &[Arc<dyn Tool>],
) -> String {
    format!(r#"You are Worker [{}] - a specialized sub-agent.

## Your Assignment
{}

## Shared Context
{}

## Coordination Protocol

You share a scratchpad with the main agent and other workers. You MUST coordinate to avoid conflicts.

### Required: Write to Scratchpad

After each significant action, append an entry:

```json
{{"a": "scratchpad", "i": {{"action": "append", "text": "YOUR_MESSAGE", "tags": {:?}, "persistent": true}}}}
```

Message formats:
- `CLAIM: src/models/user.rs` - Before working on a file
- `PROGRESS: Updated User struct` - After completing subtask
- `RESULT: auth_types.rs contains new types` - Share findings
- `BLOCKED: waiting for models worker` - Signal dependency need
- `COMPLETE: All auth models updated` - Signal done

### Required: Read Scratchpad

Before starting work, check what others are doing:

```json
{{"a": "scratchpad", "i": {{"action": "list"}}}}
```

Respect CLAIM entries - don't work on files others have claimed.

### Your Tags
Use these tags for your entries: {:?}

### Available Tools
{}
"#,
        config.id,
        config.objective,
        shared_context,
        config.scratchpad_tags,
        config.scratchpad_tags,
        format_capabilities_for_tools(parent_tools),
    )
}
```

## Implementation Phases

### Phase 1: Core Data Structures
- Update `DelegateArgs` to support `worker_configs`
- Add `WorkerConfig` struct with all fields
- Update JSON schema for parameter validation

### Phase 2: Worker Spawning
- Modify `spawn_worker` to accept `WorkerConfig`
- Implement tool filtering per worker
- Implement permission overrides per worker

### Phase 3: Coordination Scratchpad
- Create shared scratchpad for worker group
- Inject coordination instructions into worker prompt
- Add scratchpad to worker's tool set

### Phase 4: Dependency Management
- Build dependency graph from `depends_on`
- Implement sequential spawning for dependencies
- Signal dependency completion via scratchpad

### Phase 5: System Prompt Updates
- Update main model system prompt with delegation guide
- Update worker prompts with coordination protocol

## Migration Strategy

- **Backward compatible**: Old `workers: N` syntax continues to work
- **New features opt-in**: Use `worker_configs` for advanced features
- **Gradual adoption**: Main agent can use either mode as needed
