//! Step-based Engine Implementations
//!
//! These modules implement the `StepEngine` trait for single-step decision making.
//! Use when you need simple (state, input) -> Transition semantics.

/// LLM-based step engine
/// 
/// Implements `StepEngine` using an LLM to make decisions.
pub mod llm_engine;
