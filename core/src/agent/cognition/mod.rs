//! Pure cognitive kernel
//! 
//! 100% deterministic. No async. No IO.
//!
//! Architecture:
//! - `kernel.rs`: Core `AgencyKernel` trait + `AgentState`
//! - `planner.rs`: Decision-making planner implementing `AgencyKernel`
//! - `prompts/`: Prompt construction
//! - `policy/`: Approval and safety policies
//! - `unused/`: Deprecated modules (legacy_planner)
//! - Legacy modules (to be migrated): state, input, decision, engine, error, history

/// Pure kernel trait (AgencyKernel) for state machine processing
pub mod kernel;

/// Planner - decision-making component producing intent graphs
pub mod planner;

/// Prompt construction modules
pub mod prompts;

/// Policy modules (approval, safety)
pub mod policy;

/// Unused/deprecated modules
pub mod unused;

// Legacy modules - these will be phased out as we migrate to kernel-based architecture
pub mod state;
pub mod input;
pub mod decision;
pub mod engine;
pub mod error;
pub mod history;

// Note: kernel_adapter has been removed. Use `Planner` directly which implements `AgencyKernel`.

// Re-exports
pub use kernel::{AgencyKernel, AgentState, KernelError, TokenUsage, PendingApproval};
pub use planner::Planner;
pub use prompts::system::{ToolDescription, build_system_prompt, build_tool_defs};
pub use policy::{ApprovalPolicy, requires_approval};

// Legacy re-exports
pub use state::AgentState as LegacyAgentState;
pub use input::{InputEvent, WorkerId, ApprovalOutcome, LLMResponse};
pub use decision::{Transition, AgentDecision, AgentExitReason};
pub use engine::{CognitiveEngine, StubEngine};
pub use error::CognitiveError;
pub use history::Message;

// Legacy planner - deprecated, kept for reference in unused/
pub use unused::legacy_planner::LLMBasedEngine as LegacyPlanner;
