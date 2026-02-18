//! Terminal pane rendering

use crate::tui::app::state::AppStateContainer as App;
use crate::tui::app::types::Focus;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use tui_term::widget::PseudoTerminal;

pub fn render_terminal(frame: &mut Frame, app: &mut App, area: Rect) {
    // Store the offset for mouse coordinate translation
    app.terminal_area_offset = Some((area.x, area.y));

    let title = match app.focus {
        Focus::Terminal => " Terminal (F2) [Ctrl+B: Copy] ",
        _ => " Terminal ",
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(if app.focus == Focus::Terminal {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    if !app.terminal_auto_scroll {
        block = block.title_bottom(Line::from(vec![Span::styled(
            " [SCROLLBACK] ",
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
        )]));
    }

    // Dynamic Resizing
    let inner_height = area.height.saturating_sub(2);
    let inner_width = area.width.saturating_sub(2);
    if app.terminal_size != (inner_height, inner_width) {
        app.resize_pty(inner_width, inner_height);
    }

    let screen = app.terminal_parser.screen();

    // If we're auto-scrolling AND no selection is active, use the efficient PseudoTerminal widget from tui-term
    if app.terminal_auto_scroll && !app.is_selecting && app.selection_start.is_none() {
        let terminal = PseudoTerminal::new(screen).block(block);
        frame.render_widget(terminal, area);
        return;
    }

    // Custom Renderer for Scrolling and Selection
    let height = inner_height as usize;

    // Combine manual history with visible screen
    let mut all_lines = Vec::new();
    for h in &app.terminal_history {
        all_lines.push(h.as_str());
    }

    let screen_contents = screen.contents();
    let screen_lines: Vec<&str> = screen_contents.split('\n').collect();
    for s in screen_lines {
        all_lines.push(s);
    }

    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(height);
    // Clamp scroll to valid bounds
    app.terminal_scroll = app.terminal_scroll.clamp(0, max_scroll);

    let start_idx = if app.terminal_auto_scroll {
        total_lines.saturating_sub(height)
    } else {
        total_lines.saturating_sub(app.terminal_scroll).saturating_sub(height)
    };
    let end_idx = (start_idx + height).min(total_lines);

    let mut list_items = Vec::new();

    for (i, abs_line_idx) in (start_idx..end_idx).enumerate() {
        if let Some(line_content) = all_lines.get(abs_line_idx) {
            let mut spans = Vec::new();
            let row = area.y + 1 + i as u16;

            for (col_idx, c) in line_content.chars().enumerate() {
                let col = area.x + 1 + col_idx as u16;
                let is_selected = app.is_in_selection(col, row, Focus::Terminal);

                let style = if is_selected {
                    Style::default().bg(Color::Cyan).fg(Color::Black)
                } else if abs_line_idx < app.terminal_history.len() {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                };

                spans.push(Span::styled(c.to_string(), style));
            }
            list_items.push(ListItem::new(Line::from(spans)));
        }
    }

    // Fill remaining height if needed (shouldn't happen if logic is correct but good for safety)
    while list_items.len() < height {
        list_items.push(ListItem::new(Line::from("")));
    }

    let list = List::new(list_items).block(block);
    frame.render_widget(list, area);
}
