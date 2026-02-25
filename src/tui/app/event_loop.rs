//! Event Loop Module
//!
//! Handles keyboard and mouse events for the TUI.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use crate::tui::app::state::{AppState, AppStateContainer, Focus};
use crate::tui::app::types::TimestampedChatMessage;
use mylm_core::memory::graph::MemoryGraph;

/// Handle key events
pub async fn handle_key_event(app: &mut AppStateContainer, key: KeyEvent) -> LoopAction {
    // Handle special states first
    match &app.state {
        AppState::AwaitingApproval { tool: _tool, .. } => {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // Get the pending approval with response channel first
                    if let Some(pending) = app.pending_approval_with_response.take() {
                        let tool_name = pending.request.tool.clone();
                        
                        // Send approval response through the oneshot channel (unblocks runtime)
                        use mylm_core::agent::types::events::ApprovalOutcome;
                        match pending.response_tx.send(ApprovalOutcome::Granted) {
                            Ok(_) => mylm_core::info_log!("[EVENT_LOOP] Approval response sent successfully"),
                            Err(_) => mylm_core::error_log!("[EVENT_LOOP] Failed to send approval - receiver dropped"),
                        }
                        
                        // Clear pending approval state (no need to send UserInput::Approval - 
                        // the runtime will return Observation::ApprovalCompleted which becomes
                        // KernelEvent::ApprovalGiven through the proper observation path)
                        let _ = app.pending_approval.take();
                        
                        // Add approval confirmation to chat
                        app.chat_history.push(TimestampedChatMessage::assistant(format!(
                            "✅ {} approved", tool_name
                        )));
                    }
                    app.state = AppState::Idle;
                    return LoopAction::Continue;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // Get the pending approval with response channel first
                    if let Some(pending) = app.pending_approval_with_response.take() {
                        let tool_name = pending.request.tool.clone();
                        
                        // Send denial response through the oneshot channel (unblocks runtime)
                        use mylm_core::agent::types::events::ApprovalOutcome;
                        match pending.response_tx.send(ApprovalOutcome::Denied { reason: Some("User denied".to_string()) }) {
                            Ok(_) => mylm_core::info_log!("[EVENT_LOOP] Denial response sent successfully"),
                            Err(_) => mylm_core::error_log!("[EVENT_LOOP] Failed to send denial - receiver dropped"),
                        }
                        
                        // Clear pending approval state (no need to send UserInput::Approval - 
                        // the runtime will return Observation::ApprovalCompleted which becomes
                        // KernelEvent::ApprovalGiven through the proper observation path)
                        let _ = app.pending_approval.take();
                        
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
        // Memory view: ACTION KEYS (must come BEFORE filter input)
        // 'r' to reload memories
        KeyCode::Char('r') if app.show_memory_view => {
            app.memory_search_query.clear();
            app.memory_current_page = 0;
            app.memory_graph_scroll = 0;
            load_memory_graph(app).await;
            return LoopAction::Continue;
        }
        // 'd' to delete selected memory
        KeyCode::Char('d') if app.show_memory_view => {
            delete_selected_memory(app).await;
            return LoopAction::Continue;
        }
        // 'D' (shift+d) to delete all filtered memories (bulk cleanup)
        KeyCode::Char('D') if app.show_memory_view => {
            delete_all_filtered_memories(app).await;
            return LoopAction::Continue;
        }
        // 's' to star/unstar selected memory
        KeyCode::Char('s') if app.show_memory_view => {
            star_selected_memory(app).await;
            return LoopAction::Continue;
        }
        // 'e' to export selected memory
        KeyCode::Char('e') if app.show_memory_view => {
            export_selected_memory(app);
            return LoopAction::Continue;
        }
        // Memory view: real-time filter input (lowercase letters only, not action keys)
        KeyCode::Char(c) if app.show_memory_view && c.is_lowercase() && !matches!(c, 'r' | 'd' | 's' | 'e') => {
            app.memory_search_query.push(c);
            filter_memory_graph(app).await;
            return LoopAction::Continue;
        }
        KeyCode::Char(c) if app.show_memory_view && (c.is_numeric() || c.is_uppercase() || matches!(c, '-' | '_' | ' ' | '.' | '/')) => {
            app.memory_search_query.push(c);
            filter_memory_graph(app).await;
            return LoopAction::Continue;
        }
        KeyCode::Backspace if app.show_memory_view => {
            app.memory_search_query.pop();
            if app.memory_search_query.is_empty() {
                load_memory_graph(app).await;
            } else {
                filter_memory_graph(app).await;
            }
            return LoopAction::Continue;
        }
        // Memory view: SHIFT+PageUp/Down for pagination
        KeyCode::PageUp if app.show_memory_view && key.modifiers.contains(KeyModifiers::SHIFT) => {
            if app.memory_current_page > 0 {
                app.memory_current_page -= 1;
                app.memory_graph_scroll = 0; // Reset scroll to top of new page
                load_memory_graph(app).await;
            }
            return LoopAction::Continue;
        }
        KeyCode::PageDown if app.show_memory_view && key.modifiers.contains(KeyModifiers::SHIFT) => {
            let total_pages = (app.memory_total_count + app.memory_page_size - 1) / app.memory_page_size;
            if app.memory_current_page + 1 < total_pages {
                app.memory_current_page += 1;
                app.memory_graph_scroll = 0; // Reset scroll to top of new page
                load_memory_graph(app).await;
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
            // Toggle auto-approve
            let current = app.auto_approve.load(std::sync::atomic::Ordering::SeqCst);
            app.auto_approve.store(!current, std::sync::atomic::Ordering::SeqCst);
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
            app.verbose_mode = !app.verbose_mode;
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
    // When job detail is shown, handle scrolling instead of selection
    if app.show_job_detail {
        match key.code {
            KeyCode::Up => {
                app.job_scroll = app.job_scroll.saturating_sub(1);
                LoopAction::Continue
            }
            KeyCode::Down => {
                app.job_scroll += 1;
                LoopAction::Continue
            }
            KeyCode::PageUp => {
                app.job_scroll = app.job_scroll.saturating_sub(10);
                LoopAction::Continue
            }
            KeyCode::PageDown => {
                app.job_scroll += 10;
                LoopAction::Continue
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                app.show_job_detail = false;
                app.job_scroll = 0; // Reset scroll when closing
                LoopAction::Continue
            }
            _ => LoopAction::Continue,
        }
    } else {
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
            // Shift+Up/Down scrolls the jobs list without changing selection
            KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                app.jobs_list_scroll = app.jobs_list_scroll.saturating_sub(1);
                LoopAction::Continue
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                app.jobs_list_scroll += 1;
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
/// Uses the shared memory manager from app state (initialized once at startup).
async fn load_memory_graph(app: &mut AppStateContainer) {
    use mylm_core::memory::graph::{MemoryGraph, MemoryGraphNode};
    
    // Check if memory feature is enabled in config
    if !app.config.features.memory {
        mylm_core::debug_log!("[MEMORY_VIEW] Memory feature is disabled in config");
        app.memory_graph = MemoryGraph::default();
        return;
    }
    
    // Use the shared memory manager from app state (initialized once)
    let memory_manager = match app.memory_manager.as_ref() {
        Some(mm) => mm,
        None => {
            mylm_core::warn_log!("[MEMORY_VIEW] Memory manager not available");
            app.memory_graph = MemoryGraph::default();
            return;
        }
    };
    
    // Get total memory count first
    let total_count = memory_manager.stats().await.map(|s| s.total_memories).unwrap_or(0);
    
    // Load recent memories with pagination
    let limit = app.memory_page_size;
    let offset = app.memory_current_page * app.memory_page_size;
    let memories = match memory_manager.get_recent_memories_with_offset(limit, offset).await {
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
    
    let showing_info = if total_count > limit {
        format!("page {} of ~{} ({} per page)", 
            app.memory_current_page + 1, 
            (total_count + limit - 1) / limit,
            limit)
    } else {
        format!("{} total", memories.len())
    };
    
    mylm_core::info_log!("[MEMORY_VIEW] Loaded {} memories ({})", memories.len(), showing_info);
    
    // Build simple graph nodes from memories
    let nodes: Vec<MemoryGraphNode> = memories
        .into_iter()
        .map(|memory| MemoryGraphNode {
            memory,
            connections: Vec::new(),
        })
        .collect();
    
    let graph = MemoryGraph { nodes };
    
    // Store both the display graph and the original for filtering
    app.memory_graph_original = Some(graph.clone());
    app.memory_graph = graph;
    app.memory_total_count = total_count;
    
    // Reset filter state
    app.memory_search_query.clear();
}

/// Filter memory graph based on search query
/// 
/// This performs client-side filtering on the ORIGINAL loaded memories,
/// preserving the full dataset so clearing the filter is instant (no DB reload).
async fn filter_memory_graph(app: &mut AppStateContainer) {
    use mylm_core::memory::graph::{MemoryGraph, MemoryGraphNode};
    
    let query = app.memory_search_query.to_lowercase();
    if query.is_empty() {
        // Clear filter: restore from original (no DB hit!)
        if let Some(original) = app.memory_graph_original.as_ref() {
            app.memory_graph = original.clone();
            mylm_core::debug_log!("[MEMORY_VIEW] Filter cleared, restored {} memories from cache", 
                app.memory_graph.nodes.len());
        }
        app.memory_graph_scroll = 0;
        return;
    }
    
    mylm_core::info_log!("[MEMORY_VIEW] Filtering memories with query: '{}'", query);
    
    // Get source: if we have original, filter from that; otherwise use current
    let source = app.memory_graph_original.as_ref()
        .unwrap_or(&app.memory_graph);
    
    // Filter nodes that match the query
    let filtered_nodes: Vec<MemoryGraphNode> = source.nodes
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
    let total_loaded = source.nodes.len();
    
    mylm_core::info_log!("[MEMORY_VIEW] Filtered {} -> {} memories", total_loaded, filtered_count);
    
    // Replace display graph with filtered results (original is preserved)
    app.memory_graph = MemoryGraph { nodes: filtered_nodes };
    app.memory_graph_scroll = 0; // Reset scroll to top
}

/// Delete the currently selected memory
async fn delete_selected_memory(app: &mut AppStateContainer) {
    if app.memory_graph.nodes.is_empty() {
        return;
    }
    
    let idx = app.memory_graph_scroll.clamp(0, app.memory_graph.nodes.len() - 1);
    let memory_id = app.memory_graph.nodes[idx].memory.id;
    
    // Get memory manager
    let Some(manager) = app.memory_manager.as_ref() else {
        mylm_core::warn_log!("[MEMORY_VIEW] Cannot delete: memory manager not available");
        return;
    };
    
    match manager.delete_memory(memory_id).await {
        Ok(_) => {
            mylm_core::info_log!("[MEMORY_VIEW] Deleted memory {}", memory_id);
            // Remove from local graph
            app.memory_graph.nodes.remove(idx);
            // Also remove from original if present
            if let Some(ref mut original) = app.memory_graph_original {
                original.nodes.retain(|n| n.memory.id != memory_id);
            }
            // Adjust scroll if needed
            if app.memory_graph_scroll >= app.memory_graph.nodes.len() && app.memory_graph_scroll > 0 {
                app.memory_graph_scroll -= 1;
            }
            app.memory_total_count = app.memory_total_count.saturating_sub(1);
        }
        Err(e) => {
            mylm_core::warn_log!("[MEMORY_VIEW] Failed to delete memory {}: {}", memory_id, e);
        }
    }
}

/// Star or unstar the currently selected memory
async fn star_selected_memory(app: &mut AppStateContainer) {
    if app.memory_graph.nodes.is_empty() {
        return;
    }
    
    let idx = app.memory_graph_scroll.clamp(0, app.memory_graph.nodes.len() - 1);
    let node = &mut app.memory_graph.nodes[idx];
    let memory_id = node.memory.id;
    
    // Toggle star by adding/removing "starred" category
    let is_starred = node.memory.category_id.as_ref() == Some(&"starred".to_string());
    let new_category = if is_starred { "" } else { "starred" };
    
    // Get memory manager
    let Some(manager) = app.memory_manager.as_ref() else {
        mylm_core::warn_log!("[MEMORY_VIEW] Cannot star: memory manager not available");
        return;
    };
    
    match manager.vector_store().update_memory_category(memory_id, new_category.to_string()).await {
        Ok(_) => {
            mylm_core::info_log!("[MEMORY_VIEW] {} memory {}", 
                if is_starred { "Unstarred" } else { "Starred" }, memory_id);
            // Update local copy
            node.memory.category_id = if is_starred { None } else { Some("starred".to_string()) };
            // Also update original if present
            if let Some(ref mut original) = app.memory_graph_original {
                if let Some(orig_node) = original.nodes.iter_mut().find(|n| n.memory.id == memory_id) {
                    orig_node.memory.category_id = node.memory.category_id.clone();
                }
            }
        }
        Err(e) => {
            mylm_core::warn_log!("[MEMORY_VIEW] Failed to star memory {}: {}", memory_id, e);
        }
    }
}

/// Export the currently selected memory to clipboard
fn export_selected_memory(app: &mut AppStateContainer) {
    if app.memory_graph.nodes.is_empty() {
        return;
    }
    
    let idx = app.memory_graph_scroll.clamp(0, app.memory_graph.nodes.len() - 1);
    let memory = &app.memory_graph.nodes[idx].memory;
    
    let export_text = format!(
        "Memory ID: {}\nType: {}\nTime: {}\n\nContent:\n{}",
        memory.id,
        memory.r#type,
        chrono::DateTime::from_timestamp(memory.created_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        memory.content
    );
    
    // Copy to clipboard if available
    if let Some(ref mut clipboard) = app.clipboard {
        if clipboard.set_text(export_text).is_ok() {
            mylm_core::info_log!("[MEMORY_VIEW] Exported memory {} to clipboard", memory.id);
        } else {
            mylm_core::warn_log!("[MEMORY_VIEW] Failed to copy to clipboard");
        }
    } else {
        // Fallback: print to log
        mylm_core::info_log!("[MEMORY_VIEW] Memory {} content:\n{}", memory.id, export_text);
    }
}

/// Delete ALL memories matching the current filter (bulk cleanup)
async fn delete_all_filtered_memories(app: &mut AppStateContainer) {
    if app.memory_graph.nodes.is_empty() {
        return;
    }
    
    let Some(manager) = app.memory_manager.as_ref() else {
        mylm_core::warn_log!("[MEMORY_VIEW] Cannot bulk delete: memory manager not available");
        return;
    };
    
    let count = app.memory_graph.nodes.len();
    mylm_core::info_log!("[MEMORY_VIEW] Bulk deleting {} memories", count);
    
    let mut deleted = 0;
    let mut failed = 0;
    
    // Delete all currently visible memories
    for node in &app.memory_graph.nodes {
        match manager.delete_memory(node.memory.id).await {
            Ok(_) => deleted += 1,
            Err(_) => failed += 1,
        }
    }
    
    mylm_core::info_log!("[MEMORY_VIEW] Bulk delete complete: {} deleted, {} failed", deleted, failed);
    
    // Clear the graph and reload
    app.memory_graph = MemoryGraph::default();
    app.memory_graph_original = None;
    app.memory_graph_scroll = 0;
    app.memory_total_count = app.memory_total_count.saturating_sub(deleted);
}

/// Action to take after handling an event
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopAction {
    /// Continue the event loop
    Continue,
    /// Break out of the event loop
    Break,
}
