use crate::terminal::app::{App, Focus, AppState};
use crate::terminal::help::HelpSystem;
use mylm_core::llm::chat::MessageRole;
use mylm_core::agent::v2::jobs::JobStatus;
use std::sync::atomic::Ordering;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Modifier},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap, Clear},
    Frame,
};
use tui_term::widget::PseudoTerminal;

pub fn render(frame: &mut Frame, app: &mut App) {
    // Fixed heights: top bar (2 lines) + bottom bar (1 line)
    let top_bar_height = 2u16;
    let bottom_bar_height = 1u16;

    // Job panel height (fixed at 6 rows when visible, 0 when hidden)
    let job_panel_height = if app.show_jobs_panel { 6u16 } else { 0u16 };

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_bar_height),
            Constraint::Min(0),
            Constraint::Length(bottom_bar_height),
            Constraint::Length(job_panel_height),
        ])
        .split(frame.area());

    render_top_bar(frame, app, main_layout[0], top_bar_height);

    // Check if job detail view should be shown
    if app.show_job_detail {
        render_job_detail(frame, app, main_layout[1]);
    } else if app.show_memory_view {
        render_memory_view(frame, app, main_layout[1]);
    } else {
        // Dynamic layout based on terminal visibility and chat width
        // When chat width is 100% or terminal is hidden, chat takes full width
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

        if terminal_visible {
            if app.show_help_view {
                render_help_view(frame, app, chunks[0]);
            } else {
                render_terminal(frame, app, chunks[0]);
            }
        }
        // Chat is always rendered
        render_chat(frame, app, chunks[1]);
    }

    // Bottom bar with F-keys and toggles
    render_bottom_bar(frame, app, main_layout[2]);

    // Render job panel at bottom if visible
    if app.show_jobs_panel {
        render_jobs_panel(frame, app, main_layout[3]);
    }

    if app.state == AppState::ConfirmExit || app.state == AppState::NamingSession {
        render_confirm_exit(frame, app);
    }
}


fn render_memory_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[1]);

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Memory Nodes (Scroll: Up/Down) ")
        .border_style(Style::default().fg(Color::Yellow));

    let mut items = Vec::new();
    for node in &app.memory_graph.nodes {
        let title = node.memory.content.lines().next().unwrap_or("Empty Memory");
        let truncated_title = if title.len() > 50 {
            format!("{}...", &title[..47])
        } else {
            title.to_string()
        };
        
        let type_tag = format!("[{}] ", node.memory.r#type);
        items.push(ListItem::new(Line::from(vec![
            Span::styled(type_tag, Style::default().fg(Color::Cyan)),
            Span::raw(truncated_title),
        ])));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from("No related memories found.")));
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
        let idx = app.memory_graph_scroll.clamp(0, app.memory_graph.nodes.len().saturating_sub(1));
        let node = &app.memory_graph.nodes[idx];
        
        let mut detail_lines = Vec::new();
        detail_lines.push(Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Gray)),
            Span::raw(node.memory.id.to_string()),
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
        detail_lines.push(Line::from(Span::styled("Content:", Style::default().add_modifier(Modifier::UNDERLINED))));
        
        for line in node.memory.content.lines() {
            detail_lines.push(Line::from(line));
        }
        
        detail_lines.push(Line::from(""));
        detail_lines.push(Line::from(Span::styled("Connections:", Style::default().add_modifier(Modifier::UNDERLINED))));
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
        let p = Paragraph::new("Select a memory to see details.")
            .block(detail_block);
        frame.render_widget(p, right_chunks[0]);
    }

    // Render Scratchpad
    let scratchpad_content = app.scratchpad.read().unwrap_or_else(|e| e.into_inner()).clone();
    let scratchpad_block = Block::default()
        .borders(Borders::ALL)
        .title(" Scratchpad (Shared State) ")
        .border_style(Style::default().fg(Color::Magenta));
    
    let scratchpad_p = Paragraph::new(scratchpad_content)
        .block(scratchpad_block)
        .wrap(Wrap { trim: true });
    
    frame.render_widget(scratchpad_p, right_chunks[1]);
}

