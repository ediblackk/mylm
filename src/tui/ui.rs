// Use real types from TUI modules
use crate::tui::app::state::AppStateContainer as App;
use crate::tui::types::{AppState, Focus, JobStatus, ActionType};
use crate::tui::help::HelpSystem;
use mylm_core::llm::chat::MessageRole;
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

    render_top_bar(frame, app, main_layout[0], top_bar_height);

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
        render_job_detail(frame, app, chunks[0]);
        render_chat(frame, app, chunks[1]);
    } else if app.show_job_detail {
        // Terminal hidden - use full width for job detail
        render_job_detail(frame, app, main_layout[1]);
    } else if app.show_memory_view {
        render_memory_view(frame, app, main_layout[1]);
    } else {
        // Normal layout
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

    if app.state == AppState::ConfirmExit {
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
        let timestamp_str = format_timestamp(node.memory.created_at);
        
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
            format!("No memories match filter: '{}' (press Esc to clear)", app.memory_search_query)
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
        let idx = app.memory_graph_scroll.clamp(0, app.memory_graph.nodes.len().saturating_sub(1));
        let node = &app.memory_graph.nodes[idx];
        
        let mut detail_lines = Vec::new();
        detail_lines.push(Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Gray)),
            Span::raw(node.memory.id.to_string()),
        ]));
        detail_lines.push(Line::from(vec![
            Span::styled("Time: ", Style::default().fg(Color::Gray)),
            Span::raw(format_timestamp_full(node.memory.created_at)),
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
        
        // Clean up content - unescape JSON newlines and tabs for display
        let cleaned_content = node.memory.content
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"");
        
        for line in cleaned_content.lines() {
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
/// Reserved for future help system UI (currently unused)
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
    
    // Get status info from tracker
    let status_info = app.status_tracker.current();
    
    // Get elapsed time - use tool elapsed if executing, otherwise state elapsed
    let elapsed = match status_info {
        crate::tui::status_tracker::StatusInfo::Executing { .. } => {
            app.status_tracker.tool_elapsed().unwrap_or_else(|| app.state_started_at.elapsed())
        }
        _ => app.state_started_at.elapsed(),
    };
    
    let elapsed_text = format_elapsed(elapsed);

    // Build status label and color based on status tracker state (static, no animation)
    let (state_label, state_color, is_active) = match status_info {
        crate::tui::status_tracker::StatusInfo::Error { message } => {
            let msg = if message.len() > 35 {
                format!("{}...", &message[..35])
            } else {
                message.clone()
            };
            (format!("⚠ Error: {}", msg), Color::Red, false)
        }
        crate::tui::status_tracker::StatusInfo::Executing { tool, args } => {
            let args_preview = if args.len() > 25 {
                format!("{}...", &args[..25])
            } else if args.is_empty() {
                "".to_string()
            } else {
                format!(" {}", args)
            };
            (format!("⚡ {}{}", tool, args_preview), Color::Cyan, true)
        }
        crate::tui::status_tracker::StatusInfo::Thinking => {
            ("💭 Thinking...".to_string(), Color::Yellow, true)
        }
        crate::tui::status_tracker::StatusInfo::AwaitingApproval { tool, .. } => {
            (format!("⏸ Approve {}? (y/n)", tool), Color::Magenta, true)
        }
        crate::tui::status_tracker::StatusInfo::Idle => {
            match &app.state {
                AppState::Idle => ("✓ Ready".to_string(), Color::Green, false),
                AppState::Thinking(info) => (format!("💭 {}", info), Color::Yellow, true),
                AppState::Streaming(info) => (format!("📡 {}", info), Color::Cyan, true),
                AppState::ExecutingTool(tool) => (format!("⚡ {}", tool), Color::Cyan, true),
                AppState::WaitingForUser => ("⏸ Waiting".to_string(), Color::Magenta, false),
                AppState::AwaitingApproval { tool, .. } => (format!("⏸ Approve {}? (y/n)", tool), Color::Magenta, true),
                AppState::Error(err) => (format!("⚠ {}", err), Color::Red, false),
                AppState::ConfirmExit => ("❓ Exit? (y/n)".to_string(), Color::Yellow, false),
                AppState::NamingSession => ("✎ Naming...".to_string(), Color::Cyan, true),
            }
        }
    };
    
    // Static indicator (no animation in top bar)
    let state_prefix = if is_active { "◐" } else { "●" };

    // Top row: version | toggles | F-keys | state
    let left_spans = vec![
        Span::styled(" mylm ", Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::styled(format!("v{} ", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::DarkGray)),
    ];

    // Center: toggles + F-keys
    let center_spans = vec![
        // Auto-Approve toggle
        Span::styled(
            if auto_approve { "[A✓]" } else { "[A✗]" },
            Style::default().fg(if auto_approve { Color::Green } else { Color::DarkGray }),
        ),
        Span::raw(" "),
        // PaCoRe toggle
        Span::styled(
            if app.pacore_enabled { format!("[P:{}]", app.pacore_rounds) } else { "[P:off]".to_string() },
            Style::default().fg(if app.pacore_enabled { Color::Green } else { Color::DarkGray }),
        ),
        Span::raw(" "),
        // F-keys (compact)
        Span::styled("[F1:?]", Style::default().fg(if app.show_help_view { Color::Green } else { Color::Yellow })),
        Span::styled("[F2:⇄]", Style::default().fg(Color::Yellow)),
        Span::styled("[F3:M]", Style::default().fg(if app.show_memory_view { Color::Green } else { Color::Yellow })),
        Span::styled("[F4:J]", Style::default().fg(if app.show_jobs_panel { Color::Green } else { Color::Yellow })),
        Span::styled("[Esc:◀]", Style::default().fg(Color::Red)),
    ];

    // Right side: animated spinner + state + elapsed
    let right_spans = vec![
        Span::styled(
            format!("{} ", state_prefix),
            Style::default().fg(state_color),
        ),
        Span::styled(
            state_label,
            Style::default().fg(state_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {}", elapsed_text), Style::default().fg(Color::DarkGray)),
    ];

    // Context usage gauge with color based on usage
    let ratio = app.session_monitor.get_context_ratio();
    let gauge_color = if ratio >= 0.9 {
        Color::Red
    } else if ratio >= 0.7 {
        Color::Yellow
    } else {
        Color::Green
    };

    // Gauge label with cost and context
    let label = format!("${:.2} │ CTX:{}/{} {:.0}%",
        stats.cost,
        format_tokens(stats.active_context_tokens),
        format_tokens(stats.max_context_tokens),
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
        .constraints([Constraint::Min(12), Constraint::Min(50), Constraint::Min(40)])
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

/// Format elapsed time in human-readable form
fn format_elapsed(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    
    if secs >= 60 {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{}.{:01}s", secs, millis / 100)
    } else {
        format!("{}ms", millis)
    }
}

/// Format token count with K/M suffix
fn format_tokens(tokens: u32) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
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
    
    // If we're auto-scrolling AND no selection is active, use the efficient PseudoTerminal widget from tui-term
    if app.terminal_auto_scroll && !app.is_selecting && app.selection_start.is_none() {
        let terminal = PseudoTerminal::new(screen)
            .block(block);
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


fn render_chat(frame: &mut Frame, app: &mut App, area: Rect) {
    // Clear chat_input_area at the start to avoid stale data
    app.chat_input_area = None;
    
    // Store the offset for mouse coordinate translation
    app.chat_area_offset = Some((area.x, area.y));
    
    // Clear visual lines mapping at the start of rendering
    app.chat_visual_lines.clear();
    
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
    
    // Set chat_input_area after layout is computed for mouse detection
    app.chat_input_area = Some(chunks[2]);
    // Store the starting column for chat history (after layout is determined)
    app.chat_history_start_col = Some(chunks[0].x + 1);
    
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
    
    // First pass: build all visual lines data and fill chat_visual_lines
    #[derive(Clone)]
    struct VisualLineInfo {
        full_text: String,
        prefix_len: usize,
        prefix_style: Style,
        content_style: Style,
    }
    
    let mut all_visual_lines: Vec<VisualLineInfo> = Vec::new();
    let mut abs_line_idx: usize = 0;
    
    for msg_meta in &app.chat_history {
        let m = &msg_meta.message;
        // Aggressively hide command outputs in non-verbose mode
        if !app.verbose_mode && m.content.contains("CMD_OUTPUT:") {
            if m.role == MessageRole::Tool || (m.role == MessageRole::User && m.content.contains("Observation:")) {
                // Placeholder line: "AI: Command executed. Check terminal."
                let prefix = "AI: ";
                let prefix_len = prefix.len();
                let prefix_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
                let content = "Command executed. Check terminal.";
                let full_text = format!("{}{}", prefix, content);
                all_visual_lines.push(VisualLineInfo { full_text: full_text.clone(), prefix_len, prefix_style, content_style: Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC) });
                app.chat_visual_lines.push((full_text, abs_line_idx));
                abs_line_idx += 1;
                // Separator line (empty)
                all_visual_lines.push(VisualLineInfo { full_text: String::new(), prefix_len: 0, prefix_style: Style::default(), content_style: Style::default() });
                app.chat_visual_lines.push((String::new(), abs_line_idx));
                abs_line_idx += 1;
            }
            continue;
        }

        // Skip Tool messages for commands in non-verbose mode
        if !app.verbose_mode && m.role == MessageRole::Tool && m.name.as_deref() == Some("execute_command") {
            continue;
        }

        // Build prefix (just role, no timestamp - timestamp shown at bottom)
        let timestamp_str = msg_meta.formatted_time();
        // Format generation time with minimum 0.1s (never show 0.0)
        let gen_time_str = msg_meta.generation_time_ms.map(|ms| {
            let seconds = (ms as f64 / 1000.0).max(0.1);
            format!("{:.1}s", seconds)
        });
        
        let (role_prefix, color) = match m.role {
            MessageRole::User => ("You: ", Color::Cyan),
            MessageRole::Assistant => ("AI: ", Color::Green),
            MessageRole::System => ("Sys: ", Color::Gray),
            _ => ("AI: ", Color::Green),
        };
        
        // Prefix is just the role (timestamp shown at bottom of message)
        let prefix = "";
        
        // Role prefix gets colored styling (You: cyan, AI: green)
        let prefix_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
        let prefix_len = prefix.len() + role_prefix.len();

        let mut lines_to_render = Vec::new();
        
        // Hide Context Packs (Terminal Snapshot, etc.)
        let delimiter = "\n\n## Terminal Snapshot";
        let raw_display_content = if let Some(idx) = m.content.find(delimiter) {
            &m.content[..idx]
        } else {
            m.content.as_str()
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

            let is_thought = trimmed.starts_with("Thought:") || trimmed.starts_with("**Thought:**") || trimmed.starts_with("💭");
            if is_thought {
                // Always show thoughts (streaming), but style softly
                let thought_style = Style::default().fg(Color::Rgb(128, 128, 128)).add_modifier(Modifier::ITALIC);
                if app.show_thoughts {
                    lines_to_render.push((line, thought_style));
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
                        continue;
                    }
                }
            }

            let is_action = trimmed.starts_with("Action:") || trimmed.starts_with("**Action:**");
            if is_action {
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

        // Add timestamp at bottom for all messages, with generation time for AI
        let bottom_text = if m.role == MessageRole::Assistant {
            if let Some(ref gen_time) = gen_time_str {
                format!("[{}] took {}", timestamp_str, gen_time)
            } else {
                format!("[{}]", timestamp_str)
            }
        } else {
            format!("[{}]", timestamp_str)
        };
        // Right-align the timestamp
        let padding = available_width.saturating_sub(prefix_len).saturating_sub(bottom_text.len());
        let padded_bottom = format!("{}{}", " ".repeat(padding), bottom_text);
        lines_to_render.push((padded_bottom, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));

        if m.role == MessageRole::Assistant && lines_to_render.iter().all(|(l, _)| l.trim().is_empty()) {
            continue;
        }

        // Process lines for visual representation
        let content_width = available_width.saturating_sub(prefix.len());
        let mut first_line_flag = true;
        for (text, style) in lines_to_render {
            if text.is_empty() && !first_line_flag {
                // Empty line (soft wrap break) - no prefix
                all_visual_lines.push(VisualLineInfo { full_text: String::new(), prefix_len: 0, prefix_style: Style::default(), content_style: Style::default() });
                app.chat_visual_lines.push((String::new(), abs_line_idx));
                abs_line_idx += 1;
                continue;
            }
            
            let wrapped = wrap_text(&text, content_width);
            for (wrapped_idx, line_str) in wrapped.iter().enumerate() {
                let is_first = first_line_flag && wrapped_idx == 0;
                let full_text = if is_first {
                    // First line: [timestamp] [role]: content
                    format!("{}{}{}", prefix, role_prefix, line_str)
                } else {
                    // Continuation: indent to align with content
                    format!("{}{}", " ".repeat(prefix_len), line_str)
                };
                let current_prefix_style = if is_first { prefix_style } else { Style::default() };
                all_visual_lines.push(VisualLineInfo { 
                    full_text: full_text.clone(), 
                    prefix_len: if is_first { prefix.len() } else { prefix_len }, 
                    prefix_style: current_prefix_style, 
                    content_style: style 
                });
                app.chat_visual_lines.push((full_text, abs_line_idx));
                abs_line_idx += 1;
            }
            first_line_flag = false;
        }
        // Add separator line (empty)
        all_visual_lines.push(VisualLineInfo { full_text: String::new(), prefix_len: 0, prefix_style: Style::default(), content_style: Style::default() });
        app.chat_visual_lines.push((String::new(), abs_line_idx));
        abs_line_idx += 1;
    }

    // Add action stamps as conversation items at the end
    let recent_stamps = app.context_manager.recent_stamps(10);
    if !recent_stamps.is_empty() {
        use mylm_core::context::action_stamp::ActionStampType;
        
        // Add a small header for stamps section
        all_visual_lines.push(VisualLineInfo { 
            full_text: "── Action Stamps ──".to_string(), 
            prefix_len: 0, 
            prefix_style: Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC), 
            content_style: Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC) 
        });
        app.chat_visual_lines.push(("── Action Stamps ──".to_string(), abs_line_idx));
        abs_line_idx += 1;
        
        // Render each stamp as a line item
        for stamp in recent_stamps.iter().rev().take(5) {
            let color = match stamp.stamp_type {
                ActionStampType::ToolSuccess => Color::Green,
                ActionStampType::ToolFailed => Color::Red,
                ActionStampType::ContextCondensed => Color::Yellow,
                ActionStampType::MemoryRecalled => Color::Magenta,
                ActionStampType::FileRead => Color::Cyan,
                ActionStampType::FileWritten => Color::Blue,
                ActionStampType::CommandExecuted => Color::Yellow,
                ActionStampType::WebSearch => Color::Cyan,
                ActionStampType::Thinking => Color::DarkGray,
                ActionStampType::TaskComplete => Color::Green,
            };
            
            let icon = stamp.stamp_type.icon();
            let mut stamp_text = format!("[{} {}]", icon, stamp.title);
            
            // Add detail if present
            if let Some(ref detail) = stamp.detail {
                if !detail.is_empty() {
                    stamp_text.push_str(&format!(" - {}", detail));
                }
            }
            
            // Wrap text if needed
            let content_width = available_width.saturating_sub(4);
            let wrapped = wrap_text(&stamp_text, content_width);
            
            for (idx, line) in wrapped.iter().enumerate() {
                let prefix = if idx == 0 { "  " } else { "    " };
                let full_text = format!("{}{}", prefix, line);
                all_visual_lines.push(VisualLineInfo { 
                    full_text: full_text.clone(), 
                    prefix_len: prefix.len(), 
                    prefix_style: Style::default(), 
                    content_style: Style::default().fg(color) 
                });
                app.chat_visual_lines.push((full_text, abs_line_idx));
                abs_line_idx += 1;
            }
        }
        
        // Add separator after stamps
        all_visual_lines.push(VisualLineInfo { full_text: String::new(), prefix_len: 0, prefix_style: Style::default(), content_style: Style::default() });
        app.chat_visual_lines.push((String::new(), abs_line_idx));
    }

    let total_lines = all_visual_lines.len();
    
    // Smart Scrolling logic (adjust scroll if content grew)
    let height = chunks[0].height.saturating_sub(2) as usize;
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
    
    // Track visible range for selection extraction
    app.chat_visible_start_idx = start_index;
    app.chat_visible_end_idx = end_index;
    
    // Build list_items for visible lines only, with correct row calculation and selection
    let mut list_items = Vec::new();
    for (abs_line_idx, visual_line) in all_visual_lines.iter().enumerate().skip(start_index).take(end_index - start_index) {
        let current_row = chunks[0].y + 1 + (abs_line_idx as u16 - start_index as u16);
        let full_text = &visual_line.full_text;
        if full_text.is_empty() {
            list_items.push(ListItem::new(Line::from("")));
            continue;
        }
        let mut spans = Vec::new();
        for (char_idx, c) in full_text.chars().enumerate() {
            let col = chunks[0].x + 1 + char_idx as u16;
            let is_selected = app.is_in_selection(col, current_row, Focus::Chat);
            let style = if is_selected {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            } else if char_idx < visual_line.prefix_len {
                visual_line.prefix_style
            } else {
                visual_line.content_style
            };
            spans.push(Span::styled(c.to_string(), style));
        }
        list_items.push(ListItem::new(Line::from(spans)));
    }

    // Create chat block with title and borders
    let mut chat_block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(if app.focus == Focus::Chat {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    // Check status tracker first for errors and tool execution status
    let status_info = app.status_tracker.current();
    
    if let Some(status) = &app.status_message {
        chat_block = chat_block.title_bottom(Line::from(vec![
            Span::styled(format!(" {} ", status), Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC))
        ]));
    } else if let crate::tui::status_tracker::StatusInfo::Error { message } = status_info {
        // Show error from status tracker (e.g., tool execution errors)
        let err_preview = if message.len() > 50 {
            format!("{}...", &message[..50])
        } else {
            message.clone()
        };
        chat_block = chat_block.title_bottom(Line::from(vec![
            Span::styled(format!(" ❌ Error: {} ", err_preview), 
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        ]));
    } else if app.state != AppState::Idle {
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame = spinner[(app.status_animation_frame % spinner.len() as u64) as usize];
        
        let (status_text, color) = match &app.state {
            AppState::Thinking(info) => (format!(" {} Thinking ({}) ", frame, info), Color::Yellow),
            AppState::Streaming(info) => (format!(" {} Streaming: {} ", frame, info), Color::Green),
            AppState::ExecutingTool(tool) => (format!(" {} Executing: {} ", frame, tool), Color::Cyan),
            AppState::WaitingForUser => (" ⏳ Waiting for Approval ".to_string(), Color::Magenta),
            AppState::AwaitingApproval { .. } => (" ⏳ Awaiting your response ".to_string(), Color::Yellow),
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

    let chat_list = List::new(list_items).block(chat_block);
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

fn render_confirm_exit(frame: &mut Frame, _app: &mut App) {
    let area = frame.area();
    
    // Simple centered dialog for y/n confirmation
    let dialog_area = centered_rect(50, 20, area);
    
    // Clear background
    frame.render_widget(ratatui::widgets::Clear, dialog_area);
    
    let block = Block::default()
        .title(" ⚠️  Exit Confirmation ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
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

    let question = Paragraph::new(Line::from(vec![
        Span::raw("Are you sure you want to exit?"),
    ])).alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(question, chunks[0]);

    let instructions = Paragraph::new(Line::from(vec![
        Span::styled(" [Y] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("Yes, exit"),
        Span::raw("  "),
        Span::styled(" [N] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw("No, cancel"),
    ])).alignment(ratatui::layout::Alignment::Center);
    
    frame.render_widget(instructions, chunks[1]);
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

    // Show different title when focused
    let title = if app.focus == Focus::Jobs {
        format!(" Background Jobs [{} active] | ↑↓:select | c:cancel | a:cancel-all | Enter:journey | Esc:close ",
            jobs.iter().filter(|j| matches!(j.status, JobStatus::Running)).count())
    } else {
        " Background Jobs [NOT FOCUSED - Press F2 to focus, F4 to close] ".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(if app.focus == Focus::Jobs {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else if !jobs.is_empty() {
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
            JobStatus::Running => "●",
            JobStatus::Completed => "✓",
            JobStatus::Failed => "✗",
            JobStatus::Cancelled => "⊘",
            JobStatus::TimeoutPending => "⏱",
            JobStatus::Stalled => "⚠",
        };
        let status_color = match job.status {
            JobStatus::Running => Color::Yellow,
            JobStatus::Completed => Color::Green,
            JobStatus::Failed => Color::Red,
            JobStatus::Cancelled => Color::Magenta,
            JobStatus::TimeoutPending => Color::Yellow,
            JobStatus::Stalled => Color::Red,
        };

        // Get current step from action log
        let current_step = job.current_action()
            .unwrap_or_else(|| match job.status {
                JobStatus::Completed => "Completed",
                JobStatus::Failed => "Failed",
                JobStatus::Cancelled => "Cancelled",
                _ => "Starting...",
            });

        // Calculate progress based on action log
        let total_steps = job.action_log.iter()
            .filter(|e| matches!(e.action_type, ActionType::ToolCall | ActionType::Thought))
            .count();
        let progress = if total_steps > 0 {
            format!(" [{} steps]", total_steps)
        } else {
            String::new()
        };

        let is_selected = app.selected_job_index == Some(idx);
        let prefix = if is_selected { "► " } else { "  " };

        // Token usage with progress bar (will be placed at end of line1)
        let (used_tokens, max_tokens) = job.context_window();
        let token_ratio = if max_tokens > 0 { used_tokens as f64 / max_tokens as f64 } else { 0.0 };
        let bar_width = 10; // Width of progress bar in characters
        let filled = (token_ratio * bar_width as f64).round() as usize;
        let filled = filled.clamp(0, bar_width);
        let empty = bar_width - filled;
        
        // Choose color based on usage
        let token_color = if token_ratio >= 0.8 {
            Color::Red
        } else if token_ratio >= 0.5 {
            Color::Yellow
        } else {
            Color::Green
        };

        // Build progress bar with unicode blocks
        let progress_bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
        let token_text = format!(" {}↑/{} ", used_tokens, max_tokens);
        
        // Calculate space needed for token/progress section
        let token_section_len = progress_bar.len() + token_text.len();
        
        // First line: status icon, short_id, description, [N steps], tokens:X/Y [progress bar]
        // We need to reserve space for: prefix(2) + "#ID"(9) + status(1) + spaces + progress + token_section
        let reserved_len = 2 + 9 + 1 + 3 + progress.len() + 3 + token_section_len;
        let max_desc_len = area.width.saturating_sub(reserved_len as u16) as usize;
        
        let desc = if max_desc_len > 5 && job.description.len() > max_desc_len {
            format!("{}...", &job.description[..max_desc_len.saturating_sub(3)])
        } else {
            job.description.clone()
        };

        let line1 = Line::from(vec![
            Span::raw(prefix),
            Span::styled(status_str, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(format!("#{}", short_id), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(desc, Style::default().fg(Color::White)),
            Span::styled(progress, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(progress_bar, Style::default().fg(token_color)),
            Span::styled(token_text, Style::default().fg(Color::DarkGray)),
        ]);

        // Second line: current step (indented) - optional if there's a current action
        let line2 = if !current_step.is_empty() {
            let max_step_len = area.width.saturating_sub(15) as usize;
            let step_display = if current_step.len() > max_step_len {
                format!("{}...", &current_step[..max_step_len.saturating_sub(3)])
            } else {
                current_step.to_string()
            };
            Some(Line::from(vec![
                Span::raw("       "),
                Span::styled("└─ ", Style::default().fg(Color::DarkGray)),
                Span::styled(step_display, Style::default().fg(Color::Cyan)),
            ]))
        } else {
            None
        };

        if let Some(line2) = line2 {
            items.push(ListItem::new(vec![line1, line2]));
        } else {
            items.push(ListItem::new(line1));
        }
    }

    let list = List::new(items)
        .block(block);

    frame.render_widget(list, area);
}

fn render_job_detail(frame: &mut Frame, app: &mut App, area: Rect) {
    // Use a split view: left side for journey/steps, right side for details
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Job Journey (Esc/q: close, ↑↓: scroll) ")
        .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Get all jobs and find the selected one
    let jobs = app.job_registry.list_all_jobs();
    let Some(selected_idx) = app.selected_job_index else {
        let paragraph = Paragraph::new("No job selected")
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner_area);
        return;
    };
    
    let Some(job) = jobs.get(selected_idx) else {
        let paragraph = Paragraph::new("Job not found")
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner_area);
        return;
    };

    // Build journey content - narrative flow of execution
    let mut lines = Vec::new();

    // Header with mission
    lines.push(Line::from(vec![
        Span::styled("╔══════════════════════════════════════════════════════════════╗", Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("║  ", Style::default().fg(Color::Cyan)),
        Span::styled("MISSION", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("                                                    ║", Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("╚══════════════════════════════════════════════════════════════╝", Style::default().fg(Color::Cyan)),
    ]));
    
    // Word wrap the description nicely
    for line in wrap_text(&job.description, inner_area.width.saturating_sub(4) as usize) {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(line, Style::default().fg(Color::White)),
        ]));
    }
    lines.push(Line::from(""));

    // Status line
    let (status_text, status_color) = match job.status {
        JobStatus::Running => ("▶ RUNNING", Color::Yellow),
        JobStatus::Completed => ("✓ COMPLETED", Color::Green),
        JobStatus::Failed => ("✗ FAILED", Color::Red),
        JobStatus::Cancelled => ("⊘ CANCELLED", Color::Magenta),
        JobStatus::TimeoutPending => ("⏱ TIMEOUT PENDING", Color::Yellow),
        JobStatus::Stalled => ("⚠ STALLED", Color::Red),
    };
    lines.push(Line::from(vec![
        Span::raw("  Status: "),
        Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        Span::raw(format!("  |  Job: #{}  |  Tool: {}", &job.id[..8.min(job.id.len())], job.tool_name)),
    ]));
    lines.push(Line::from(""));

    // Journey Timeline
    lines.push(Line::from(vec![
        Span::styled("╔══════════════════════════════════════════════════════════════╗", Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("║  ", Style::default().fg(Color::Cyan)),
        Span::styled("EXECUTION JOURNEY", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("                                           ║", Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("╚══════════════════════════════════════════════════════════════╝", Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(""));

    // Build journey from action log
    if job.action_log.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Waiting for execution to start...", Style::default().fg(Color::DarkGray)),
        ]));
    } else {
        let mut step_num = 1;
        let mut last_was_tool = false;
        
        for entry in &job.action_log {
            match entry.action_type {
                ActionType::Thought => {
                    if !last_was_tool {
                        lines.push(Line::from(""));
                    }
                    lines.push(Line::from(vec![
                        Span::styled(format!("  Step {}: ", step_num), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::styled("💭 Thinking", Style::default().fg(Color::Cyan)),
                        Span::styled(format!(" [{}]", entry.timestamp.format("%H:%M:%S")), Style::default().fg(Color::DarkGray)),
                    ]));
                    for line in wrap_text(&entry.content, inner_area.width.saturating_sub(8) as usize) {
                        lines.push(Line::from(vec![
                            Span::raw("           "),
                            Span::styled(line, Style::default().fg(Color::Gray)),
                        ]));
                    }
                    step_num += 1;
                    last_was_tool = false;
                }
                ActionType::ToolCall => {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled(format!("  Step {}: ", step_num), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::styled("🔧 Action", Style::default().fg(Color::Blue)),
                        Span::styled(format!(" [{}]", entry.timestamp.format("%H:%M:%S")), Style::default().fg(Color::DarkGray)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::raw("           "),
                        Span::styled(&entry.content, Style::default().fg(Color::White)),
                    ]));
                    step_num += 1;
                    last_was_tool = true;
                }
                ActionType::ToolResult => {
                    lines.push(Line::from(vec![
                        Span::raw("           "),
                        Span::styled("└─ ", Style::default().fg(Color::DarkGray)),
                        Span::styled("📤 Result: ", Style::default().fg(Color::Green)),
                        Span::styled(&entry.content, Style::default().fg(Color::Gray)),
                    ]));
                    last_was_tool = false;
                }
                ActionType::Error => {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("  ⚠️  ", Style::default().fg(Color::Red)),
                        Span::styled("ERROR", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                        Span::styled(format!(" [{}]", entry.timestamp.format("%H:%M:%S")), Style::default().fg(Color::DarkGray)),
                    ]));
                    for line in wrap_text(&entry.content, inner_area.width.saturating_sub(8) as usize) {
                        lines.push(Line::from(vec![
                            Span::raw("      "),
                            Span::styled(line, Style::default().fg(Color::Red)),
                        ]));
                    }
                    last_was_tool = false;
                }
                ActionType::FinalAnswer => {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("  ════════════════════════════════════════════════════════════", Style::default().fg(Color::Green)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                        Span::styled("TASK COMPLETE", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("  ════════════════════════════════════════════════════════════", Style::default().fg(Color::Green)),
                    ]));
                    last_was_tool = false;
                }
                ActionType::System => {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled("⚙ ", Style::default().fg(Color::DarkGray)),
                        Span::styled(&entry.content, Style::default().fg(Color::DarkGray)),
                    ]));
                }
                _ => {
                    // Handle Shell, Read, Write, Search, Ask, Done - treat as generic actions
                    lines.push(Line::from(vec![
                        Span::styled(format!("  Step {}: ", step_num), Style::default().fg(Color::Yellow)),
                        Span::styled("⚡ Action", Style::default().fg(Color::Blue)),
                        Span::styled(format!(" [{}]", entry.timestamp.format("%H:%M:%S")), Style::default().fg(Color::DarkGray)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::raw("           "),
                        Span::styled(&entry.content, Style::default().fg(Color::White)),
                    ]));
                    step_num += 1;
                    last_was_tool = false;
                }
            }
        }
    }

    // Output section if available
    if !job.output.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("╔══════════════════════════════════════════════════════════════╗", Style::default().fg(Color::Cyan)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("║  ", Style::default().fg(Color::Cyan)),
            Span::styled("FINAL OUTPUT", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("                                          ║", Style::default().fg(Color::Cyan)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("╚══════════════════════════════════════════════════════════════╝", Style::default().fg(Color::Cyan)),
        ]));
        lines.push(Line::from(""));
        for line in job.output.lines() {
            lines.push(Line::from(vec![Span::raw(format!("  {}", line))]));
        }
    }

    // Error section
    if let Some(ref error) = job.error {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("╔══════════════════════════════════════════════════════════════╗", Style::default().fg(Color::Red)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("║  ", Style::default().fg(Color::Red)),
            Span::styled("ERROR DETAILS", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled("                                          ║", Style::default().fg(Color::Red)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("╚══════════════════════════════════════════════════════════════╝", Style::default().fg(Color::Red)),
        ]));
        lines.push(Line::from(""));
        for line in error.lines() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {}", line), Style::default().fg(Color::Red)),
            ]));
        }
    }

    // Metrics footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("─" .repeat(inner_area.width as usize), Style::default().fg(Color::DarkGray)),
    ]));
    let metrics = &job.metrics;
    lines.push(Line::from(vec![
        Span::styled("  📊 Metrics: ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("Tokens: {}↑ {}↓ ({} total) | Requests: {} | Errors: {}",
            metrics.prompt_tokens, metrics.completion_tokens, metrics.total_tokens,
            metrics.request_count, metrics.error_count)),
    ]));

    // Apply scrolling
    let visible_height = inner_area.height as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(visible_height);
    app.job_scroll = app.job_scroll.min(max_scroll);

    let start_idx = app.job_scroll;
    let end_idx = (start_idx + visible_height).min(total_lines);
    let visible_content: Vec<Line> = lines[start_idx..end_idx].to_vec();

    let paragraph = Paragraph::new(visible_content)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, inner_area);
}

// Timestamp formatting helpers for memory view

/// Format a unix timestamp as a compact string for the list view
/// Shows date if not today, otherwise shows time
fn format_timestamp(ts: i64) -> String {
    use chrono::{DateTime, Local, Utc};
    
    let dt = DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|| Local::now());
    
    let now = Local::now();
    let is_today = dt.date_naive() == now.date_naive();
    
    if is_today {
        // Today: show time only
        dt.format("%H:%M").to_string()
    } else {
        // Not today: show month/day
        dt.format("%m/%d").to_string()
    }
}

/// Format a unix timestamp as a full string for the detail view
fn format_timestamp_full(ts: i64) -> String {
    use chrono::{DateTime, Local, Utc};
    
    DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}
