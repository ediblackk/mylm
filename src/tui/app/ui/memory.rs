//! Memory view rendering (F3)

use crate::tui::app::state::AppStateContainer as App;
use mylm_core::config::agent::UserProfile;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

/// Render user profile panel
fn render_profile_panel(profile: &UserProfile) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    
    if profile.is_empty() {
        lines.push(Line::from("No profile data yet."));
        lines.push(Line::from("The system will learn your preferences automatically."));
        return lines;
    }
    
    // Preferences
    if !profile.preferences.is_empty() {
        lines.push(Line::from(Span::styled(
            "Preferences:",
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
        )));
        for (k, v) in &profile.preferences {
            lines.push(Line::from(format!("  • {}: {}", k, v)));
        }
        lines.push(Line::from(""));
    }
    
    // Facts
    if !profile.facts.is_empty() {
        lines.push(Line::from(Span::styled(
            "Known Facts:",
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Green),
        )));
        for (k, v) in &profile.facts {
            lines.push(Line::from(format!("  • {}: {}", k, v)));
        }
        lines.push(Line::from(""));
    }
    
    // Patterns
    if !profile.patterns.is_empty() {
        lines.push(Line::from(Span::styled(
            "Behavioral Patterns:",
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
        )));
        for pattern in &profile.patterns {
            lines.push(Line::from(format!("  • {}", pattern)));
        }
        lines.push(Line::from(""));
    }
    
    // Goals
    if !profile.active_goals.is_empty() {
        lines.push(Line::from(Span::styled(
            "Active Goals:",
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Magenta),
        )));
        for goal in &profile.active_goals {
            lines.push(Line::from(format!("  • {}", goal)));
        }
    }
    
    lines
}

pub fn render_memory_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(25), Constraint::Percentage(25)])
        .split(chunks[1]);

    // Build title - show filter status and pagination info
    let title = if !app.memory_search_query.is_empty() {
        // Showing filtered results
        format!(
            " Memories (filter: '{}' - {}/{}) ↑↓:Scroll d:Del s:Star r:Reload ",
            app.memory_search_query,
            app.memory_graph_scroll + 1,
            app.memory_graph.nodes.len()
        )
    } else if app.memory_total_count > app.memory_page_size {
        // Paginated view
        let total_pages = (app.memory_total_count + app.memory_page_size - 1) / app.memory_page_size;
        format!(
            " Memories (page {}/{} - {}/{}) ↑↓:Scroll Sh+PgUp/Dn:Page r:Reload ",
            app.memory_current_page + 1,
            total_pages,
            app.memory_graph_scroll + 1,
            app.memory_graph.nodes.len()
        )
    } else {
        // Normal view (all loaded)
        format!(
            " Memories ({}/{}) ↑↓:Scroll d:Del s:Star e:Export r:Reload ",
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

        // Show star indicator for starred memories
        let star_indicator = if node.memory.category_id.as_ref() == Some(&"starred".to_string()) {
            "⭐ "
        } else {
            "  "
        };

        let type_tag = format!("[{}] ", node.memory.r#type);
        items.push(ListItem::new(Line::from(vec![
            Span::styled(timestamp_str, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::raw(star_indicator),
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

    // Render User Profile Panel
    let profile_block = Block::default()
        .borders(Borders::ALL)
        .title(" User Profile (Auto-Learned) ")
        .border_style(Style::default().fg(Color::Magenta));

    let profile = app.memory_manager.as_ref().map(|m| m.get_profile());
    let profile_lines = if let Some(ref p) = profile {
        render_profile_panel(p)
    } else {
        vec![Line::from("Memory manager not available.")]
    };

    let profile_p = Paragraph::new(profile_lines)
        .block(profile_block)
        .wrap(Wrap { trim: true });
    frame.render_widget(profile_p, right_chunks[1]);

    // Render Stats & Actions Panel (replaces disabled scratchpad)
    let stats_block = Block::default()
        .borders(Borders::ALL)
        .title(" Memory Stats & Actions ")
        .border_style(Style::default().fg(Color::Green));

    let total_loaded = app.memory_graph_original.as_ref()
        .map(|g| g.nodes.len())
        .unwrap_or(app.memory_graph.nodes.len());
    let showing = app.memory_graph.nodes.len();
    let total_db = app.memory_total_count;
    
    let page_info = if app.memory_page_size < total_db {
        let total_pages = (total_db + app.memory_page_size - 1) / app.memory_page_size;
        format!("Page {}/{}", app.memory_current_page + 1, total_pages)
    } else {
        "All loaded".to_string()
    };
    
    let filter_info = if app.memory_search_query.is_empty() {
        "No filter".to_string()
    } else {
        format!("Filter: '{}'", app.memory_search_query)
    };
    
    // Show selected memory info if any
    let selected_info = if !app.memory_graph.nodes.is_empty() {
        let idx = app.memory_graph_scroll.clamp(0, app.memory_graph.nodes.len() - 1);
        let node = &app.memory_graph.nodes[idx];
        let is_starred = node.memory.category_id.as_ref() == Some(&"starred".to_string());
        format!("\nSelected: {} {}", 
            if is_starred { "⭐" } else { "  " },
            &node.memory.content[..node.memory.content.len().min(30)]
        )
    } else {
        String::new()
    };

    // Get user profile info if available
    let profile_info = if let Some(ref manager) = app.memory_manager {
        let profile = manager.get_profile();
        if profile.is_empty() {
            "Profile: (empty)\n".to_string()
        } else {
            let prefs = if profile.preferences.is_empty() {
                String::new()
            } else {
                format!("  Prefs: {}\n", profile.preferences.len())
            };
            let facts = if profile.facts.is_empty() {
                String::new()
            } else {
                format!("  Facts: {}\n", profile.facts.len())
            };
            let patterns = if profile.patterns.is_empty() {
                String::new()
            } else {
                format!("  Patterns: {}\n", profile.patterns.len())
            };
            let goals = if profile.active_goals.is_empty() {
                String::new()
            } else {
                format!("  Goals: {}\n", profile.active_goals.len())
            };
            format!("Profile:\n{}{}{}{}", prefs, facts, patterns, goals)
        }
    } else {
        "Profile: (disabled)\n".to_string()
    };

    let stats_text = format!(
        "Database: {} total\nLoaded: {} (in memory)\nShowing: {}\n\n{}\n{}{}\n\n{}\nNavigation:\n↑↓ Scroll  r Reload\nShift+PgUp/Dn Page\nEsc Clear filter\n\nActions:\nd Delete  s Star/Unstar\ne Export (clipboard)\nD Delete all filtered",
        total_db, total_loaded, showing, page_info, filter_info, selected_info, profile_info
    );

    let stats_p = Paragraph::new(stats_text)
        .block(stats_block)
        .wrap(Wrap { trim: true });

    frame.render_widget(stats_p, right_chunks[2]);
}
