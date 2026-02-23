//! Jobs panel and job detail rendering

use crate::tui::app::state::AppStateContainer as App;
use crate::tui::app::types::{ActionType, Focus, JobStatus};
use ratatui::{
    layout::{Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render_jobs_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    // Get all jobs from the registry (including completed and failed)
    let jobs = app.job_registry.list_all_jobs();

    // Show different title when focused
    let title = if app.focus == Focus::Jobs {
        format!(
            " Background Jobs [{} active] | ↑↓:select | c:cancel | a:cancel-all | Enter:journey | Esc:close ",
            jobs.iter().filter(|j| matches!(j.status, JobStatus::Running)).count()
        )
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
        let current_step = job
            .current_action()
            .unwrap_or_else(|| match job.status {
                JobStatus::Completed => "Completed",
                JobStatus::Failed => "Failed",
                JobStatus::Cancelled => "Cancelled",
                _ => "Starting...",
            });

        // Calculate progress based on action log
        let total_steps = job
            .action_log
            .iter()
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
        let token_ratio = if max_tokens > 0 {
            used_tokens as f64 / max_tokens as f64
        } else {
            0.0
        };
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

        // Build progress bar with unicode blocks (or show "—" when not tracked)
        let (progress_bar, token_text) = if max_tokens == 0 {
            ("".to_string(), " —/— ".to_string())
        } else {
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
            let text = format!(" {}↑/{} ", used_tokens, max_tokens);
            (bar, text)
        };

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
            Span::styled(
                status_str,
                Style::default().fg(status_color).add_modifier(Modifier::BOLD),
            ),
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

    let list = List::new(items).block(block);

    frame.render_widget(list, area);
}

pub fn render_job_detail(frame: &mut Frame, app: &mut App, area: Rect) {
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
        let paragraph = Paragraph::new("No job selected").wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner_area);
        return;
    };

    let Some(job) = jobs.get(selected_idx) else {
        let paragraph = Paragraph::new("Job not found").wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner_area);
        return;
    };

    // Build journey content - narrative flow of execution
    let mut lines = Vec::new();

    // Header with mission
    lines.push(Line::from(vec![Span::styled(
        "╔══════════════════════════════════════════════════════════════╗",
        Style::default().fg(Color::Cyan),
    )]));
    lines.push(Line::from(vec![
        Span::styled("║  ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "MISSION",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "                                                    ║",
            Style::default().fg(Color::Cyan),
        ),
    ]));
    lines.push(Line::from(vec![Span::styled(
        "╚══════════════════════════════════════════════════════════════╝",
        Style::default().fg(Color::Cyan),
    )]));

    // Word wrap the description nicely
    for line in super::utils::wrap_text(&job.description, inner_area.width.saturating_sub(4) as usize) {
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
        Span::styled(
            status_text,
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  |  Job: #{}  |  Tool: {}",
            &job.id[..8.min(job.id.len())],
            job.tool_name
        )),
    ]));
    lines.push(Line::from(""));

    // Journey Timeline
    lines.push(Line::from(vec![Span::styled(
        "╔══════════════════════════════════════════════════════════════╗",
        Style::default().fg(Color::Cyan),
    )]));
    lines.push(Line::from(vec![
        Span::styled("║  ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "EXECUTION JOURNEY",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "                                           ║",
            Style::default().fg(Color::Cyan),
        ),
    ]));
    lines.push(Line::from(vec![Span::styled(
        "╚══════════════════════════════════════════════════════════════╝",
        Style::default().fg(Color::Cyan),
    )]));
    lines.push(Line::from(""));

    // Build journey from action log
    if job.action_log.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  Waiting for execution to start...",
            Style::default().fg(Color::DarkGray),
        )]));
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
                        Span::styled(
                            format!("  Step {}: ", step_num),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("💭 Thinking", Style::default().fg(Color::Cyan)),
                        Span::styled(
                            format!(" [{}]", entry.timestamp.format("%H:%M:%S")),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                    for line in super::utils::wrap_text(
                        &entry.content,
                        inner_area.width.saturating_sub(8) as usize,
                    ) {
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
                        Span::styled(
                            format!("  Step {}: ", step_num),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("🔧 Action", Style::default().fg(Color::Blue)),
                        Span::styled(
                            format!(" [{}]", entry.timestamp.format("%H:%M:%S")),
                            Style::default().fg(Color::DarkGray),
                        ),
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
                        Span::styled(
                            "ERROR",
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(" [{}]", entry.timestamp.format("%H:%M:%S")),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                    for line in super::utils::wrap_text(
                        &entry.content,
                        inner_area.width.saturating_sub(8) as usize,
                    ) {
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
                        Span::styled(
                            "  ════════════════════════════════════════════════════════════",
                            Style::default().fg(Color::Green),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                        Span::styled(
                            "TASK COMPLETE",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled(
                            "  ════════════════════════════════════════════════════════════",
                            Style::default().fg(Color::Green),
                        ),
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
                        Span::styled(
                            format!("  Step {}: ", step_num),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::styled("⚡ Action", Style::default().fg(Color::Blue)),
                        Span::styled(
                            format!(" [{}]", entry.timestamp.format("%H:%M:%S")),
                            Style::default().fg(Color::DarkGray),
                        ),
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
        lines.push(Line::from(vec![Span::styled(
            "╔══════════════════════════════════════════════════════════════╗",
            Style::default().fg(Color::Cyan),
        )]));
        lines.push(Line::from(vec![
            Span::styled("║  ", Style::default().fg(Color::Cyan)),
            Span::styled(
                "FINAL OUTPUT",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "                                          ║",
                Style::default().fg(Color::Cyan),
            ),
        ]));
        lines.push(Line::from(vec![Span::styled(
            "╚══════════════════════════════════════════════════════════════╝",
            Style::default().fg(Color::Cyan),
        )]));
        lines.push(Line::from(""));
        for line in job.output.lines() {
            lines.push(Line::from(vec![Span::raw(format!("  {}", line))]));
        }
    }

    // Error section
    if let Some(ref error) = job.error {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "╔══════════════════════════════════════════════════════════════╗",
            Style::default().fg(Color::Red),
        )]));
        lines.push(Line::from(vec![
            Span::styled("║  ", Style::default().fg(Color::Red)),
            Span::styled(
                "ERROR DETAILS",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "                                          ║",
                Style::default().fg(Color::Red),
            ),
        ]));
        lines.push(Line::from(vec![Span::styled(
            "╚══════════════════════════════════════════════════════════════╝",
            Style::default().fg(Color::Red),
        )]));
        lines.push(Line::from(""));
        for line in error.lines() {
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::Red),
            )]));
        }
    }

    // Metrics footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "─".repeat(inner_area.width as usize),
        Style::default().fg(Color::DarkGray),
    )]));
    let metrics = &job.metrics;
    lines.push(Line::from(vec![
        Span::styled("  📊 Metrics: ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(
            "Tokens: {}↑ {}↓ ({} total) | Requests: {} | Errors: {}",
            metrics.prompt_tokens,
            metrics.completion_tokens,
            metrics.total_tokens,
            metrics.request_count,
            metrics.error_count
        )),
    ]));

    // Apply scrolling
    let visible_height = inner_area.height as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(visible_height);
    app.job_scroll = app.job_scroll.min(max_scroll);

    let start_idx = app.job_scroll;
    let end_idx = (start_idx + visible_height).min(total_lines);
    let visible_content: Vec<Line> = lines[start_idx..end_idx].to_vec();

    let paragraph = Paragraph::new(visible_content).wrap(Wrap { trim: false });

    frame.render_widget(paragraph, inner_area);
}
