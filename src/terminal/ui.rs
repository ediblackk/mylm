use crate::terminal::app::{App, Focus, AppState};
use crate::llm::chat::MessageRole;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Modifier},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame,
};
use tui_term::widget::PseudoTerminal;

pub fn render(frame: &mut Frame, app: &mut App) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(frame.area());

    render_top_bar(frame, app, main_layout[0]);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(main_layout[1]);

    render_terminal(frame, app, chunks[0]);
    render_chat(frame, app, chunks[1]);
}

fn render_top_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let stats = app.session_monitor.get_stats();
    let duration = app.session_monitor.format_duration();

    let active_profile = app.config.active_profile.clone();
    let profile_data = app.config.get_active_profile();
    let endpoint_name = profile_data.map(|p| p.endpoint.as_str()).unwrap_or("?");
    let prompt_name = profile_data.map(|p| p.prompt.as_str()).unwrap_or("?");
    
    let model_name = if let Ok(e) = app.config.get_endpoint(profile_data.map(|p| p.endpoint.as_str())) {
        e.model.clone()
    } else {
        "unknown".to_string()
    };

    let (verbose_text, verbose_color) = if app.verbose_mode {
        (" [VERBOSE: ON] ", Color::Magenta)
    } else {
        (" [VERBOSE: OFF] ", Color::DarkGray)
    };
    let auto_approve_text = if app.auto_approve { " [AUTO-APPROVE: ON] " } else { " [AUTO-APPROVE: OFF] " };

    let stats_text = vec![
        Line::from(vec![
            Span::styled("Profile: ", Style::default().fg(Color::Gray)),
            Span::styled(active_profile, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::raw(" ("),
            Span::styled(endpoint_name, Style::default().fg(Color::DarkGray)),
            Span::raw(":"),
            Span::styled(model_name, Style::default().fg(Color::DarkGray)),
            Span::raw(") | "),
            Span::styled("Prompt: ", Style::default().fg(Color::Gray)),
            Span::styled(prompt_name, Style::default().fg(Color::White)),
            Span::raw(" | "),
            Span::styled("Tokens: ", Style::default().fg(Color::Gray)),
            Span::styled(stats.total_tokens.to_string(), Style::default().fg(Color::Cyan)),
            Span::raw(" ("),
            Span::styled(stats.input_tokens.to_string(), Style::default().fg(Color::DarkGray)),
            Span::raw("/"),
            Span::styled(stats.output_tokens.to_string(), Style::default().fg(Color::DarkGray)),
            Span::raw(") | "),
            Span::styled("Cost: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("${:.2}", stats.cost), Style::default().fg(Color::Green)),
            Span::raw(" | "),
            Span::styled("Time: ", Style::default().fg(Color::Gray)),
            Span::styled(duration, Style::default().fg(Color::Gray)),
            Span::styled(verbose_text, Style::default().fg(verbose_color).add_modifier(Modifier::BOLD)),
            Span::styled(auto_approve_text, Style::default().fg(if app.auto_approve { Color::Green } else { Color::Red }).add_modifier(Modifier::BOLD)),
        ])
    ];

    let ratio = app.session_monitor.get_context_ratio();
    let gauge_color = if ratio > 0.8 {
        Color::Red
    } else if ratio > 0.5 {
        Color::Yellow
    } else {
        Color::Green
    };

    let label = format!("Context: {}/{} ({:.0}%)",
        stats.active_context_tokens,
        stats.max_context_tokens,
        ratio * 100.0
    );

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    let top_row_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(18), // Version
            Constraint::Min(0),     // Gauge
        ])
        .split(rows[0]);

    let version_text = format!(" myLM v{}-{} ", env!("CARGO_PKG_VERSION"), env!("BUILD_NUMBER"));
    let version_p = Paragraph::new(Span::styled(
        version_text,
        Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
    ));
    
    let gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::default().fg(gauge_color).bg(Color::DarkGray))
        .ratio(ratio.clamp(0.0, 1.0))
        .label(label);

    let stats_p = Paragraph::new(stats_text).alignment(ratatui::layout::Alignment::Right);

    frame.render_widget(version_p, top_row_chunks[0]);
    frame.render_widget(gauge, top_row_chunks[1]);
    frame.render_widget(stats_p, rows[1]);
}

