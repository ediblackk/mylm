//! Governance - Authority and Permission Enforcement
//!
//! Runtime-level permission enforcement for the multi-agent system.
//!
//! # Security Model
//!
//! - **Runtime-enforced**: Authority checks happen in Runtime, not LLM
//! - **Identity-based**: AgentId determines permissions
//! - **Deny-by-default**: Unknown tools/commands are denied
//!
//! # Components
//!
//! - `AuthorityMatrix`: Permission definitions
//! - `Authority`: Runtime enforcement engine
//! - `Enforcer`: Bridges authority to approval system
//! - `WorkerStall`: Handle stalled worker resolution

pub mod authority;
pub mod enforcer;
pub mod worker_stall;

pub use authority::{
    Authority, AuthorityMatrix, MainPermissions, WorkerPermissions,
    ToolAccess, ShellAccess,
};
pub use enforcer::ApprovalEnforcer;
pub use worker_stall::{WorkerStall, StallResolution, StalledJob};
