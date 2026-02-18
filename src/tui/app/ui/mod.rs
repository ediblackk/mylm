//! UI Rendering Module
//!
//! Split into focused submodules for maintainability.

pub mod chat;
pub mod confirm_exit;
pub mod help;
pub mod jobs;
pub mod memory;
pub mod terminal;
pub mod top_bar;
pub mod utils;

// Re-export utility functions
pub use utils::calculate_terminal_dimensions;

use crate::tui::app::state::AppStateContainer as App;
use crate::tui::app::types::AppState;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

/// Main render entry point
pub fn render(frame: &mut Frame, app: &mut App) {
    // Fixed heights: top bar (2 lines) + bottom bar (1 line)
    let top_bar_height = 2u16;
    let bottom_bar_height = 1u16;

    // Job panel height (fixed at 8 rows when visible to show 2-line job entries)
    let job_panel_height = if app.show_jobs_panel { 8u16 } else { 0u16 };

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_bar_height),
            Constraint::Min(0),
            Constraint::Length(bottom_bar_height),
            Constraint::Length(job_panel_height),
        ])
        .split(frame.area());

    top_bar::render_top_bar(frame, app, main_layout[0], top_bar_height);

    // Compute layout first - needed for all view modes
    let terminal_visible = app.show_terminal && app.chat_width_percent < 100;
    let chunks = if terminal_visible {
        let chat_pct = app.chat_width_percent;
        let term_pct = 100u16.saturating_sub(chat_pct);
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(term_pct), Constraint::Percentage(chat_pct)])
            .split(main_layout[1])
    } else {
        // Terminal hidden or chat at 100%, chat takes full width
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Percentage(100)])
            .split(main_layout[1])
    };

    // Render based on view state
    if app.show_job_detail && terminal_visible {
        // Job detail renders over terminal pane only (like Help)
        jobs::render_job_detail(frame, app, chunks[0]);
        chat::render_chat(frame, app, chunks[1]);
    } else if app.show_job_detail {
        // Terminal hidden - use full width for job detail
        jobs::render_job_detail(frame, app, main_layout[1]);
    } else if app.show_memory_view {
        memory::render_memory_view(frame, app, main_layout[1]);
    } else {
        // Normal layout
        if terminal_visible {
            if app.show_help_view {
                help::render_help_view(frame, app, chunks[0]);
            } else {
                terminal::render_terminal(frame, app, chunks[0]);
            }
        }
        // Chat is always rendered
        chat::render_chat(frame, app, chunks[1]);
    }

    // Bottom bar with F-keys and toggles
    render_bottom_bar(frame, app, main_layout[2]);

    // Render job panel at bottom if visible
    if app.show_jobs_panel {
        jobs::render_jobs_panel(frame, app, main_layout[3]);
    }

    if app.state == AppState::ConfirmExit {
        confirm_exit::render_confirm_exit(frame, app);
    }
}

/// Render bottom bar - now empty since everything moved to top
fn render_bottom_bar(_frame: &mut Frame, _app: &mut App, _area: ratatui::layout::Rect) {
    // All controls moved to top bar
}

/// Helper to create a centered rectangle
pub fn centered_rect(percent_x: u16, percent_y: u16, r: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
