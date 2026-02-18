//! Exit confirmation dialog

use crate::tui::app::state::AppStateContainer as App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_confirm_exit(frame: &mut Frame, _app: &mut App) {
    let area = frame.area();

    // Simple centered dialog for y/n confirmation
    let dialog_area = super::centered_rect(50, 20, area);

    // Clear background
    frame.render_widget(ratatui::widgets::Clear, dialog_area);

    let block = Block::default()
        .title(" ⚠️  Exit Confirmation ")
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(Color::Black));

    frame.render_widget(block, dialog_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(2), // Question
            Constraint::Min(0),    // Instructions
        ])
        .split(dialog_area);

    let question = Paragraph::new(Line::from(vec![Span::raw(
        "Are you sure you want to exit?",
    )]))
    .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(question, chunks[0]);

    let instructions = Paragraph::new(Line::from(vec![
        Span::styled(
            " [Y] ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("Yes, exit"),
        Span::raw("  "),
        Span::styled(
            " [N] ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw("No, cancel"),
    ]))
    .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(instructions, chunks[1]);
}