fn render_terminal(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = match app.focus {
        Focus::Terminal => " Terminal (Focus: Ctrl+x) ",
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
        block = block.title_bottom(Line::from(vec![
            Span::styled(" [SCROLLBACK] ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD))
        ]));
    }

    // Dynamic Resizing
    let inner_height = area.height.saturating_sub(2);
    let inner_width = area.width.saturating_sub(2);
    if app.terminal_size != (inner_height, inner_width) {
        app.resize_pty(inner_width, inner_height);
    }

    let screen = app.terminal_parser.screen();
    
    // If we're auto-scrolling, use the efficient PseudoTerminal widget from tui-term
    if app.terminal_auto_scroll {
        let terminal = PseudoTerminal::new(screen)
            .block(block);
        frame.render_widget(terminal, area);
        return;
    }

    // Custom Renderer for Scrolling
    // Since tui-term 0.1.x PseudoTerminal doesn't support manual scrolling offset easily,
    // we implement a basic renderer here to support viewing history.
    
    let height = inner_height as usize;
    
    // Combine manual history with visible screen
    let mut all_lines = Vec::new();
    for h in &app.terminal_history {
        all_lines.push((h.as_str(), Style::default().fg(Color::DarkGray)));
    }
    
    let screen_contents = screen.contents();
    let screen_lines: Vec<&str> = screen_contents.split('\n').collect();
    for s in screen_lines {
        all_lines.push((s, Style::default().fg(Color::White)));
    }

    let total_lines = all_lines.len();
    let max_scroll = total_lines.saturating_sub(height);
    let effective_scroll = app.terminal_scroll.min(max_scroll);
    
    let start_idx = total_lines.saturating_sub(effective_scroll).saturating_sub(height);
    let end_idx = (start_idx + height).min(total_lines);
    
    let mut list_items = Vec::new();
    
    for i in start_idx..end_idx {
        if let Some((line_content, style)) = all_lines.get(i) {
             list_items.push(ListItem::new(Line::from(Span::styled(line_content.to_string(), *style))));
        }
    }
    
    // Fill remaining height if needed (shouldn't happen if logic is correct but good for safety)
    while list_items.len() < height {
        list_items.push(ListItem::new(Line::from("")));
    }

    let list = List::new(list_items).block(block);
    frame.render_widget(list, area);
}


fn render_chat(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(area);

    let title = match app.focus {
        Focus::Chat => " AI Chat (Focus: Ctrl+x) ",
        _ => " AI Chat ",
    };

    // Chat history with manual wrapping for correct scrolling
    let available_width = chunks[0].width.saturating_sub(2) as usize;
    let mut list_items = Vec::new();

    for m in &app.chat_history {
        // Aggressively hide command outputs in non-verbose mode
        if !app.verbose_mode && m.content.contains("CMD_OUTPUT:") {
            // Only show the placeholder for the observation/tool result itself, not for every message containing the string
            if m.role == MessageRole::Tool || (m.role == MessageRole::User && m.content.contains("Observation:")) {
                list_items.push(ListItem::new(Line::from(vec![
                    Span::styled("AI: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled("Command executed. Check terminal.", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ])));
                list_items.push(ListItem::new(Line::from("")));
            }
            continue;
        }

        // Skip Tool messages for commands in non-verbose mode (redundant with above but safe)
        if !app.verbose_mode && m.role == MessageRole::Tool && m.name.as_deref() == Some("execute_command") {
            continue;
        }

        let (prefix, color) = match m.role {
            MessageRole::User => ("You: ", Color::Cyan),
            MessageRole::Assistant => ("AI: ", Color::Green),
            MessageRole::System => ("Sys: ", Color::Gray),
            _ => ("AI: ", Color::Green),
        };

        let mut lines_to_render = Vec::new();
        
        let delimiter = "\n\n---\n[TERMINAL STATE ATTACHMENT]";
        let (display_content, has_hidden_context) = if m.content.contains(delimiter) {
            let parts: Vec<&str> = m.content.split(delimiter).collect();
            (parts[0], true)
        } else {
            (m.content.as_str(), false)
        };

        let raw_lines: Vec<&str> = display_content.split('\n').collect();
        
        for line in raw_lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                lines_to_render.push((line.to_string(), Style::default()));
                continue;
            }

            if trimmed.starts_with("Thought:") {
                if app.show_thoughts && app.verbose_mode {
                    lines_to_render.push((line.to_string(), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));
                }
                continue;
            }

            if trimmed.starts_with("Action:") {
                // Always show actions to provide feedback on what the agent is doing
                lines_to_render.push((line.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
                continue;
            }

            if trimmed.starts_with("Action Input:") {
                if !app.verbose_mode {
                    continue;
                }
                lines_to_render.push((line.to_string(), Style::default().fg(Color::DarkGray)));
                continue;
            }

            if !app.verbose_mode && (trimmed.starts_with("Observation:") || trimmed.contains("CMD_OUTPUT:")) {
                continue;
            }

            if trimmed.starts_with("Final Answer:") {
                let content = line.replace("Final Answer:", "");
                lines_to_render.push((content.trim().to_string(), Style::default()));
                continue;
            }

            lines_to_render.push((line.to_string(), Style::default()));
        }

        if has_hidden_context {
            lines_to_render.push(("[Context Attached]".to_string(), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));
        }

        if m.role == MessageRole::Assistant && lines_to_render.iter().all(|(l, _)| l.trim().is_empty()) {
            continue;
        }

        // Wrap and render the lines
        let content_width = available_width.saturating_sub(prefix.len());
        let mut first_line = true;

        for (text, style) in lines_to_render {
            if text.is_empty() && !first_line {
                list_items.push(ListItem::new(Line::from("")));
                continue;
            }
            
            let wrapped = wrap_text(&text, content_width);
            for line in wrapped {
                let mut spans = Vec::new();
                if first_line {
                    spans.push(Span::styled(prefix.to_string(), Style::default().fg(color).add_modifier(Modifier::BOLD)));
                    first_line = false;
                } else {
                    spans.push(Span::raw(" ".repeat(prefix.len())));
                }
                
                spans.push(Span::styled(line, style));
                list_items.push(ListItem::new(Line::from(spans)));
            }
        }
        // Add separator line
        list_items.push(ListItem::new(Line::from("")));
    }

    let mut chat_block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(if app.focus == Focus::Chat {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    if let Some(status) = &app.status_message {
        chat_block = chat_block.title_bottom(Line::from(vec![
            Span::styled(format!(" {} ", status), Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC))
        ]));
    } else if app.state != AppState::Idle {
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame = spinner[(app.tick_count % spinner.len() as u64) as usize];
        
        let (status_text, color) = match &app.state {
            AppState::Thinking(info) => (format!(" {} Thinking ({}) ", frame, info), Color::Yellow),
            AppState::Streaming(info) => (format!(" {} Streaming: {} ", frame, info), Color::Green),
            AppState::ExecutingTool(tool) => (format!(" {} Executing: {} ", frame, tool), Color::Cyan),
            AppState::WaitingForUser => (" ⏳ Waiting for Approval ".to_string(), Color::Magenta),
            AppState::Error(err) => (format!(" ❌ Error: {} ", err), Color::Red),
            AppState::Idle => unreachable!(),
        };
        chat_block = chat_block.title_bottom(Line::from(vec![
            Span::styled(status_text, Style::default().fg(color).add_modifier(Modifier::BOLD))
        ]));
    } else if !app.chat_auto_scroll {
        chat_block = chat_block.title_bottom(Line::from(vec![
            Span::styled(" [SCROLLING] ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD))
        ]));
    }

    // Scrolling logic
    let height = chunks[0].height.saturating_sub(2) as usize;
    let total_lines = list_items.len();
    
    let start_index = if app.chat_auto_scroll {
        total_lines.saturating_sub(height)
    } else {
        // Clamp scroll to valid range
        let max_scroll = total_lines.saturating_sub(height);
        app.chat_scroll = app.chat_scroll.clamp(0, max_scroll);
        max_scroll.saturating_sub(app.chat_scroll)
    };
    
    let end_index = (start_index + height).min(total_lines);
    let items_to_show = list_items.drain(start_index..end_index).collect::<Vec<_>>();

    let chat_list = List::new(items_to_show).block(chat_block);
    frame.render_widget(chat_list, chunks[0]);

    // Chat input
    let input_width = chunks[1].width.saturating_sub(2) as usize;
    let input_title = if app.focus == Focus::Chat {
        if app.state != AppState::Idle {
            " Input (Locked - Ctrl+c to stop) "
        } else {
            " Input (Home/End/Del support) "
        }
    } else {
        " Input "
    };

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(input_title)
        .border_style(if app.focus == Focus::Chat {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    if app.state != AppState::Idle && app.state != AppState::WaitingForUser {
        let p = Paragraph::new(Span::styled("(AI is active...)", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)))
            .block(input_block);
        frame.render_widget(p, chunks[1]);
    } else {
        // Horizontal scrolling logic for input
        let _char_count = app.chat_input.chars().count();
        
        // Auto-scroll input_scroll to keep cursor visible
        if app.cursor_position < app.input_scroll {
            app.input_scroll = app.cursor_position;
        } else if app.cursor_position >= app.input_scroll + input_width {
            app.input_scroll = app.cursor_position.saturating_sub(input_width).saturating_add(1);
        }

        let display_text: String = app.chat_input.chars()
            .skip(app.input_scroll)
            .take(input_width)
            .collect();

        let input_paragraph = Paragraph::new(display_text).block(input_block);
        frame.render_widget(input_paragraph, chunks[1]);

        if app.focus == Focus::Chat {
            frame.set_cursor_position((
                chunks[1].x + (app.cursor_position - app.input_scroll) as u16 + 1,
                chunks[1].y + 1,
            ));
        }
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = if width == 0 { 1 } else { width };
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        
        let mut current_line = String::new();
        for word in paragraph.split(' ') {
            if word.is_empty() && !current_line.is_empty() {
                if current_line.len() + 1 <= width {
                    current_line.push(' ');
                } else {
                    lines.push(current_line);
                    current_line = String::new();
                }
                continue;
            }

            if current_line.is_empty() {
                let mut w = word;
                while w.chars().count() > width {
                    let split_idx = w.char_indices().nth(width).map(|(i, _)| i).unwrap_or(w.len());
                    lines.push(w[..split_idx].to_string());
                    w = &w[split_idx..];
                }
                current_line = w.to_string();
            } else if current_line.len() + 1 + word.len() <= width {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                lines.push(current_line);
                let mut w = word;
                while w.chars().count() > width {
                    let split_idx = w.char_indices().nth(width).map(|(i, _)| i).unwrap_or(w.len());
                    lines.push(w[..split_idx].to_string());
                    w = &w[split_idx..];
                }
                current_line = w.to_string();
            }
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }
    }
    lines
}
