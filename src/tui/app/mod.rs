//! Terminal application module - Main TUI session with integrated terminal, chat
//!
//! Architecture:
//! - Terminal: vt100 emulator for rendering ANSI output inline
//! - Chat: Scrollable conversation history  
//! - Input: Command palette with auto-complete
//! - Event Loop: Main event handling

pub mod agent_setup;
pub mod approval;
pub mod controls;
pub mod draw;
pub mod event_loop;
pub mod help;
pub mod pty;
pub mod session;
pub mod session_manager;
pub mod state;
pub mod status_tracker;
pub mod terminal_executor;
pub mod types;
pub mod ui;
pub mod app;

// Re-export main types for convenience
pub use state::AppStateContainer;
pub use app::App;
pub use types::{
    AppState,
    TimestampedChatMessage,
    spawn_pty,
};
