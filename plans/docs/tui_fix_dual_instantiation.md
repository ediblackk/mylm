# TUI Fix: Eliminate Dual Agent Instantiation

## Problem Summary
Currently, `src/terminal/mod.rs` creates TWO agents when using V2:
1. A V1 `Agent` struct (for UI compatibility)
2. A separate `AgentV2` for the orchestrator

This causes:
- Duplicate logs (both agents log)
- Wasted memory (duplicate scribe, scratchpad, etc.)
- Confusion about which agent has the "real" state

## Solution: Make AppState Use Either V1 or V2

### Step 1: Create an AgentState Trait

Create new file: `core/src/agent/state.rs`

```rust
//! Agent state trait for UI compatibility
//!
//! This trait abstracts the common state fields that the TUI needs
//! from both V1 and V2 agents, allowing the UI to work with either.

use crate::llm::chat::ChatMessage;
use crate::agent::tools::StructuredScratchpad;
use crate::agent::v2::jobs::JobRegistry;
use crate::memory::scribe::Scribe;
use std::sync::{Arc, RwLock};

/// Trait for agent state that the TUI needs to access
#[async_trait::async_trait]
pub trait AgentState: Send + Sync {
    /// Get the conversation history
    fn history(&self) -> &[ChatMessage];
    
    /// Set the conversation history (for session restore)
    fn set_history(&mut self, history: Vec<ChatMessage>);
    
    /// Get the scratchpad for structured data
    fn scratchpad(&self) -> Option<Arc<RwLock<StructuredScratchpad>>>;
    
    /// Get the session ID
    fn session_id(&self) -> &str;
    
    /// Set the session ID (for session restore)
    fn set_session_id(&mut self, session_id: String);
    
    /// Get the scribe for memory operations
    fn scribe(&self) -> Option<Arc<Scribe>>;
    
    /// Get the job registry
    fn job_registry(&self) -> &JobRegistry;
    
    /// Get reference to inner agent (for orchestrator)
    /// Returns None if this is V1 (orchestrator uses V2 only)
    fn as_agent_v2(&self) -> Option<&crate::agent::v2::AgentV2> {
        None
    }
    
    /// Get mutable reference to inner V2 agent
    fn as_agent_v2_mut(&mut self) -> Option<&mut crate::agent::v2::AgentV2> {
        None
    }
}

// Implement for V1 Agent
#[async_trait::async_trait]
impl AgentState for crate::agent::Agent {
    fn history(&self) -> &[ChatMessage] {
        &self.history
    }
    
    fn set_history(&mut self, history: Vec<ChatMessage>) {
        self.history = history;
    }
    
    fn scratchpad(&self) -> Option<Arc<RwLock<StructuredScratchpad>>> {
        self.scratchpad.clone()
    }
    
    fn session_id(&self) -> &str {
        &self.session_id
    }
    
    fn set_session_id(&mut self, session_id: String) {
        self.session_id = session_id;
    }
    
    fn scribe(&self) -> Option<Arc<Scribe>> {
        self.scribe.clone()
    }
    
    fn job_registry(&self) -> &JobRegistry {
        &self.job_registry
    }
    
    // V1 can return its embedded V2 if present
    fn as_agent_v2(&self) -> Option<&crate::agent::v2::AgentV2> {
        self.v2_agent.as_ref()
    }
    
    fn as_agent_v2_mut(&mut self) -> Option<&mut crate::agent::v2::AgentV2> {
        self.v2_agent.as_mut()
    }
}

// Implement for V2 Agent
#[async_trait::async_trait]
impl AgentState for crate::agent::v2::AgentV2 {
    fn history(&self) -> &[ChatMessage] {
        &self.history
    }
    
    fn set_history(&mut self, history: Vec<ChatMessage>) {
        self.history = history;
    }
    
    fn scratchpad(&self) -> Option<Arc<RwLock<StructuredScratchpad>>> {
        Some(self.scratchpad.clone())
    }
    
    fn session_id(&self) -> &str {
        &self.session_id
    }
    
    fn set_session_id(&mut self, session_id: String) {
        self.session_id = session_id;
    }
    
    fn scribe(&self) -> Option<Arc<Scribe>> {
        Some(self.scribe.clone())
    }
    
    fn job_registry(&self) -> &JobRegistry {
        &self.job_registry
    }
    
    fn as_agent_v2(&self) -> Option<&crate::agent::v2::AgentV2> {
        Some(self)
    }
    
    fn as_agent_v2_mut(&mut self) -> Option<&mut crate::agent::v2::AgentV2> {
        Some(self)
    }
}
```

