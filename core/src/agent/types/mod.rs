//! Shared types for the agent system
//! 
//! NO dependencies on other modules within agent/.
//! Both cognition and contract import from here.

pub mod ids;
pub mod intents;
pub mod events;
pub mod observations;
pub mod common;

// Re-exports for convenience
pub use ids::*;
pub use intents::*;
pub use events::*;
pub use observations::*;
pub use common::*;
