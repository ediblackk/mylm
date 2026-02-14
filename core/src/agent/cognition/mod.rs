//! Pure cognitive kernel
//! 
//! 100% deterministic. No async. No IO.

pub mod state;
pub mod input;
pub mod decision;
pub mod engine;
pub mod error;
pub mod history;
pub mod llm_engine;

/// Adapter to bridge old CognitiveEngine to new AgencyKernel contract
/// 
/// This allows gradual migration from the old step-based API to the new
/// batch-process API without breaking existing implementations.
pub mod kernel_adapter;

pub use state::*;
pub use input::*;
pub use decision::*;
pub use engine::*;
pub use error::*;
pub use history::*;
pub use llm_engine::{LLMBasedEngine, ResponseParser};
