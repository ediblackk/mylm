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
//! - `StallScheduler`: Handle stalled worker resolution

pub mod authority;
pub mod stall_scheduler;

pub use authority::{
    Authority, AuthorityMatrix, MainPermissions, WorkerPermissions,
    ToolAccess, ShellAccess,
};
pub use stall_scheduler::{StallScheduler, StallResolution, StalledJob};
