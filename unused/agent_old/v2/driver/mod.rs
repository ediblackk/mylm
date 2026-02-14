//! Agent execution drivers (legacy and event-driven)
pub mod legacy;
pub mod event_driven;
pub mod factory;

pub use legacy::run_legacy;
pub use event_driven::run_event_driven;
pub use factory::{AgentConfigs, AgentBuilder, BuiltAgent, create_basic_agent, create_development_agent, create_web_agent, create_full_agent};
