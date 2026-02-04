//! Agent execution drivers (legacy and event-driven)
pub mod legacy;
pub mod event_driven;

pub use legacy::run_legacy;
pub use event_driven::run_event_driven;
