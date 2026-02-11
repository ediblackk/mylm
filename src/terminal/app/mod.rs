//! Terminal application module - split into focused submodules
pub mod clipboard;
pub mod commands;
pub mod input;
pub mod session;
pub mod state;
pub mod app;

pub use state::{Focus, AppState};
#[allow(unused_imports)]
pub use state::AppStateContainer;
pub use app::{TuiEvent, App};
