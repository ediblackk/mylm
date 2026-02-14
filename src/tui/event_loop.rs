//! Event Loop Module
//!
//! Handles keyboard and mouse events for the TUI.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use crate::tui::app::state::{AppState, AppStateContainer, Focus};

/// Handle key events
pub async fn handle_key_event(app: &mut AppStateContainer, key: KeyEvent) -> LoopAction {
    // Handle special states first
    match &app.state {
        AppState::AwaitingApproval { tool: _, .. } => {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(tx) = app.pending_approval_tx.take() {
                        let _ = tx.send(true);
                    }
                    app.state = AppState::Idle;
                    return LoopAction::Continue;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    if let Some(tx) = app.pending_approval_tx.take() {
                        let _ = tx.send(false);
                    }
                    app.set_state(AppState::Idle);
                    return LoopAction::Continue;
                }
                _ => return LoopAction::Continue,
            }
        }
        AppState::ConfirmExit => {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    app.should_quit = true;
                    return LoopAction::Break;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    app.set_state(AppState::Idle);
                    return LoopAction::Continue;
                }
                _ => return LoopAction::Continue,
            }
        }
        AppState::NamingSession => {
            match key.code {
                KeyCode::Enter => {
                    app.exit_name_input = app.chat_input.clone();
                    app.chat_input.clear();
                    app.set_state(AppState::Idle);
                    app.should_quit = true;
                    return LoopAction::Break;
                }
                KeyCode::Esc => {
                    app.chat_input.clear();
                    app.set_state(AppState::Idle);
                    return LoopAction::Continue;
                }
                KeyCode::Char(c) => {
                    app.enter_char(c);
                    return LoopAction::Continue;
                }
                KeyCode::Backspace => {
                    app.delete_char();
                    return LoopAction::Continue;
                }
                _ => return LoopAction::Continue,
            }
        }
        _ => {}
    }
    
    // Global shortcuts
    match key.code {
        KeyCode::F(1) => {
            app.show_help_view = !app.show_help_view;
            return LoopAction::Continue;
        }
        KeyCode::F(2) => {
            app.toggle_focus();
            return LoopAction::Continue;
        }
        KeyCode::F(3) => {
            app.show_memory_view = !app.show_memory_view;
            return LoopAction::Continue;
        }
        KeyCode::F(4) => {
            app.show_jobs_panel = !app.show_jobs_panel;
            if !app.show_jobs_panel && app.focus == Focus::Jobs {
                app.focus = Focus::Terminal;
            }
            return LoopAction::Continue;
        }
        KeyCode::Esc => {
            if app.show_help_view {
                app.show_help_view = false;
                return LoopAction::Continue;
            }
            if app.show_memory_view {
                app.show_memory_view = false;
                return LoopAction::Continue;
            }
            if app.show_jobs_panel {
                app.show_jobs_panel = false;
                return LoopAction::Continue;
            }
            app.set_state(AppState::ConfirmExit);
            return LoopAction::Continue;
        }
        _ => {}
    }
    
    // Focus-specific handling
    match app.focus {
        Focus::Chat => handle_chat_focus(app, key).await,
        Focus::Terminal => handle_terminal_focus(app, key),
        Focus::Jobs => handle_jobs_focus(app, key),
    }
}

async fn handle_chat_focus(app: &mut AppStateContainer, key: KeyEvent) -> LoopAction {
    match key.code {
        KeyCode::Enter => {
            mylm_core::info_log!("[EVENT_LOOP] Enter pressed in chat focus");
            if !app.chat_input.is_empty() {
                mylm_core::debug_log!("[EVENT_LOOP] Input not empty, calling submit_message");
                // Create a dummy event sender since we're handling directly
                let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
                app.submit_message(tx).await;
            } else {
                mylm_core::debug_log!("[EVENT_LOOP] Input is empty, ignoring Enter");
            }
            LoopAction::Continue
        }
        // Control key shortcuts must come before KeyCode::Char(c)
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_cursor_home();
            LoopAction::Continue
        }
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_cursor_end();
            LoopAction::Continue
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.abort_current_task();
            LoopAction::Continue
        }
        KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.copy_terminal_buffer_to_clipboard();
            LoopAction::Continue
        }
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.copy_last_ai_response_to_clipboard();
            LoopAction::Continue
        }
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.copy_visible_conversation_to_clipboard();
            LoopAction::Continue
        }
        KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.adjust_chat_width(-5);
            LoopAction::Continue
        }
        KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.adjust_chat_width(5);
            LoopAction::Continue
        }
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Kill to end of line
            let pos = app.cursor_position;
            let char_count = app.chat_input.chars().count();
            if pos < char_count {
                let byte_start = app.chat_input.char_indices().nth(pos).map(|(i, _)| i).unwrap_or(0);
                app.chat_input.truncate(byte_start);
            }
            LoopAction::Continue
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Kill to start of line
            let pos = app.cursor_position;
            if pos > 0 {
                let byte_end = app.chat_input.char_indices().nth(pos).map(|(i, _)| i).unwrap_or(app.chat_input.len());
                app.chat_input = app.chat_input[byte_end..].to_string();
                app.cursor_position = 0;
            }
            LoopAction::Continue
        }
        KeyCode::Char(c) => {
            app.enter_char(c);
            LoopAction::Continue
        }
        KeyCode::Backspace => {
            app.delete_char();
            LoopAction::Continue
        }
        KeyCode::Delete => {
            app.delete_at_cursor();
            LoopAction::Continue
        }
        KeyCode::Left => {
            app.move_cursor_left();
            LoopAction::Continue
        }
        KeyCode::Right => {
            app.move_cursor_right();
            LoopAction::Continue
        }
        KeyCode::Home => {
            app.move_cursor_home();
            LoopAction::Continue
        }
        KeyCode::End => {
            app.move_cursor_end();
            LoopAction::Continue
        }
        KeyCode::Up => {
            app.scroll_chat_up();
            LoopAction::Continue
        }
        KeyCode::Down => {
            app.scroll_chat_down();
            LoopAction::Continue
        }
        _ => LoopAction::Continue,
    }
}

