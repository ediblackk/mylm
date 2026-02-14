//! Common primitive types

use serde::{Deserialize, Serialize};

/// Approval decision
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Approval {
    Granted,
    Denied,
}
