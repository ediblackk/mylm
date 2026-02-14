//! Terminal setup and initialization module
//!
//! Handles raw mode, alternate screen, and cleanup via TerminalGuard.

use anyhow::Result;
use crossterm::{
    event::{EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{enable_raw_mode, EnterAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

/// RAII guard that ensures terminal cleanup on drop
pub struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best-effort cleanup, suppress all errors
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableBracketedPaste,
            crossterm::event::DisableMouseCapture,
            crossterm::cursor::Show
        );
    }
}

/// Initialize terminal in raw mode with alternate screen
pub fn init_terminal() -> Result<(Terminal<CrosstermBackend<io::Stdout>>, TerminalGuard)> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    let guard = TerminalGuard;

    Ok((terminal, guard))
}

/// Resize PTY based on terminal dimensions and chat width percentage
pub fn calculate_terminal_dimensions(
    total_width: u16,
    total_height: u16,
    chat_width_percent: u16,
) -> (u16, u16) {
    // Terminal pane is (100% - chat_width_percent) of width, minus borders
    let term_width =
        ((total_width as f32 * (1.0 - chat_width_percent as f32 / 100.0)) as u16).saturating_sub(2);
    let term_height = total_height.saturating_sub(4);
    (term_width, term_height)
}
