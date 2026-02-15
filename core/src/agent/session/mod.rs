//! Session orchestration
//!
//! Coordinates cognition + runtime.

pub mod session;
pub mod input;
pub mod persistence;

pub use session::*;
pub use input::*;
pub use persistence::{
    SessionPersistence, PersistedSession, SessionMetadata,
    AgentStateCheckpoint, SessionBuilder,
};