fn handle_terminal_focus(app: &mut AppStateContainer, key: KeyEvent) -> LoopAction {
    match key.code {
        KeyCode::Up => {
            app.scroll_terminal_up();
            LoopAction::Continue
        }
        KeyCode::Down => {
            app.scroll_terminal_down();
            LoopAction::Continue
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.handle_terminal_input(&[3]); // Ctrl+C
            LoopAction::Continue
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.handle_terminal_input(&[4]); // Ctrl+D
            LoopAction::Continue
        }
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.handle_terminal_input(&[12]); // Ctrl+L (clear)
            LoopAction::Continue
        }
        KeyCode::Char(c) => {
            app.handle_terminal_input(c.to_string().as_bytes());
            LoopAction::Continue
        }
        KeyCode::Enter => {
            app.handle_terminal_input(b"\r");
            LoopAction::Continue
        }
        KeyCode::Backspace => {
            app.handle_terminal_input(&[127]);
            LoopAction::Continue
        }
        KeyCode::Tab => {
            app.handle_terminal_input(b"\t");
            LoopAction::Continue
        }
        _ => LoopAction::Continue,
    }
}

fn handle_jobs_focus(app: &mut AppStateContainer, key: KeyEvent) -> LoopAction {
    match key.code {
        KeyCode::Up => {
            if let Some(idx) = app.selected_job_index {
                if idx > 0 {
                    app.selected_job_index = Some(idx - 1);
                }
            } else {
                let jobs = app.job_registry.list_all_jobs();
                if !jobs.is_empty() {
                    app.selected_job_index = Some(0);
                }
            }
            LoopAction::Continue
        }
        KeyCode::Down => {
            let jobs = app.job_registry.list_all_jobs();
            if let Some(idx) = app.selected_job_index {
                if idx + 1 < jobs.len() {
                    app.selected_job_index = Some(idx + 1);
                }
            } else if !jobs.is_empty() {
                app.selected_job_index = Some(0);
            }
            LoopAction::Continue
        }
        KeyCode::Enter => {
            app.show_job_detail = true;
            LoopAction::Continue
        }
        KeyCode::Delete | KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(idx) = app.selected_job_index {
                let jobs = app.job_registry.list_all_jobs();
                if let Some(job) = jobs.get(idx) {
                    app.job_registry.cancel_job(&job.id);
                }
            }
            LoopAction::Continue
        }
        _ => LoopAction::Continue,
    }
}

/// Handle mouse events
pub fn handle_mouse_event(app: &mut AppStateContainer, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::Down(_) => {
            // Determine which pane was clicked
            // This is simplified - in reality we'd need to track pane areas
        }
        MouseEventKind::Drag(_) => {
            if app.is_selecting {
                app.update_selection(mouse.column, mouse.row);
            }
        }
        MouseEventKind::Up(_) => {
            if app.is_selecting {
                app.end_selection();
            }
        }
        MouseEventKind::ScrollDown => {
            match app.focus {
                Focus::Chat => app.scroll_chat_up(),
                Focus::Terminal => app.scroll_terminal_up(),
                _ => {}
            }
        }
        MouseEventKind::ScrollUp => {
            match app.focus {
                Focus::Chat => app.scroll_chat_down(),
                Focus::Terminal => app.scroll_terminal_down(),
                _ => {}
            }
        }
        _ => {}
    }
}

/// Action to take after handling an event
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopAction {
    /// Continue the event loop
    Continue,
    /// Break out of the event loop
    Break,
}
