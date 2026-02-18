//! UI component types module
//!
//! Provides visual elements for the TUI interface.

pub mod action_stamp;

// Re-export action stamp types
pub use action_stamp::{ActionStamp, ActionStampType, ActionStampRegistry, stamps};
