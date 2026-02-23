//! Pure cognitive kernel
//! 
//! 100% deterministic. No async. No IO.
//!
//! Architecture:
//! - `kernel.rs`: Core `GraphEngine` trait + unified `AgentState`
//! - `planner.rs`: Decision-making planner implementing `GraphEngine`
//! - `step/`: Step-based engine implementations (`StepEngine` trait)
//! - `prompts/`: Prompt construction
//! - `policy/`: Approval and safety policies
//! - `input/decision/engine/error/history`: Supporting modules

/// Graph-based engine trait (GraphEngine) for DAG intent planning
pub mod kernel;

/// GraphPlanner - decision-making component producing intent graphs
pub mod planner;

/// Step-based engine implementations
pub mod step;

/// Prompt construction modules
pub mod prompts;

/// Policy modules (approval, safety)
pub mod policy;

// Supporting modules for both engine types
pub mod input;
pub mod decision;
pub mod engine;
pub mod error;
pub mod history;

// Re-exports
pub use kernel::{GraphEngine, AgentState, KernelError, TokenUsage, PendingApproval, StubGraphEngine};
pub use planner::Planner;
pub use step::llm_engine::LlmEngine;
pub use prompts::system::{ToolDescription, build_system_prompt, build_tool_defs};
pub use policy::{ApprovalPolicy, requires_approval};

// Supporting module re-exports
pub use input::{InputEvent, WorkerId, ApprovalOutcome, LLMResponse};
pub use decision::{Transition, AgentDecision, AgentExitReason};
pub use engine::{StepEngine, StubEngine};
pub use error::CognitiveError;
pub use history::Message;