### Step 2: Update AppStateContainer to Use Either Agent

Modify `src/terminal/app/state.rs`:

```rust
// Change line 13 from:
use mylm_core::agent::{Agent, EventBus};
// To:
use mylm_core::agent::{EventBus};
use mylm_core::agent::state::AgentState;

// Change line 116 from:
pub agent: Arc<Mutex<Agent>>,
// To an enum that can hold either:
pub agent: Arc<Mutex<dyn AgentState>>,
```

Actually, using `dyn AgentState` with Mutex is tricky. Better approach: use an enum.

```rust
/// Wrapper enum to hold either V1 or V2 agent
pub enum AgentWrapper {
    V1(Agent),
    V2(AgentV2),
}

impl AgentWrapper {
    pub fn history(&self) -> &[ChatMessage] {
        match self {
            AgentWrapper::V1(a) => &a.history,
            AgentWrapper::V2(a) => &a.history,
        }
    }
    
    pub fn set_history(&mut self, history: Vec<ChatMessage>) {
        match self {
            AgentWrapper::V1(a) => a.history = history,
            AgentWrapper::V2(a) => a.history = history,
        }
    }
    
    // ... implement other methods similarly
    
    /// Get V2 reference if this is V2 (for orchestrator)
    pub fn as_v2(&self) -> Option<&AgentV2> {
        match self {
            AgentWrapper::V2(a) => Some(a),
            _ => None,
        }
    }
    
    pub fn as_v2_mut(&mut self) -> Option<&mut AgentV2> {
        match self {
            AgentWrapper::V2(a) => Some(a),
            _ => None,
        }
    }
}

// Then in AppStateContainer:
pub agent: Arc<Mutex<AgentWrapper>>,
```

### Step 3: Update terminal/mod.rs to Create Only One Agent

Replace the current dual creation (lines ~200-310) with:

