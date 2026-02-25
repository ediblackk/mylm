//! Common primitive types
//!
//! Shared primitive types used across the agent system.
//! These are the basic building blocks - no logic, just data.
//!
//! Links:
//! - Used by: cognition (approval decisions), runtime (telemetry)
//! - Key types: `Approval` (grant/deny), `TokenUsage` (LLM costs)

use serde::{Deserialize, Serialize};

/// Approval decision
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Approval {
    Granted,
    Denied,
}
