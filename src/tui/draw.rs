//! UI Rendering Module
//!
//! Handles drawing the TUI interface using ratatui.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::App;
use crate::tui::types::{AppState, Focus, JobStatus};

/// Draw the complete UI
pub fn draw_ui(f: &mut Frame, app: &mut App) {
    let size = f.area();
    
    // Main layout: chat + terminal side by side
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(app.chat_width_percent),
            Constraint::Percentage(100 - app.chat_width_percent),
        ])
        .split(size);
    
    // Draw chat panel
    draw_chat_panel(f, app, main_chunks[0]);
    
    // Draw terminal panel (or jobs panel if visible)
    if app.show_jobs_panel {
        let job_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Percentage(30)])
            .split(main_chunks[1]);
        draw_terminal_panel(f, app, job_chunks[0]);
        draw_jobs_panel(f, app, job_chunks[1]);
    } else {
        draw_terminal_panel(f, app, main_chunks[1]);
    }
    
    // Draw help overlay if visible
    if app.show_help_view {
        draw_help_overlay(f, app, size);
    }
    
    // Draw memory view if visible
    if app.show_memory_view {
        draw_memory_overlay(f, app, size);
    }
    
    // Draw status line
    draw_status_line(f, app, size);
}

fn draw_chat_panel(f: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.focus == Focus::Chat {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    
    let chat_block = Block::default()
        .title(" Chat ")
        .borders(Borders::ALL)
        .border_style(border_style);
    
    let inner = chat_block.inner(area);
    f.render_widget(chat_block, area);
    
    // Split into history and input
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(inner);
    
    // Draw chat history
    draw_chat_history(f, app, chunks[0]);
    
    // Draw input area
    draw_input_area(f, app, chunks[1]);
}

fn draw_chat_history(f: &mut Frame, app: &App, area: Rect) {
    use mylm_core::llm::chat::MessageRole;
    
    let mut lines: Vec<Line> = Vec::new();
    
    for msg in &app.chat_history {
        let (prefix, style) = match msg.role {
            MessageRole::User => ("You: ", Style::default().fg(Color::Green)),
            MessageRole::Assistant => ("AI: ", Style::default().fg(Color::Blue)),
            MessageRole::System => ("", Style::default().fg(Color::Gray)),
            _ => ("", Style::default()),
        };
        
        let content = format!("{}{}", prefix, msg.content);
        for line in content.lines() {
            lines.push(Line::from(Span::styled(line.to_string(), style)));
        }
        lines.push(Line::from(""));
    }
    
    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: true })
        .scroll((app.chat_scroll as u16, 0));
    
    f.render_widget(paragraph, area);
}

fn draw_input_area(f: &mut Frame, app: &App, area: Rect) {
    let input_style = if app.focus == Focus::Chat {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::Gray)
    };
    
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(input_style);
    
    let input_text = if app.chat_input.is_empty() {
        if matches!(app.state, AppState::Idle) {
            "Type your message..."
        } else {
            ""
        }
    } else {
        &app.chat_input
    };
    
    let paragraph = Paragraph::new(input_text)
        .block(input_block)
        .wrap(Wrap { trim: true });
    
    f.render_widget(paragraph, area);
}

fn draw_terminal_panel(f: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.focus == Focus::Terminal {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    
    let terminal_block = Block::default()
        .title(" Terminal ")
        .borders(Borders::ALL)
        .border_style(border_style);
    
    let inner = terminal_block.inner(area);
    f.render_widget(terminal_block, area);
    
    // Render terminal content
    let screen = app.terminal_parser.screen();
    let contents = screen.contents();
    
    let lines: Vec<Line> = contents
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect();
    
    let paragraph = Paragraph::new(Text::from(lines))
        .scroll((app.terminal_scroll as u16, 0));
    
    f.render_widget(paragraph, inner);
}

fn draw_jobs_panel(f: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.focus == Focus::Jobs {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    
    let jobs_block = Block::default()
        .title(" Jobs ")
        .borders(Borders::ALL)
        .border_style(border_style);
    
    let inner = jobs_block.inner(area);
    f.render_widget(jobs_block, area);
    
    let jobs = app.job_registry.list_all_jobs();
    let mut lines: Vec<Line> = Vec::new();
    
    for (idx, job) in jobs.iter().enumerate() {
        let status_icon = match job.status {
            JobStatus::Running => "⏳",
            JobStatus::Completed => "✅",
            JobStatus::Failed => "❌",
            JobStatus::Cancelled => "🛑",
            JobStatus::TimeoutPending => "⏱",
            JobStatus::Stalled => "⚠️",
        };
        
        let line = format!("{} {} - {}", status_icon, &job.id[..8.min(job.id.len())], job.description);
        let style = if Some(idx) == app.selected_job_index {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        
        lines.push(Line::from(Span::styled(line, style)));
    }
    
    if jobs.is_empty() {
        lines.push(Line::from("No active jobs"));
    }
    
    let paragraph = Paragraph::new(Text::from(lines));
    f.render_widget(paragraph, inner);
}

fn draw_help_overlay(f: &mut Frame, _app: &App, area: Rect) {
    let help_text = crate::tui::types::HelpSystem::generate_help_text(None, None);
    
    let popup_area = centered_rect(80, 80, area);
    
    let help_block = Block::default()
        .title(" Help (F1 to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    
    let paragraph = Paragraph::new(help_text)
        .block(help_block)
        .wrap(Wrap { trim: true });
    
    f.render_widget(Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

fn draw_memory_overlay(f: &mut Frame, _app: &App, area: Rect) {
    let popup_area = centered_rect(70, 70, area);
    
    let memory_block = Block::default()
        .title(" Memory (F3 to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    
    let text = Text::from("Memory graph visualization would appear here");
    let paragraph = Paragraph::new(text).block(memory_block);
    
    f.render_widget(Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

fn draw_status_line(f: &mut Frame, app: &App, area: Rect) {
    let status_text = match &app.state {
        AppState::Idle => {
            let stats = app.session_monitor.get_stats();
            format!("Cost: ${:.4} | Context: {}/{}", 
                stats.cost,
                stats.active_context_tokens,
                stats.max_context_tokens
            )
        }
        AppState::Thinking(msg) => msg.clone(),
        AppState::Streaming(msg) => msg.clone(),
        AppState::ExecutingTool(cmd) => format!("Executing: {}", cmd),
        AppState::AwaitingApproval { tool, .. } => format!("Approve: {}? (y/n)", tool),
        AppState::Error(e) => format!("Error: {}", e),
        AppState::ConfirmExit => "Confirm exit? (y/n)".to_string(),
        AppState::NamingSession => "Enter session name: ".to_string(),
        AppState::WaitingForUser => "Waiting for user...".to_string(),
    };
    
    let status_style = match &app.state {
        AppState::Idle => Style::default().fg(Color::Gray),
        AppState::Thinking(_) => Style::default().fg(Color::Yellow),
        AppState::Streaming(_) => Style::default().fg(Color::Cyan),
        AppState::Error(_) => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::White),
    };
    
    let status_line = Paragraph::new(status_text)
        .style(status_style)
        .alignment(Alignment::Left);
    
    let status_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    
    f.render_widget(status_line, status_area);
}

/// Helper to create a centered rectangle
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