```rust
// Determine which agent version to use
let agent_wrapper = if agent_version == mylm_core::config::AgentVersion::V2 {
    // Create V2 agent ONCE
    let agent_v2_config = mylm_core::agent::v2::AgentV2Config {
        client: llm_client.clone(),
        scribe: scribe.clone(),
        tools: tools_list,
        system_prompt_prefix: v2_system_prompt_prefix,
        max_iterations,
        version: agent_version,
        memory_store: Some(store.clone()),
        categorizer: categorizer.clone(),
        job_registry: Some(job_registry.clone()),
        capabilities_context: None,
        permissions: resolved.agent.permissions.clone(),
        scratchpad: Some(scratchpad.clone()),
        disable_memory: incognito,
        event_bus: Some(event_bus.clone()),
        execute_tools_internally: false, // Let orchestrator handle execution
    };
    let agent_v2 = AgentV2::new_with_config(agent_v2_config);
    
    // Create orchestrator with the SAME agent
    let agent_v2_arc = Arc::new(Mutex::new(agent_v2));
    let orchestrator = AgentOrchestrator::new_with_agent_v2(
        agent_v2_arc.clone(),
        event_bus.clone(),
        orchestrator_config,
    ).await;
    
    // Return wrapper
    (AgentWrapper::V2(agent_v2_arc), Some(orchestrator))
} else {
    // V1 path - create V1 agent
    let config = mylm_core::agent::AgentConfig {
        client: llm_client.clone(),
        tools: tools_list,
        system_prompt_prefix,
        max_iterations,
        version: agent_version,
        memory_store: Some(store.clone()),
        categorizer: categorizer.clone(),
        job_registry: Some(job_registry.clone()),
        scratchpad: Some(scratchpad.clone()),
        disable_memory: incognito,
        permissions: resolved.agent.permissions.clone(),
        event_bus: Some(event_bus.clone()),
    };
    let agent_v1 = Agent::new_with_config(config).await;
    
    // Set additional fields
    agent_v1.scribe = Some(scribe.clone());
    agent_v1.disable_memory = incognito;
    agent_v1.scratchpad = Some(scratchpad.clone());
    agent_v1.version = agent_version;
    
    let agent_arc = Arc::new(Mutex::new(agent_v1));
    let orchestrator = AgentOrchestrator::new_with_agent_v1(
        agent_arc.clone(),
        event_bus.clone(),
        orchestrator_config,
    ).await;
    
    (AgentWrapper::V1(agent_arc), Some(orchestrator))
};

// Create app with the single agent
let mut app = App::new_with_orchestrator(
    pty_manager,
    agent_wrapper,  // Single agent
    config,
    scratchpad,
    job_registry,
    incognito,
    orchestrator,
    terminal_delegate.clone(),
    event_bus.clone(),
).await;
```

### Step 4: Update All Agent Field Accesses

In `src/terminal/mod.rs` and `src/terminal/app/*.rs`, change:

```rust
// From:
let mut agent = app.agent.lock().await;
agent.history = session.agent_history.clone();

// To:
let mut agent = app.agent.lock().await;
agent.set_history(session.agent_history.clone());
```

And:

```rust
// From:
let agent = app.agent.lock().await;
agent.scratchpad.as_ref()...

// To:
let agent = app.agent.lock().await;
agent.scratchpad().as_ref()...
```

## Alternative Simpler Approach

If the enum wrapper is too invasive, a simpler fix:

**Just fix the TUI to not create a V1 wrapper when using V2:**

```rust
// In terminal/mod.rs, when version is V2:
if agent_version == V2 {
    // Create V2 agent
    let agent_v2 = AgentV2::new_with_config(...);
    let agent_v2_arc = Arc::new(Mutex::new(agent_v2));
    
    // Create orchestrator
    let orchestrator = AgentOrchestrator::new_with_agent_v2(agent_v2_arc.clone(), ...);
    
    // HACK: Create a minimal V1 that references the V2
    // (requires adding a "proxy" variant to Agent)
    let agent_v1 = Agent::wrap_v2(agent_v2_arc.clone());
    let agent_arc = Arc::new(Mutex::new(agent_v1));
    
    // Both point to same underlying V2!
}
```

This requires adding to `core/src/agent/core.rs`:

```rust
impl Agent {
    /// Create a V1 wrapper that delegates to a V2 instance
    pub fn wrap_v2(v2: Arc<Mutex<AgentV2>>) -> Self {
        Self {
            // Most fields can be default/empty since we delegate
            llm_client: // get from v2,
            tool_registry: ToolRegistry::new(), // empty
            // ... other fields
            v2_agent: Some(v2), // Store the Arc to delegate to
            version: AgentVersion::V2,
            // ...
        }
    }
}
```

Then override all methods to delegate to V2 when `v2_agent` is `Some(Arc<Mutex<AgentV2>>)`.

## Recommendation

The **AgentWrapper enum** approach is cleanest and sets us up for future feature flags. It requires more changes but eliminates the conceptual confusion.

The **wrap_v2** hack is faster to implement (fewer files touched) but leaves technical debt.

Given the timeline, I recommend the **AgentWrapper enum** - it's the proper fix and can be done in 1-2 days.
