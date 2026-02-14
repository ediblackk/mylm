//! Capability Graph Runtime
//!
//! Async, side-effect capable. No decision logic.

pub mod capability;
pub mod context;
pub mod graph;
pub mod runtime;
pub mod error;
pub mod impls;

/// Contract runtime implementation
/// 
/// Bridges the new contract's AgencyRuntime trait to existing V3 capabilities.
pub mod contract_runtime;

pub use capability::*;
pub use context::*;
pub use graph::*;
pub use runtime::*;
pub use error::*;
pub use contract_runtime::ContractRuntime;
