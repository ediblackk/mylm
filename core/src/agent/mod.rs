//! Agent system for LLM-based task execution.
//!
//! ORGANIZATION STATUS:
//! - V1: Legacy implementation in `v1/` folder - MARKED FOR DELETION
//! - V2: Current active implementation in `v2/` folder
//! - Root level files: Shared components used by both V1 and V2
//! - `tools/`: Tool implementations
//!
//! STRUCTURE:
//! ```text
//! agent/
//! ├── mod.rs           # This file - module declarations
//! ├── tool.rs          # Tool trait (SHARED)
//! ├── protocol.rs      # Protocol types (SHARED)
//! ├── event_bus.rs     # Event bus (SHARED)
//! ├── tool_registry.rs # Tool registry (SHARED)
//! ├── prompt.rs        # Prompt builder (SHARED)
//! ├── execution.rs     # Execution helpers (SHARED)
//! ├── context.rs       # Context management (SHARED)
//! ├── permissions.rs   # Permissions (SHARED)
//! ├── role.rs          # Role definitions (SHARED)
//! ├── workspace.rs     # Workspace management (SHARED)
//! ├── wait.rs          # Wait functionality (SHARED)
//! ├── budget.rs        # Budget management (SHARED)
//! ├── logger.rs        # Logging (SHARED)
//! ├── wrapper.rs       # Agent wrapper (SHARED)
//! ├── traits.rs        # Shared traits (SHARED)
//! ├── toolcall_log.rs  # Tool call logging (SHARED)
//! ├── v1/              # V1 LEGACY - MARKED FOR DELETION
//! │   └── core.rs
//! ├── v2/              # V2 ACTIVE
//! │   ├── core.rs
//! │   ├── orchestrator/
//! │   └── ...
//! └── tools/           # Tool implementations
//! ```

// =============================================================================
// SHARED COMPONENTS (at root level)
// =============================================================================
pub mod tool;
pub mod protocol;
pub mod event_bus;
pub mod tool_registry;
pub mod toolcall_log;
pub mod execution;
pub mod context;
pub mod permissions;
pub mod role;
pub mod workspace;
pub mod wait;
pub mod budget;
pub mod logger;
pub mod wrapper;
pub mod traits;
pub mod prompt;
// =============================================================================
// ACTIVE MODULES
// =============================================================================
pub mod v2;
pub mod tools;

// =============================================================================
// LEGACY V1 - MARKED FOR DELETION
// =============================================================================
pub mod v1;  // TODO: DELETE after V2 migration complete

// =============================================================================
// RE-EXPORTS (Shared)
// =============================================================================
pub use tool::{Tool, ToolKind, ToolOutput};
pub use protocol::{AgentRequest, AgentResponse, AgentError, ShortKeyAction, parse_short_key_action_from_content};
pub use event_bus::{CoreEvent, EventBus};
pub use tool_registry::{ToolRegistry, ToolRegistryStats};
pub use traits::TerminalExecutor;
pub use workspace::SharedWorkspace;
pub use prompt::PromptBuilder;
pub use wrapper::AgentWrapper;

// =============================================================================
// V2 EXPORTS (Active implementation)
// =============================================================================
pub use v2::{AgentV2, AgentV2Config};
pub use v2::orchestrator::{AgentOrchestrator, OrchestratorConfig, ChatSessionHandle, ChatSessionMessage, TaskHandle};
pub use v2::jobs::{JobRegistry, JobStatus};

// =============================================================================
// LEGACY V1 EXPORTS - MARKED FOR DELETION
// =============================================================================
pub use v1::{Agent, AgentConfig, AgentDecision};  // TODO: DELETE