fn render_help_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let help_text = HelpSystem::generate_help_text(app, None);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" [ myLM Help (F1 to close, ↑/↓ to scroll) ] ")
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.help_scroll as u16, 0));

    frame.render_widget(paragraph, area);
}

/// Render help panel as a centered modal popup
#[allow(dead_code)]
pub fn render_help_panel(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let help_text = HelpSystem::generate_help_text(app, None);

    // Create a centered popup (80% width, 80% height)
    let popup_area = centered_rect(80, 80, area);

    // Clear the background
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" [ myLM Help (Press any key to close) ] ")
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, popup_area);
}

fn render_top_bar(frame: &mut Frame, app: &mut App, area: Rect, _height: u16) {
    let stats = app.session_monitor.get_stats();
    let auto_approve = app.auto_approve.load(Ordering::SeqCst);

    // Agent state indicator
    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let frame_char = spinner[(app.tick_count % spinner.len() as u64) as usize];

    let elapsed = app.state_started_at.elapsed();
    let elapsed_ms = elapsed.as_millis();
    let elapsed_text = if elapsed_ms >= 1000 {
        format!("{:.1}s", elapsed.as_secs_f64())
    } else {
        format!("{}ms", elapsed_ms)
    };

    let (state_label, state_color) = match &app.state {
        AppState::Idle => ("Ready".to_string(), Color::Green),
        AppState::Thinking(info) => (format!("Thinking ({})", info), Color::Yellow),
        AppState::Streaming(info) => (format!("Streaming ({})", info), Color::Green),
        AppState::ExecutingTool(tool) => (format!("Executing ({})", tool), Color::Cyan),
        AppState::WaitingForUser => ("Waiting".to_string(), Color::Magenta),
        AppState::Error(err) => (format!("Error ({})", err), Color::Red),
        AppState::ConfirmExit => ("Exit?".to_string(), Color::Yellow),
        AppState::NamingSession => ("Naming".to_string(), Color::Cyan),
    };
    let state_prefix = if app.state == AppState::Idle { "●" } else { frame_char };

    // Top row: version | toggles | F-keys | state
    let left_spans = vec![
        Span::styled(" mylm ", Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::styled(format!("v{} ", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::DarkGray)),
    ];

    // Center: toggles + F-keys
    let center_spans = vec![
        // Auto-Approve toggle
        Span::styled(
            if auto_approve { "[AUTO✓]" } else { "[AUTO✗]" },
            Style::default().fg(if auto_approve { Color::Green } else { Color::DarkGray }),
        ),
        Span::raw(" "),
        // PaCoRe toggle
        Span::styled(
            if app.pacore_enabled { format!("[PACORE:{}]", app.pacore_rounds) } else { "[PACORE✗]".to_string() },
            Style::default().fg(if app.pacore_enabled { Color::Green } else { Color::DarkGray }),
        ),
        Span::raw(" "),
        // F-keys
        Span::styled(
            "[F1:Help]",
            Style::default().fg(if app.show_help_view { Color::Green } else { Color::Yellow }),
        ),
        Span::styled("[F2:Focus]", Style::default().fg(Color::Yellow)),
        Span::styled(
            "[F3:Mem]",
            Style::default().fg(if app.show_memory_view { Color::Green } else { Color::Yellow }),
        ),
        Span::styled(
            "[F4:Jobs]",
            Style::default().fg(if app.show_jobs_panel { Color::Green } else { Color::Yellow }),
        ),
        Span::styled("[Esc:Hub]", Style::default().fg(Color::Red)),
    ];

    // Right side: state + elapsed
    let right_spans = vec![
        Span::styled(
            format!("{} {}", state_prefix, state_label),
            Style::default().fg(state_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" ({})", elapsed_text), Style::default().fg(Color::DarkGray)),
    ];

    let ratio = app.session_monitor.get_context_ratio();
    let gauge_color = if ratio >= 0.8 {
        Color::Red
    } else if ratio >= 0.5 {
        Color::Yellow
    } else {
        Color::Green
    };

    // Gauge label includes cost + context
    let label = format!("💰${:.2} | Ctx: {} / {} ({:.0}%)",
        stats.cost,
        stats.active_context_tokens,
        stats.max_context_tokens,
        (ratio * 100.0).clamp(0.0, 100.0)
    );

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status line with toggles/F-keys
            Constraint::Length(1), // Gauge with cost+context
        ])
        .split(area);

    // Split top row into left/center/right
    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(12), Constraint::Min(60), Constraint::Min(20)])
        .split(rows[0]);

    let left_text = Line::from(left_spans);
    let center_text = Line::from(center_spans).alignment(ratatui::layout::Alignment::Center);
    let right_text = Line::from(right_spans).alignment(ratatui::layout::Alignment::Right);

    let gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::default().fg(gauge_color).bg(Color::DarkGray))
        .ratio(ratio.clamp(0.0, 1.0))
        .label(label);

    frame.render_widget(Paragraph::new(left_text), top_chunks[0]);
    frame.render_widget(Paragraph::new(center_text), top_chunks[1]);
    frame.render_widget(Paragraph::new(right_text), top_chunks[2]);
    frame.render_widget(gauge, rows[1]);
}

