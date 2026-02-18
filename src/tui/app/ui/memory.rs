//! Memory view rendering (F3)

use crate::tui::app::state::AppStateContainer as App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render_memory_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[1]);

    // Build title - show filter if active
    let title = if !app.memory_search_query.is_empty() {
        // Showing filtered results
        format!(
            " Memories (filter: '{}' - {}/{}) ↑↓:Scroll Esc:Clear r:Reload ",
            app.memory_search_query,
            app.memory_graph_scroll + 1,
            app.memory_graph.nodes.len()
        )
    } else {
        // Normal view
        format!(
            " Memories ({}/{}) ↑↓:Scroll Type:Filter r:Reload ",
            app.memory_graph_scroll + 1,
            app.memory_graph.nodes.len()
        )
    };

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Yellow));

    let mut items = Vec::new();
    for node in &app.memory_graph.nodes {
        let title = node.memory.content.lines().next().unwrap_or("Empty Memory");
        let truncated_title = if title.len() > 50 {
            format!("{}...", &title[..47])
        } else {
            title.to_string()
        };

        // Format timestamp
        let timestamp_str = super::utils::format_timestamp(node.memory.created_at);

        let type_tag = format!("[{}] ", node.memory.r#type);
        items.push(ListItem::new(Line::from(vec![
            Span::styled(timestamp_str, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(type_tag, Style::default().fg(Color::Cyan)),
            Span::raw(truncated_title),
        ])));
    }

    if items.is_empty() {
        let empty_msg = if !app.memory_search_query.is_empty() {
            format!(
                "No memories match filter: '{}' (press Esc to clear)",
                app.memory_search_query
            )
        } else {
            "No memories found.".to_string()
        };
        items.push(ListItem::new(Line::from(empty_msg)));
    }

    let list = List::new(items)
        .block(list_block)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray))
        .highlight_symbol("> ");

    // Clamp scroll to valid bounds and select highlighted item
    let mut list_state = ratatui::widgets::ListState::default();
    if !app.memory_graph.nodes.is_empty() {
        let max_scroll = app.memory_graph.nodes.len().saturating_sub(1);
        app.memory_graph_scroll = app.memory_graph_scroll.clamp(0, max_scroll);
        list_state.select(Some(app.memory_graph_scroll));
    }

    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    // Render details of selected node
    let detail_block = Block::default()
        .borders(Borders::ALL)
        .title(" Memory Details & Connections ")
        .border_style(Style::default().fg(Color::Cyan));

    if !app.memory_graph.nodes.is_empty() {
        let idx = app
            .memory_graph_scroll
            .clamp(0, app.memory_graph.nodes.len().saturating_sub(1));
        let node = &app.memory_graph.nodes[idx];

        let mut detail_lines = Vec::new();
        detail_lines.push(Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Gray)),
            Span::raw(node.memory.id.to_string()),
        ]));
        detail_lines.push(Line::from(vec![
            Span::styled("Time: ", Style::default().fg(Color::Gray)),
            Span::raw(super::utils::format_timestamp_full(node.memory.created_at)),
        ]));
        detail_lines.push(Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::Gray)),
            Span::raw(node.memory.r#type.to_string()),
        ]));
        if let Some(cat) = &node.memory.category_id {
            detail_lines.push(Line::from(vec![
                Span::styled("Category: ", Style::default().fg(Color::Gray)),
                Span::raw(cat),
            ]));
        }
        if let Some(summary) = &node.memory.summary {
            detail_lines.push(Line::from(vec![
                Span::styled("Summary (Index): ", Style::default().fg(Color::Gray)),
                Span::raw(summary),
            ]));
        }
        detail_lines.push(Line::from(""));
        detail_lines.push(Line::from(Span::styled(
            "Content:",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )));

        // Clean up content - unescape JSON newlines and tabs for display
        let cleaned_content = node
            .memory
            .content
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"");

        for line in cleaned_content.lines() {
            detail_lines.push(Line::from(line));
        }

        detail_lines.push(Line::from(""));
        detail_lines.push(Line::from(Span::styled(
            "Connections:",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )));
        if node.connections.is_empty() {
            detail_lines.push(Line::from(" (No direct connections identified)"));
        } else {
            for conn_id in &node.connections {
                detail_lines.push(Line::from(format!(" - Linked to Memory {}", conn_id)));
            }
        }

        let p = Paragraph::new(detail_lines)
            .block(detail_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(p, right_chunks[0]);
    } else {
        let p = Paragraph::new("Select a memory to see details.").block(detail_block);
        frame.render_widget(p, right_chunks[0]);
    }

    // Render Scratchpad (disabled in new architecture)
    let scratchpad_block = Block::default()
        .borders(Borders::ALL)
        .title(" Scratchpad (Disabled) ")
        .border_style(Style::default().fg(Color::DarkGray));

    let scratchpad_p = Paragraph::new("Scratchpad not available in new architecture")
        .block(scratchpad_block)
        .wrap(Wrap { trim: true });

    frame.render_widget(scratchpad_p, right_chunks[1]);
}
