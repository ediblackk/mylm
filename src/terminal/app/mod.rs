//! Terminal application module - split into focused submodules
pub mod clipboard;
pub mod commands;
pub mod input;
pub mod session;
pub mod state;
pub mod app;

pub use state::Focus;
pub use app::{AppState, TuiEvent, App};
