//! Identifier types
//!
//! Type-safe identifiers for entities across the system.
//! All identifiers are cheap to clone (Copy where possible).

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier for an intent within a graph
/// 
/// Deterministically derived from kernel evolution.
/// Format: high 32 bits = step_count, low 32 bits = intent_index
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IntentId(pub u64);

impl IntentId {
    /// Create a new intent ID from a raw value
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the next sequential ID
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }

    /// Create deterministic IntentId from kernel state
    pub fn from_step(step_count: u32, intent_index: u32) -> Self {
        let id = ((step_count as u64) << 32) | (intent_index as u64);
        Self(id)
    }

    /// Extract step count (high 32 bits)
    pub fn step_count(&self) -> u32 {
        (self.0 >> 32) as u32
    }

    /// Extract intent index (low 32 bits)
    pub fn intent_index(&self) -> u32 {
        (self.0 & 0xFFFFFFFF) as u32
    }
}

impl std::fmt::Display for IntentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "intent:{}", self.0)
    }
}

/// Unique identifier for a node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

impl NodeId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "node:{}", self.0)
    }
}

/// Unique identifier for an event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EventId(pub u64);

impl EventId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "event:{}", self.0)
    }
}

/// Logical clock for event ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LogicalClock(pub u64);

impl LogicalClock {
    pub const fn zero() -> Self {
        Self(0)
    }

    pub const fn new(time: u64) -> Self {
        Self(time)
    }

    pub fn increment(&mut self) {
        self.0 += 1;
    }

    pub fn update(&mut self, received: LogicalClock) {
        self.0 = std::cmp::max(self.0, received.0) + 1;
    }

    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl std::fmt::Display for LogicalClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "t{}", self.0)
    }
}

/// Unique identifier for a session
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

/// Task identifier
pub type TaskId = String;

/// Job identifier
pub type JobId = String;

impl SessionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session:{}", self.0)
    }
}

/// Generator for sequential IDs
pub struct IdGenerator {
    next: AtomicU64,
}

impl IdGenerator {
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    pub fn starting_at(start: u64) -> Self {
        Self {
            next: AtomicU64::new(start),
        }
    }

    pub fn next_intent(&self) -> IntentId {
        IntentId(self.next.fetch_add(1, Ordering::SeqCst))
    }

    pub fn next_event(&self) -> EventId {
        EventId(self.next.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_id_from_step() {
        let id = IntentId::from_step(5, 3);
        assert_eq!(id.step_count(), 5);
        assert_eq!(id.intent_index(), 3);
    }

    #[test]
    fn test_logical_clock() {
        let mut c1 = LogicalClock::new(5);
        let c2 = LogicalClock::new(10);
        c1.update(c2);
        assert_eq!(c1.0, 11);
    }
}