/// Render bottom bar - now empty since everything moved to top
fn render_bottom_bar(_frame: &mut Frame, _app: &mut App, _area: Rect) {
    // All controls moved to top bar
}

fn render_terminal(frame: &mut Frame, app: &mut App, area: Rect) {
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
    // Clamp scroll to valid bounds
    app.terminal_scroll = app.terminal_scroll.clamp(0, max_scroll);
    
    let start_idx = total_lines.saturating_sub(app.terminal_scroll).saturating_sub(height);
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
    // Store the offset for mouse coordinate translation
    app.chat_area_offset = Some((area.x, area.y));
    
    let input_width = area.width.saturating_sub(2) as usize;
    let input_content = if app.state != AppState::Idle && app.state != AppState::WaitingForUser {
        "(AI is active...)".to_string()
    } else {
        app.chat_input.clone()
    };

    // Calculate dynamic input height (up to 3 rows of text + 2 for borders)
    let wrapped_input = wrap_text(&input_content, input_width);
    let input_lines = wrapped_input.len().clamp(1, 3) as u16;
    let input_height = input_lines + 2;
    
    // Check if we need to show PaCoRe progress bar
    let show_progress = app.pacore_progress.is_some();
    let progress_height = if show_progress { 3u16 } else { 0u16 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3), 
            Constraint::Length(progress_height),
            Constraint::Length(input_height)
        ])
        .split(area);
    
    // Render PaCoRe progress bar if active
    if show_progress {
        if let Some((completed, total)) = app.pacore_progress {
            let ratio = if total > 0 { completed as f64 / total as f64 } else { 0.0 };
            let _percent = (ratio * 100.0) as u16;
            
            let (current_round, total_rounds) = app.pacore_current_round.unwrap_or((1, 1));
            
            // Create progress bar with custom styling
            let filled = (ratio * 20.0) as usize;
            let empty = 20 - filled;
            let bar_str = format!(
                "[{}{}] {}/{} calls (Round {}/{})",
                "█".repeat(filled),
                "░".repeat(empty),
                completed,
                total,
                current_round,
                total_rounds
            );
            
            let progress_widget = Paragraph::new(bar_str)
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .alignment(ratatui::layout::Alignment::Center);
            
            frame.render_widget(progress_widget, chunks[1]);
        }
    }

    let title = match app.focus {
        Focus::Chat => " AI Chat (F2) [Ctrl+Y: Copy AI] ",
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
        
        // Hide Context Packs (Terminal Snapshot, etc.)
        let delimiter = "\n\n## Terminal Snapshot";
        let (raw_display_content, has_hidden_context) = if let Some(idx) = m.content.find(delimiter) {
            (&m.content[..idx], true)
        } else {
            (m.content.as_str(), false)
        };

        // Try to parse entire content as JSON first (handles multi-line JSON)
        let processed_content = if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw_display_content.trim()) {
            let mut parts = Vec::new();
            
            if let Some(t) = val.get("t").and_then(|v| v.as_str()) {
                if !t.is_empty() {
                    parts.push(format!("Thought: {}", t));
                }
            }
            
            if let Some(a) = val.get("a").and_then(|v| v.as_str()) {
                let i = val.get("i").map(|v| v.to_string()).unwrap_or_default();
                parts.push(format!("Action: {} ({})", a, i));
            }
            
            if let Some(f) = val.get("f").and_then(|v| v.as_str()) {
                parts.push(f.to_string());
            }

            if parts.is_empty() {
                raw_display_content.to_string()
            } else {
                parts.join("\n")
            }
        } else {
            raw_display_content.to_string()
        };

        let raw_lines: Vec<&str> = processed_content.split('\n').collect();
        
        for raw_line in raw_lines {
            let line = raw_line.replace('\r', "");
            let trimmed = line.trim();
            if trimmed.is_empty() {
                lines_to_render.push((line, Style::default()));
                continue;
            }

            let is_thought = trimmed.starts_with("Thought:") || trimmed.starts_with("**Thought:**");
            if is_thought {
                if app.show_thoughts && app.verbose_mode {
                    lines_to_render.push((line, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));
                }
                continue;
            }

            // Hide JSON blocks from display
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                // Check if it's a JSON decision block
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    let has_thought = val.get("t").is_some();
                    let has_action = val.get("a").is_some();
                    let has_final = val.get("f").is_some();

                    if has_thought || has_action || has_final {
                        if app.show_thoughts && has_thought && app.verbose_mode {
                            if let Some(t) = val.get("t").and_then(|v| v.as_str()) {
                                lines_to_render.push((format!("Thought: {}", t), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));
                            }
                        }
                        if has_action && app.verbose_mode {
                            if let Some(a) = val.get("a").and_then(|v| v.as_str()) {
                                let i = val.get("i").map(|v| v.to_string()).unwrap_or_default();
                                lines_to_render.push((format!("Action: {} ({})", a, i), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
                            }
                        }
                        // Final answer is usually followed by the same text outside JSON if the model is being redundant,
                        // but if it's ONLY in JSON, we should show it.
                        // However, usually we prefer to let the rest of the loop handle it.
                        continue;
                    }
                }
            }

            let is_action = trimmed.starts_with("Action:") || trimmed.starts_with("**Action:**");
            if is_action {
                // Always show actions to provide feedback on what the agent is doing
                lines_to_render.push((line, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
                continue;
            }

            let is_action_input = trimmed.starts_with("Action Input:") || trimmed.starts_with("**Action Input:**");
            if is_action_input {
                if !app.verbose_mode {
                    continue;
                }
                lines_to_render.push((line, Style::default().fg(Color::DarkGray)));
                continue;
            }

            let is_observation = trimmed.starts_with("Observation:") || trimmed.starts_with("**Observation:**");
            if !app.verbose_mode && (is_observation || trimmed.contains("CMD_OUTPUT:")) {
                continue;
            }

            let is_final_answer = trimmed.starts_with("Final Answer:") || trimmed.starts_with("**Final Answer:**");
            if is_final_answer {
                let content = line.replace("Final Answer:", "").replace("**Final Answer:**", "");
                lines_to_render.push((content.trim().to_string(), Style::default()));
                continue;
            }

            lines_to_render.push((line, Style::default()));
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
            AppState::ConfirmExit => (" ⚠️  Confirm Exit? ".to_string(), Color::Yellow),
            AppState::NamingSession => (" 💾 Name Session ".to_string(), Color::Cyan),
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

    // Smart Scrolling logic
    let height = chunks[0].height.saturating_sub(2) as usize;
    let total_lines = list_items.len();

    // If content grew and we're not auto-scrolling, increment scroll to stay fixed
    if let Some(last) = app.last_total_chat_lines {
        if total_lines > last && !app.chat_auto_scroll {
            let diff = total_lines - last;
            app.chat_scroll = app.chat_scroll.saturating_add(diff);
        }
    }
    app.last_total_chat_lines = Some(total_lines);
    
    // Calculate max scroll based on current content
    let max_scroll = total_lines.saturating_sub(height);
    
    // Always clamp scroll to valid bounds first
    app.chat_scroll = app.chat_scroll.clamp(0, max_scroll);
    
    let start_index = if app.chat_auto_scroll {
        total_lines.saturating_sub(height)
    } else {
        max_scroll.saturating_sub(app.chat_scroll)
    };
    
    let end_index = (start_index + height).min(total_lines);
    let items_to_show = list_items.drain(start_index..end_index).collect::<Vec<_>>();

    let chat_list = List::new(items_to_show).block(chat_block);
    frame.render_widget(chat_list, chunks[0]);

    // Chat input
    let input_title = if app.focus == Focus::Chat {
        if app.state != AppState::Idle && app.state != AppState::WaitingForUser {
            " Input (Locked - Ctrl+c to stop) "
        } else {
            " Input (Home/End/Del/Arrows) [Esc: Exit] "
        }
    } else {
        " Input (F2 to focus for Esc/Commands) "
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
        let p = Paragraph::new(Span::styled(&input_content, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)))
            .block(input_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(p, chunks[2]);
    } else {
        // Calculate cursor position in wrapped text
        let (cursor_x, cursor_y) = calculate_input_cursor_pos(&app.chat_input, app.cursor_position, input_width);

        // Calculate which lines of the input to show (vertical scrolling window)
        let total_input_lines = wrapped_input.len();
        let max_visible_lines = input_lines as usize;
        
        let start_line = if total_input_lines <= max_visible_lines {
            0
        } else {
            // Keep cursor in view by adjusting the window
            if (cursor_y as usize) < max_visible_lines {
                0
            } else {
                (cursor_y as usize).saturating_sub(max_visible_lines - 1)
            }
        };

        let end_line = (start_line + max_visible_lines).min(total_input_lines);
        let visible_lines = &wrapped_input[start_line..end_line];
        
        // Ensure we always have at least one line to avoid Paragraph panic or weirdness
        let display_content = if visible_lines.is_empty() {
            String::new()
        } else {
            visible_lines.join("\n")
        };

        let input_paragraph = Paragraph::new(display_content)
            .block(input_block);
        frame.render_widget(input_paragraph, chunks[2]);

        if app.focus == Focus::Chat {
            let visible_cursor_y = cursor_y.saturating_sub(start_line as u16);

            frame.set_cursor_position((
                chunks[2].x + cursor_x + 1,
                chunks[2].y + visible_cursor_y + 1,
            ));
        }
    }
}

pub fn calculate_input_cursor_pos(text: &str, cursor_idx: usize, width: usize) -> (u16, u16) {
    if width == 0 { return (0, 0); }
    
    let prefix: String = text.chars().take(cursor_idx).collect();
    let wrapped = wrap_text(&prefix, width);
    
    if wrapped.is_empty() {
        return (0, 0);
    }
    
    let row = wrapped.len().saturating_sub(1);
    let col = wrapped.last().map(|l| l.chars().count()).unwrap_or(0);
    
    (col as u16, row as u16)
}

pub fn find_idx_from_coords(text: &str, target_x: u16, target_y: u16, width: usize) -> usize {
    let mut best_idx = 0;
    let mut min_dist = u32::MAX;
    
    let char_count = text.chars().count();
    for i in 0..=char_count {
        let (x, y) = calculate_input_cursor_pos(text, i, width);
        if y == target_y {
            let dist = (x as i32 - target_x as i32).unsigned_abs();
            if dist <= min_dist {
                min_dist = dist;
                best_idx = i;
            }
        } else if y < target_y {
             best_idx = i;
        }
    }
    best_idx
}

pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = if width == 0 { 1 } else { width };
    let mut lines = Vec::new();
    
    // split('\n') returns at least one element even for empty string
    let paragraphs: Vec<&str> = text.split('\n').collect();
    
    for paragraph in paragraphs.iter() {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        let mut current_width = 0;

        let mut chars = paragraph.chars().peekable();
        while let Some(c) = chars.next() {
            if c == ' ' {
                if current_width < width {
                    current_line.push(' ');
                    current_width += 1;
                } else {
                    lines.push(current_line);
                    current_line = String::new();
                    current_width = 0;
                }
            } else {
                let mut word = String::from(c);
                while let Some(&nc) = chars.peek() {
                    if nc == ' ' { break; }
                    word.push(chars.next().unwrap());
                }
                
                let word_len = word.chars().count();
                if current_width + word_len <= width {
                    current_line.push_str(&word);
                    current_width += word_len;
                } else {
                    if !current_line.is_empty() {
                        lines.push(current_line);
                        current_line = String::new();
                        current_width = 0;
                    }
                    
                    let mut remaining = word;
                    while !remaining.is_empty() {
                        let r_len = remaining.chars().count();
                        if r_len <= width {
                            current_line = remaining;
                            current_width = r_len;
                            remaining = String::new();
                        } else {
                            let split_idx = remaining.char_indices().nth(width).map(|(i, _)| i).unwrap_or(remaining.len());
                            lines.push(remaining[..split_idx].to_string());
                            remaining = remaining[split_idx..].to_string();
                        }
                    }
                }
            }
        }
        lines.push(current_line);
    }
    lines
}

fn render_confirm_exit(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let block = Block::default()
        .title(" ⚠️  Exit Confirmation ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(Color::Black));

    // Center the dialog
    let dialog_area = centered_rect(60, 40, area);
    frame.render_widget(ratatui::widgets::Clear, dialog_area); // Clear the area behind the dialog
    frame.render_widget(block, dialog_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(1), // Question
            Constraint::Length(1), // Empty
            Constraint::Length(3), // Input field for name
            Constraint::Min(0),    // Instructions
        ])
        .split(dialog_area);

    let question = Paragraph::new(Line::from(vec![
        Span::raw("Are you sure you want to exit the current session?"),
    ])).alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(question, chunks[0]);

    let input_block = Block::default()
        .title(" Session Name (Optional) ")
        .borders(Borders::ALL)
        .border_style(if app.state == AppState::NamingSession {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        });
    
    let name_text = if app.exit_name_input.is_empty() {
        Span::styled(" (session_YYYYMMDD_HHMMSS) ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
    } else {
        Span::raw(&app.exit_name_input)
    };
    
    let name_p = Paragraph::new(name_text).block(input_block);
    frame.render_widget(name_p, chunks[2]);

    let mut instructions_lines = vec![Line::from("")];

    if app.state == AppState::ConfirmExit {
        instructions_lines.push(Line::from(vec![
            Span::styled(" [S] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Save & Exit"),
            Span::raw("  "),
            Span::styled(" [E] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw("Exit (Discard)"),
            Span::raw("  "),
            Span::styled(" [C] ", Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD)),
            Span::raw("Cancel"),
        ]));
    } else {
        instructions_lines.push(Line::from(vec![
            Span::styled(" Enter ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Confirm Name & Save"),
            Span::raw("  "),
            Span::styled(" Esc ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Back"),
        ]));
    }

    let instructions = Paragraph::new(instructions_lines).alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(instructions, chunks[3]);

    if app.state == AppState::NamingSession {
        // Set cursor in the name input field
        frame.set_cursor_position((
            chunks[2].x + 1 + app.exit_name_input.chars().count() as u16,
            chunks[2].y + 1,
        ));
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

fn render_jobs_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    // Get all jobs from the registry (including completed and failed)
    let jobs = app.job_registry.list_all_jobs();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Background Jobs (F4: toggle, ↑↓: navigate, Enter: details, q/Esc: close) ")
        .border_style(if !jobs.is_empty() {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        });

    if jobs.is_empty() {
        let empty_text = Paragraph::new("No background jobs")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty_text, area);
        return;
    }

    let mut items = Vec::new();
    for (idx, job) in jobs.iter().enumerate() {
        let short_id = &job.id[..8.min(job.id.len())];
        let status_str = match job.status {
            JobStatus::Running => "● Running",
            JobStatus::Completed => "✓ Done",
            JobStatus::Failed => "✗ Failed",
        };
        let status_color = match job.status {
            JobStatus::Running => Color::Yellow,
            JobStatus::Completed => Color::Green,
            JobStatus::Failed => Color::Red,
        };

        // Truncate description to fit (accounting for status text)
        let max_desc_len = area.width.saturating_sub(30) as usize;
        let desc = if job.description.len() > max_desc_len {
            format!("{}...", &job.description[..max_desc_len.saturating_sub(3)])
        } else {
            job.description.clone()
        };

        let is_selected = app.selected_job_index == Some(idx);
        let prefix = if is_selected { "► " } else { "  " };

        let line = Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("#{}", short_id), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(status_str, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::raw(desc),
        ]);

        items.push(ListItem::new(line));
    }

    let list = List::new(items)
        .block(block);

    frame.render_widget(list, area);
}

fn render_job_detail(frame: &mut Frame, app: &mut App, area: Rect) {
    // Clear the area and render a popup-like view
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Job Details (Esc/q: close, ↑↓: scroll) ")
        .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Get all jobs (including completed and failed) and find the selected one
    let jobs = app.job_registry.list_all_jobs();
    let content = if let Some(selected_idx) = app.selected_job_index {
        if let Some(job) = jobs.get(selected_idx) {
            let mut lines = Vec::new();

            // Job ID
            lines.push(Line::from(vec![
                Span::styled("Job ID: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(&job.id),
            ]));

            // Tool name
            lines.push(Line::from(vec![
                Span::styled("Tool: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(&job.tool_name),
            ]));

            // Status
            let (status_text, status_color) = match job.status {
                JobStatus::Running => ("Running", Color::Yellow),
                JobStatus::Completed => ("Completed", Color::Green),
                JobStatus::Failed => ("Failed", Color::Red),
            };
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
            ]));

            // Started at
            lines.push(Line::from(vec![
                Span::styled("Started: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(job.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
            ]));

            // Finished at (if applicable)
            if let Some(finished) = job.finished_at {
                lines.push(Line::from(vec![
                    Span::styled("Finished: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(finished.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
                ]));
            }

            lines.push(Line::from(""));

            // Description
            lines.push(Line::from(vec![
                Span::styled("Description:", Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED)),
            ]));
            for line in job.description.lines() {
                lines.push(Line::from(line));
            }

            // Output
            if !job.output.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Output:", Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED)),
                ]));
                for line in job.output.lines() {
                    lines.push(Line::from(line));
                }
            }

            // Error
            if let Some(ref error) = job.error {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Error:", Style::default().fg(Color::Red).add_modifier(Modifier::UNDERLINED)),
                ]));
                for line in error.lines() {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::Red))));
                }
            }

            // Result
            if let Some(ref result) = job.result {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Result:", Style::default().fg(Color::Green).add_modifier(Modifier::UNDERLINED)),
                ]));
                let result_str = serde_json::to_string_pretty(result).unwrap_or_default();
                let result_lines: Vec<String> = result_str.lines().map(|s| s.to_string()).collect();
                for line in result_lines {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::Green))));
                }
            }

            lines
        } else {
            vec![Line::from("No job selected")]
        }
    } else {
        vec![Line::from("No job selected")]
    };

    // Apply scrolling
    let visible_height = inner_area.height as usize;
    let total_lines = content.len();
    let max_scroll = total_lines.saturating_sub(visible_height);
    app.job_scroll = app.job_scroll.min(max_scroll);

    let start_idx = app.job_scroll;
    let end_idx = (start_idx + visible_height).min(total_lines);
    let visible_content: Vec<Line> = content[start_idx..end_idx].to_vec();

    let paragraph = Paragraph::new(visible_content)
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, inner_area);
}
