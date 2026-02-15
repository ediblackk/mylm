//! Event Loop Module
//!
//! Handles keyboard and mouse events for the TUI.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use crate::tui::app::state::{AppState, AppStateContainer, Focus};
use crate::tui::types::TimestampedChatMessage;

/// Handle key events
pub async fn handle_key_event(app: &mut AppStateContainer, key: KeyEvent) -> LoopAction {
    // Handle special states first
    match &app.state {
        AppState::AwaitingApproval { tool: _tool, .. } => {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some((intent_id, tool_name, _)) = app.pending_approval.take() {
                        if let Some(ref input_tx) = app.input_tx {
                            use mylm_core::agent::contract::session::UserInput;
                            use mylm_core::agent::contract::ids::IntentId;
                            let _ = input_tx.send(UserInput::Approval {
                                intent_id: IntentId::new(intent_id),
                                approved: true,
                            }).await;
                        }
                        // Add approval confirmation to chat
                        app.chat_history.push(TimestampedChatMessage::assistant(format!(
                            "✅ {} approved", tool_name
                        )));
                    }
                    app.state = AppState::Idle;
                    return LoopAction::Continue;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    if let Some((intent_id, tool_name, _)) = app.pending_approval.take() {
                        if let Some(ref input_tx) = app.input_tx {
                            use mylm_core::agent::contract::session::UserInput;
                            use mylm_core::agent::contract::ids::IntentId;
                            let _ = input_tx.send(UserInput::Approval {
                                intent_id: IntentId::new(intent_id),
                                approved: false,
                            }).await;
                        }
                        // Add denial confirmation to chat
                        app.chat_history.push(TimestampedChatMessage::assistant(format!(
                            "❌ {} cancelled", tool_name
                        )));
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
            let was_showing = app.show_memory_view;
            app.show_memory_view = !was_showing;
            
            // If turning on memory view, load memories
            if !was_showing {
                load_memory_graph(app).await;
            }
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
        // Memory view: real-time filter input
        KeyCode::Char(c) if app.show_memory_view => {
            app.memory_search_query.push(c);
            // Apply filter in real-time
            filter_memory_graph(app).await;
            return LoopAction::Continue;
        }
        KeyCode::Backspace if app.show_memory_view => {
            app.memory_search_query.pop();
            // Apply filter in real-time (or reload all if empty)
            if app.memory_search_query.is_empty() {
                load_memory_graph(app).await;
            } else {
                filter_memory_graph(app).await;
            }
            return LoopAction::Continue;
        }
        // Memory view navigation
        KeyCode::Up if app.show_memory_view => {
            if app.memory_graph_scroll > 0 {
                app.memory_graph_scroll -= 1;
            }
            return LoopAction::Continue;
        }
        KeyCode::Down if app.show_memory_view => {
            let max_scroll = app.memory_graph.nodes.len().saturating_sub(1);
            if app.memory_graph_scroll < max_scroll {
                app.memory_graph_scroll += 1;
            }
            return LoopAction::Continue;
        }
        KeyCode::PageUp if app.show_memory_view => {
            app.memory_graph_scroll = app.memory_graph_scroll.saturating_sub(10);
            return LoopAction::Continue;
        }
        KeyCode::PageDown if app.show_memory_view => {
            let max_scroll = app.memory_graph.nodes.len().saturating_sub(1);
            app.memory_graph_scroll = (app.memory_graph_scroll + 10).min(max_scroll);
            return LoopAction::Continue;
        }
        // Memory view: 'r' to reload memories
        KeyCode::Char('r') if app.show_memory_view => {
            app.memory_search_query.clear();
            load_memory_graph(app).await;
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

/// Load memories into the memory graph for display (F3 view)
/// 
/// This function is called when F3 is pressed to toggle the memory view.
/// It loads recent memories from the memory store and populates the graph.
async fn load_memory_graph(app: &mut AppStateContainer) {
    use mylm_core::memory::graph::{MemoryGraph, MemoryGraphNode};
    use mylm_core::agent::memory::AgentMemoryManager;
    use mylm_core::config::agent::MemoryConfig;
    
    // Check if memory feature is enabled in config
    // Note: app.config.features.memory is a bool in the store::Config type
    if !app.config.features.memory {
        mylm_core::debug_log!("[MEMORY_VIEW] Memory feature is disabled in config");
        app.memory_graph = MemoryGraph::default();
        return;
    }
    
    // Create memory config from the feature flag
    // TODO: In the future, we should use a unified MemoryConfig throughout the codebase
    let memory_config = MemoryConfig {
        enabled: true,
        ..MemoryConfig::default()
    };
    
    // Create memory manager from config
    // This is a temporary solution - in the future, the memory manager should be 
    // shared between the agent and the TUI
    let memory_manager = match AgentMemoryManager::new(memory_config).await {
        Ok(mm) => mm,
        Err(e) => {
            mylm_core::warn_log!("[MEMORY_VIEW] Failed to create memory manager: {}", e);
            app.memory_graph = MemoryGraph::default();
            return;
        }
    };
    
    // Get total memory count first
    let total_count = memory_manager.stats().await.map(|s| s.total_memories).unwrap_or(0);
    
    // Load recent memories - load up to 500 for display
    // This is a trade-off: more memories = slower UI, but users want to see all
    let limit = 500;
    let memories = match memory_manager.get_recent_memories(limit).await {
        Ok(m) => m,
        Err(e) => {
            mylm_core::warn_log!("[MEMORY_VIEW] Failed to load memories: {}", e);
            app.memory_graph = MemoryGraph::default();
            return;
        }
    };
    
    if memories.is_empty() {
        mylm_core::debug_log!("[MEMORY_VIEW] No memories found in store");
        app.memory_graph = MemoryGraph::default();
        return;
    }
    
    if memories.len() >= limit && total_count > limit {
        mylm_core::info_log!("[MEMORY_VIEW] Loaded {} memories ({} total in store - showing first {})", 
            memories.len(), total_count, limit);
    } else {
        mylm_core::info_log!("[MEMORY_VIEW] Loaded {} memories ({} total)", memories.len(), total_count);
    }
    
    // Build simple graph nodes from memories
    // For now, we don't compute connections - just display as list
    let nodes: Vec<MemoryGraphNode> = memories
        .into_iter()
        .map(|memory| MemoryGraphNode {
            memory,
            connections: Vec::new(),
        })
        .collect();
    
    app.memory_graph = MemoryGraph { nodes };
    app.memory_total_count = total_count;
}

/// Filter memory graph based on search query
/// 
/// This performs client-side filtering on the loaded memories.
/// For large memory stores, we might want to use semantic search instead.
async fn filter_memory_graph(app: &mut AppStateContainer) {
    use mylm_core::memory::graph::{MemoryGraph, MemoryGraphNode};
    
    let query = app.memory_search_query.to_lowercase();
    if query.is_empty() {
        // Reload all memories
        load_memory_graph(app).await;
        return;
    }
    
    mylm_core::info_log!("[MEMORY_VIEW] Filtering memories with query: '{}'", query);
    
    // Filter nodes that match the query
    let filtered_nodes: Vec<MemoryGraphNode> = app.memory_graph.nodes
        .iter()
        .filter(|node| {
            let content_match = node.memory.content.to_lowercase().contains(&query);
            let type_match = node.memory.r#type.to_string().to_lowercase().contains(&query);
            let category_match = node.memory.category_id
                .as_ref()
                .map(|c| c.to_lowercase().contains(&query))
                .unwrap_or(false);
            content_match || type_match || category_match
        })
        .cloned()
        .collect();
    
    let filtered_count = filtered_nodes.len();
    let total_loaded = app.memory_graph.nodes.len();
    
    mylm_core::info_log!("[MEMORY_VIEW] Filtered {} -> {} memories", total_loaded, filtered_count);
    
    // Replace with filtered graph
    app.memory_graph = MemoryGraph { nodes: filtered_nodes };
    app.memory_graph_scroll = 0; // Reset scroll to top
}

/// Action to take after handling an event
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopAction {
    /// Continue the event loop
    Continue,
    /// Break out of the event loop
    Break,
}
