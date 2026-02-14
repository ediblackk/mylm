//! Terminal application module - split into focused submodules
pub mod clipboard;
pub mod commands;
pub mod input;
pub mod session;
pub mod state;
pub mod app;

#[allow(unused_imports)]
pub use state::AppStateContainer;
pub use app::App;
