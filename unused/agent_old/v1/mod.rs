//! V1 Agent - LEGACY IMPLEMENTATION
//!
//! STATUS: MARKED FOR DELETION
//! This is the original agent implementation. It has been superseded by V2.
//! V2 is the current active implementation.
//!
//! This module is kept temporarily for reference during migration.
//! DO NOT add new features here.

pub mod core;

// Re-exports
pub use core::{Agent, AgentConfig, AgentDecision};
