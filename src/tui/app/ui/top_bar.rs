//! Top bar rendering - status, toggles, F-keys, and context gauge

use crate::tui::app::state::AppStateContainer as App;
use crate::tui::app::types::AppState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Gauge, Paragraph},
    Frame,
};
use std::sync::atomic::Ordering;

pub fn render_top_bar(frame: &mut Frame, app: &mut App, area: Rect, _height: u16) {
    let stats = app.session_monitor.get_stats();
    let auto_approve = app.auto_approve.load(Ordering::SeqCst);

    // Get status info from tracker
    let status_info = app.status_tracker.current();

    // Get elapsed time - use tool elapsed if executing, otherwise state elapsed
    let elapsed = match status_info {
        crate::tui::app::status_tracker::StatusInfo::Executing { .. } => app
            .status_tracker
            .tool_elapsed()
            .unwrap_or_else(|| app.state_started_at.elapsed()),
        _ => app.state_started_at.elapsed(),
    };

    let elapsed_text = super::utils::format_elapsed(elapsed);

    // Build status label and color based on status tracker state (static, no animation)
    let (state_label, state_color, is_active) = match status_info {
        crate::tui::app::status_tracker::StatusInfo::Error { message } => {
            let msg = if message.len() > 35 {
                format!("{}...", &message[..35])
            } else {
                message.clone()
            };
            (format!("⚠ Error: {}", msg), Color::Red, false)
        }
        crate::tui::app::status_tracker::StatusInfo::Executing { tool, args } => {
            let args_preview = if args.len() > 25 {
                format!("{}...", &args[..25])
            } else if args.is_empty() {
                "".to_string()
            } else {
                format!(" {}", args)
            };
            (format!("⚡ {}{}", tool, args_preview), Color::Cyan, true)
        }
        crate::tui::app::status_tracker::StatusInfo::Thinking => {
            ("💭 Thinking...".to_string(), Color::Yellow, true)
        }
        crate::tui::app::status_tracker::StatusInfo::AwaitingApproval { tool, .. } => {
            (format!("⏸ Approve {}? (y/n)", tool), Color::Magenta, true)
        }
        crate::tui::app::status_tracker::StatusInfo::Idle => match &app.state {
            AppState::Idle => ("✓ Ready".to_string(), Color::Green, false),
            AppState::Thinking(info) => (format!("💭 {}", info), Color::Yellow, true),
            AppState::Streaming(info) => (format!("📡 {}", info), Color::Cyan, true),
            AppState::ExecutingTool(tool) => (format!("⚡ {}", tool), Color::Cyan, true),
            AppState::WaitingForUser => ("⏸ Waiting".to_string(), Color::Magenta, false),
            AppState::AwaitingApproval { tool, .. } => {
                (format!("⏸ Approve {}? (y/n)", tool), Color::Magenta, true)
            }
            AppState::Error(err) => (format!("⚠ {}", err), Color::Red, false),
            AppState::ConfirmExit => ("❓ Exit? (y/n)".to_string(), Color::Yellow, false),
            AppState::NamingSession => ("✎ Naming...".to_string(), Color::Cyan, true),
        },
    };

    // Static indicator (no animation in top bar)
    let state_prefix = if is_active { "◐" } else { "●" };

    // Top row: version | toggles | F-keys | state
    let left_spans = vec![
        Span::styled(
            " mylm ",
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("v{} ", env!("CARGO_PKG_VERSION")),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    // Center: toggles + F-keys (full names)
    let center_spans = vec![
        // Auto-Approve toggle
        Span::styled(
            if auto_approve {
                "[Auto-approval ✓]"
            } else {
                "[Auto-approval ✗]"
            },
            Style::default().fg(if auto_approve {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw(" "),
        // Verbose toggle
        Span::styled(
            if app.verbose_mode {
                "[Verbose on]"
            } else {
                "[Verbose off]"
            },
            Style::default().fg(if app.verbose_mode {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw(" "),
        // F-keys (full names)
        Span::styled(
            "[F1 Help]",
            Style::default().fg(if app.show_help_view {
                Color::Green
            } else {
                Color::Yellow
            }),
        ),
        Span::styled("[F2 Focus]", Style::default().fg(Color::Yellow)),
        Span::styled(
            "[F3 Memory]",
            Style::default().fg(if app.show_memory_view {
                Color::Green
            } else {
                Color::Yellow
            }),
        ),
        Span::styled(
            "[F4 Jobs]",
            Style::default().fg(if app.show_jobs_panel {
                Color::Green
            } else {
                Color::Yellow
            }),
        ),
        Span::styled("[Esc: Exit]", Style::default().fg(Color::Red)),
    ];

    // Right side: animated spinner + state + elapsed
    let right_spans = vec![
        Span::styled(
            format!("{} ", state_prefix),
            Style::default().fg(state_color),
        ),
        Span::styled(
            state_label,
            Style::default()
                .fg(state_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", elapsed_text),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    // Context usage gauge - Show CACHED tokens (system + history)
    // This is what remains for next request (minus ephemeral)
    let (cached_tokens, max_tokens) = app.context_manager.get_cached_token_usage();
    let ratio = app.context_manager.get_cached_context_ratio();
    let gauge_color = if ratio >= 0.9 {
        Color::Red
    } else if ratio >= 0.7 {
        Color::Yellow
    } else {
        Color::Green
    };

    // Gauge label with cost and context
    let label = format!(
        "${:.2} │ CTX:{}/{} {:.0}%",
        stats.cost,
        super::utils::format_tokens(cached_tokens as u32),
        super::utils::format_tokens(max_tokens as u32),
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
    // Use Percentage constraints to allow flexible sizing and prevent truncation
    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Percentage(60),
            Constraint::Percentage(25),
        ])
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
