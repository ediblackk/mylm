//! Chat pane rendering - history and input

use crate::tui::app::state::AppStateContainer as App;
use crate::tui::app::types::{ActionType, AppState, Focus};
use mylm_core::provider::chat::MessageRole;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render_chat(frame: &mut Frame, app: &mut App, area: Rect) {
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
    let wrapped_input = super::utils::wrap_text(&input_content, input_width);
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
            Constraint::Length(input_height),
        ])
        .split(area);

    // Set chat_input_area after layout is computed for mouse detection
    app.chat_input_area = Some(chunks[2]);
    // Store the starting column for chat history (after layout is determined)
    app.chat_history_start_col = Some(chunks[0].x + 1);

    // Render PaCoRe progress bar if active
    if show_progress {
        if let Some((completed, total)) = app.pacore_progress {
            let ratio = if total > 0 {
                completed as f64 / total as f64
            } else {
                0.0
            };
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
            if m.role == MessageRole::Tool
                || (m.role == MessageRole::User && m.content.contains("Observation:"))
            {
                // Placeholder line: "AI: Command executed. Check terminal."
                let prefix = "AI: ";
                let prefix_len = prefix.len();
                let prefix_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
                let content = "Command executed. Check terminal.";
                let full_text = format!("{}{}", prefix, content);
                all_visual_lines.push(VisualLineInfo {
                    full_text: full_text.clone(),
                    prefix_len,
                    prefix_style,
                    content_style: Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                });
                app.chat_visual_lines.push((full_text, abs_line_idx));
                abs_line_idx += 1;
                // Separator line (empty)
                all_visual_lines.push(VisualLineInfo {
                    full_text: String::new(),
                    prefix_len: 0,
                    prefix_style: Style::default(),
                    content_style: Style::default(),
                });
                app.chat_visual_lines.push((String::new(), abs_line_idx));
                abs_line_idx += 1;
            }
            continue;
        }

        // Skip Tool messages for commands in non-verbose mode
        if !app.verbose_mode
            && m.role == MessageRole::Tool
            && m.name.as_deref() == Some("execute_command")
        {
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
        let processed_content =
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw_display_content.trim()) {
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

            let is_thought = trimmed.starts_with("Thought:")
                || trimmed.starts_with("**Thought:**")
                || trimmed.starts_with("💭");
            if is_thought {
                // Show thoughts only in verbose mode
                let thought_style = Style::default()
                    .fg(Color::Rgb(128, 128, 128))
                    .add_modifier(Modifier::ITALIC);
                if app.verbose_mode {
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
                        if app.verbose_mode && has_thought {
                            if let Some(t) = val.get("t").and_then(|v| v.as_str()) {
                                lines_to_render.push((
                                    format!("Thought: {}", t),
                                    Style::default()
                                        .fg(Color::DarkGray)
                                        .add_modifier(Modifier::ITALIC),
                                ));
                            }
                        }
                        if has_action && app.verbose_mode {
                            if let Some(a) = val.get("a").and_then(|v| v.as_str()) {
                                let i = val
                                    .get("i")
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                                lines_to_render.push((
                                    format!("Action: {} ({})", a, i),
                                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                                ));
                            }
                        }
                        continue;
                    }
                }
            }

            let is_action = trimmed.starts_with("Action:") || trimmed.starts_with("**Action:**");
            if is_action {
                lines_to_render.push((
                    line,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ));
                continue;
            }

            let is_action_input = trimmed.starts_with("Action Input:")
                || trimmed.starts_with("**Action Input:**");
            if is_action_input {
                if !app.verbose_mode {
                    continue;
                }
                lines_to_render.push((line, Style::default().fg(Color::DarkGray)));
                continue;
            }

            let is_observation = trimmed.starts_with("Observation:")
                || trimmed.starts_with("**Observation:**");
            if !app.verbose_mode && (is_observation || trimmed.contains("CMD_OUTPUT:")) {
                continue;
            }

            let is_final_answer = trimmed.starts_with("Final Answer:")
                || trimmed.starts_with("**Final Answer:**");
            if is_final_answer {
                let content = line
                    .replace("Final Answer:", "")
                    .replace("**Final Answer:**", "");
                lines_to_render.push((content.trim().to_string(), Style::default()));
                continue;
            }

            lines_to_render.push((line, Style::default()));
        }

        // Skip AI messages that would have no visible content (before adding timestamp)
        if m.role == MessageRole::Assistant
            && lines_to_render.iter().all(|(l, _)| l.trim().is_empty())
        {
            continue;
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
        lines_to_render.push((
            padded_bottom,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ));

        // Process lines for visual representation
        // Subtract prefix_len to account for indentation on continuation lines
        let content_width = available_width.saturating_sub(prefix.len()).saturating_sub(prefix_len);
        let mut first_line_flag = true;
        for (text, style) in lines_to_render {
            if text.is_empty() {
                if first_line_flag {
                    // Skip empty lines at the start (don't render "AI:" alone)
                    continue;
                }
                // Empty line (soft wrap break) - no prefix
                all_visual_lines.push(VisualLineInfo {
                    full_text: String::new(),
                    prefix_len: 0,
                    prefix_style: Style::default(),
                    content_style: Style::default(),
                });
                app.chat_visual_lines.push((String::new(), abs_line_idx));
                abs_line_idx += 1;
                continue;
            }

            let wrapped = super::utils::wrap_text(&text, content_width);
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
                    content_style: style,
                });
                app.chat_visual_lines.push((full_text, abs_line_idx));
                abs_line_idx += 1;
            }
            first_line_flag = false;
        }
        // Add separator line (empty)
        all_visual_lines.push(VisualLineInfo {
            full_text: String::new(),
            prefix_len: 0,
            prefix_style: Style::default(),
            content_style: Style::default(),
        });
        app.chat_visual_lines.push((String::new(), abs_line_idx));
        abs_line_idx += 1;
    }

    // Add action stamps as conversation items at the end
    let recent_stamps = app.context_manager.recent_stamps(10);
    if !recent_stamps.is_empty() {
        use mylm_core::ui::ActionStampType;

        // Add a small header for stamps section
        all_visual_lines.push(VisualLineInfo {
            full_text: "── Action Stamps ──".to_string(),
            prefix_len: 0,
            prefix_style: Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            content_style: Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
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
            let wrapped = super::utils::wrap_text(&stamp_text, content_width);

            for (idx, line) in wrapped.iter().enumerate() {
                let prefix = if idx == 0 { "  " } else { "    " };
                let full_text = format!("{}{}", prefix, line);
                all_visual_lines.push(VisualLineInfo {
                    full_text: full_text.clone(),
                    prefix_len: prefix.len(),
                    prefix_style: Style::default(),
                    content_style: Style::default().fg(color),
                });
                app.chat_visual_lines.push((full_text, abs_line_idx));
                abs_line_idx += 1;
            }
        }

        // Add separator after stamps
        all_visual_lines.push(VisualLineInfo {
            full_text: String::new(),
            prefix_len: 0,
            prefix_style: Style::default(),
            content_style: Style::default(),
        });
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
    for (abs_line_idx, visual_line) in all_visual_lines
        .iter()
        .enumerate()
        .skip(start_index)
        .take(end_index - start_index)
    {
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
        chat_block = chat_block.title_bottom(Line::from(vec![Span::styled(
            format!(" {} ", status),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::ITALIC),
        )]));
    } else if let crate::tui::app::status_tracker::StatusInfo::Error { message } = status_info {
        // Show error from status tracker (e.g., tool execution errors)
        let err_preview = if message.len() > 50 {
            format!("{}...", &message[..50])
        } else {
            message.clone()
        };
        chat_block = chat_block.title_bottom(Line::from(vec![Span::styled(
            format!(" ❌ Error: {} ", err_preview),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]));
    } else if app.state != AppState::Idle {
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame = spinner[(app.status_animation_frame % spinner.len() as u64) as usize];

        let (status_text, color) = match &app.state {
            AppState::Thinking(info) => {
                (format!(" {} Thinking ({}) ", frame, info), Color::Yellow)
            }
            AppState::Streaming(info) => {
                (format!(" {} Streaming: {} ", frame, info), Color::Green)
            }
            AppState::ExecutingTool(tool) => {
                (format!(" {} Executing: {} ", frame, tool), Color::Cyan)
            }
            AppState::WaitingForUser => {
                (" ⏳ Waiting for Approval ".to_string(), Color::Magenta)
            }
            AppState::AwaitingApproval { .. } => {
                (" ⏳ Awaiting your response ".to_string(), Color::Yellow)
            }
            AppState::Error(err) => (format!(" ❌ Error: {} ", err), Color::Red),
            AppState::ConfirmExit => (" ⚠️  Confirm Exit? ".to_string(), Color::Yellow),
            AppState::NamingSession => (" 💾 Name Session ".to_string(), Color::Cyan),
            AppState::Idle => unreachable!(),
        };
        chat_block = chat_block.title_bottom(Line::from(vec![Span::styled(
            status_text,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]));
    } else if !app.chat_auto_scroll {
        chat_block = chat_block.title_bottom(Line::from(vec![Span::styled(
            " [SCROLLING] ",
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
        )]));
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
        let p = Paragraph::new(Span::styled(
            &input_content,
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        ))
        .block(input_block)
        .wrap(Wrap { trim: true });
        frame.render_widget(p, chunks[2]);
    } else {
        // Calculate cursor position in wrapped text
        let (cursor_x, cursor_y) =
            super::utils::calculate_input_cursor_pos(&app.chat_input, app.cursor_position, input_width);

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

        let input_paragraph = Paragraph::new(display_content).block(input_block);
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
