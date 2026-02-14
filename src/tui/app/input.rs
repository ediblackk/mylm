//! Input handling - cursor movement, text editing, and selection
use crate::tui::app::state::{AppStateContainer, Focus};
use mylm_core::llm::TokenUsage;

impl AppStateContainer {
    // Cursor movement
    pub fn move_cursor_left(&mut self) {
        self.cursor_position = self.cursor_position.saturating_sub(1);
    }

    pub fn move_cursor_right(&mut self) {
        let char_count = self.chat_input.chars().count();
        if self.cursor_position < char_count {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.chat_input.chars().count();
    }

    pub fn reset_cursor(&mut self) {
        self.cursor_position = 0;
    }

    // Text input
    pub fn enter_char(&mut self, new_char: char) {
        if new_char == '\r' {
            return;
        }

        if self.cursor_position >= self.chat_input.chars().count() {
            self.chat_input.push(new_char);
        } else {
            let byte_idx = self
                .chat_input
                .char_indices()
                .nth(self.cursor_position)
                .map(|(i, _)| i)
                .unwrap_or(self.chat_input.len());
            self.chat_input.insert(byte_idx, new_char);
        }
        self.cursor_position += 1;
    }

    #[allow(dead_code)]
    pub fn enter_string(&mut self, text: &str) {
        let clean_text = text.replace('\r', "");
        if clean_text.is_empty() {
            return;
        }

        // Large paste warning
        if clean_text.len() > 10_000 {
            self.status_message = Some(
                "⚠️ Large paste detected. Consider using /read or asking AI to read the file for efficiency.".to_string(),
            );
        }

        if self.cursor_position >= self.chat_input.chars().count() {
            self.chat_input.push_str(&clean_text);
        } else {
            let byte_idx = self
                .chat_input
                .char_indices()
                .nth(self.cursor_position)
                .map(|(i, _)| i)
                .unwrap_or(self.chat_input.len());
            self.chat_input.insert_str(byte_idx, &clean_text);
        }

        self.cursor_position += clean_text.chars().count();
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            let mut chars: Vec<char> = self.chat_input.chars().collect();
            chars.remove(self.cursor_position - 1);
            self.chat_input = chars.into_iter().collect();
            self.move_cursor_left();
        }
    }

    pub fn delete_at_cursor(&mut self) {
        let char_count = self.chat_input.chars().count();
        if self.cursor_position < char_count {
            let mut chars: Vec<char> = self.chat_input.chars().collect();
            chars.remove(self.cursor_position);
            self.chat_input = chars.into_iter().collect();
        }
    }

    // Scrolling
    pub fn scroll_chat_up(&mut self) {
        self.chat_scroll = self.chat_scroll.saturating_add(1);
        self.chat_auto_scroll = false;
    }

    pub fn scroll_chat_down(&mut self) {
        self.chat_scroll = self.chat_scroll.saturating_sub(1);
        if self.chat_scroll == 0 {
            self.chat_auto_scroll = true;
        }
    }

    pub fn scroll_terminal_up(&mut self) {
        self.terminal_scroll = self.terminal_scroll.saturating_add(1);
        self.terminal_auto_scroll = false;
    }

    pub fn scroll_terminal_down(&mut self) {
        self.terminal_scroll = self.terminal_scroll.saturating_sub(1);
        if self.terminal_scroll == 0 {
            self.terminal_auto_scroll = true;
        }
    }

    // Layout
    pub fn adjust_chat_width(&mut self, delta: i16) {
        let new_width = self.chat_width_percent as i16 + delta;
        self.chat_width_percent = new_width.clamp(20, 100) as u16;
    }

    // Selection
    #[allow(dead_code)]
    pub fn start_selection(&mut self, x: u16, y: u16, pane: Focus) {
        self.selection_start = Some((x, y));
        self.selection_end = Some((x, y));
        self.selection_pane = Some(pane);
        self.is_selecting = true;
    }

    pub fn update_selection(&mut self, x: u16, y: u16) {
        if self.is_selecting {
            self.selection_end = Some((x, y));
        }
    }

    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        self.get_selected_text()
        // Note: We intentionally do NOT clear selection_start/selection_end here
        // so the visual highlight persists until the user starts a new selection
        // or explicitly clears it (e.g., pressing Escape or after copy/paste)
    }

    #[allow(dead_code)]
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.selection_pane = None;
        self.is_selecting = false;
    }

    #[allow(dead_code)]
    pub fn is_in_selection(&self, x: u16, y: u16, pane: Focus) -> bool {
        let (start, end, p) = match (self.selection_start, self.selection_end, self.selection_pane) {
            (Some(s), Some(e), Some(p)) => (s, e, p),
            _ => return false,
        };

        if p != pane {
            return false;
        }

        let (x1, y1) = start;
        let (x2, y2) = end;
        let ((min_x, min_y), (max_x, max_y)) = if y1 < y2 || (y1 == y2 && x1 <= x2) {
            ((x1, y1), (x2, y2))
        } else {
            ((x2, y2), (x1, y1))
        };

        if y < min_y || y > max_y {
            return false;
        }

        if y == min_y && y == max_y {
            return x >= min_x && x <= max_x;
        }

        if y == min_y {
            return x >= min_x;
        }

        if y == max_y {
            return x <= max_x;
        }

        true
    }

    pub fn get_selected_text(&self) -> Option<String> {
        let (start, end, pane) =
            match (self.selection_start, self.selection_end, self.selection_pane) {
                (Some(s), Some(e), Some(p)) => (s, e, p),
                _ => return None,
            };

        let (x1, y1) = start;
        let (x2, y2) = end;
        let ((start_x, start_y), (end_x, end_y)) = if y1 < y2 || (y1 == y2 && x1 <= x2) {
            ((x1, y1), (x2, y2))
        } else {
            ((x2, y2), (x1, y1))
        };

        match pane {
            Focus::Terminal => self.get_terminal_selected_text(start_x, start_y, end_x, end_y),
            Focus::Chat => self.get_chat_selected_text(start_x, start_y, end_x, end_y),
            Focus::Jobs => None, // Jobs panel doesn't have selectable text
        }
    }

    fn get_terminal_selected_text(
        &self,
        start_x: u16,
        start_y: u16,
        end_x: u16,
        end_y: u16,
    ) -> Option<String> {
        let (offset_x, offset_y) = self.terminal_area_offset.unwrap_or((0, 0));
        let (screen_rows, _screen_cols) = self.terminal_parser.screen().size();

        let mut all_lines = Vec::new();
        for h in &self.terminal_history {
            all_lines.push(h.clone());
        }
        let screen_contents = self.terminal_parser.screen().contents();
        for s in screen_contents.split('\n') {
            all_lines.push(s.to_string());
        }

        let total_lines = all_lines.len();
        let height = screen_rows as usize;

        let start_idx = if self.terminal_auto_scroll {
            total_lines.saturating_sub(height)
        } else {
            let max_scroll = total_lines.saturating_sub(height);
            let clamped_scroll = self.terminal_scroll.min(max_scroll);
            total_lines.saturating_sub(clamped_scroll).saturating_sub(height)
        };

        let mut lines = Vec::new();
        for y in start_y..=end_y {
            let abs_y = start_idx + (y.saturating_sub(offset_y) as usize).saturating_sub(1);
            if let Some(line) = all_lines.get(abs_y) {
                let col_start = if y == start_y {
                    start_x.saturating_sub(offset_x).saturating_sub(1) as usize
                } else {
                    0
                };
                let col_end = if y == end_y {
                    (end_x.saturating_sub(offset_x).saturating_sub(1) as usize).min(line.chars().count())
                } else {
                    line.chars().count()
                };

                let chars: Vec<char> = line.chars().collect();
                if col_start < chars.len() {
                    let part: String = chars[col_start..col_end.min(chars.len())].iter().collect();
                    lines.push(part);
                } else if col_start == 0 && chars.is_empty() {
                    lines.push(String::new());
                }
            }
        }

        Some(lines.join("\n"))
    }

    fn get_chat_selected_text(
        &self,
        start_x: u16,
        start_y: u16,
        end_x: u16,
        end_y: u16,
    ) -> Option<String> {
        mylm_core::info_log!("get_chat_selected_text: start=({}, {}), end=({}, {})", start_x, start_y, end_x, end_y);
        // Normalize selection coordinates
        let (start_x, start_y, end_x, end_y) = if start_y < end_y || (start_y == end_y && start_x <= end_x) {
            (start_x, start_y, end_x, end_y)
        } else {
            (end_x, end_y, start_x, start_y)
        };

        let start_col = self.chat_history_start_col.unwrap_or(0);
        let area_y = self.chat_area_offset.map(|(_, y)| y).unwrap_or(0);
        mylm_core::info_log!("get_chat_selected_text: start_col={}, area_y={}, visible_start_idx={}", start_col, area_y, self.chat_visible_start_idx);

        // Collect visual lines that fall within the vertical selection range
        let mut selected_lines = Vec::new();
        for (line_text, abs_row) in &self.chat_visual_lines {
            // Skip lines not in visible window
            if *abs_row < self.chat_visible_start_idx {
                continue;
            }
            if *abs_row >= self.chat_visible_end_idx {
                break; // Beyond visible area
            }
            // Compute the screen row for this visual line: area_y + 1 + (abs_row - visible_start_idx)
            let screen_row = area_y + 1 + (*abs_row as u16 - self.chat_visible_start_idx as u16);
            if screen_row >= start_y && screen_row <= end_y {
                selected_lines.push((line_text, *abs_row));
                mylm_core::info_log!("get_chat_selected_text: matched line abs_row={}, text={:?}", abs_row, line_text);
            }
        }

        if selected_lines.is_empty() {
            return None;
        }

        let mut result_parts = Vec::new();
        let total_selected = selected_lines.len();

        for (i, (line_text, _)) in selected_lines.iter().enumerate() {
            let is_first = i == 0;
            let is_last = i == total_selected - 1;

            let line_len = line_text.chars().count();
            let line_end_col = start_col + line_len as u16; // exclusive column after last char

            // Check horizontal intersection for first/last lines
            if is_first && start_x >= line_end_col {
                continue; // selection starts after line ends
            }
            if is_last && end_x < start_col {
                continue; // selection ends before line starts
            }

            // Determine character range to extract (by char index)
            let char_start = if is_first {
                let s = start_x as i16 - start_col as i16;
                if s < 0 { 0 } else { s as usize }
            } else {
                0
            };

            let char_end = if is_last {
                let e = end_x as i16 - start_col as i16 + 1; // +1 because end_x inclusive
                if e < 0 {
                    0
                } else {
                    (e as usize).min(line_len)
                }
            } else {
                line_len
            };

            if char_start >= char_end {
                // No characters selected in this line
                result_parts.push(String::new());
                continue;
            }

            // Extract substring by character indices
            let chars: Vec<char> = line_text.chars().collect();
            let selected: String = chars[char_start..char_end].iter().collect();
            result_parts.push(selected);
        }

        // If all parts are empty, return None
        if result_parts.iter().all(|s| s.is_empty()) {
            return None;
        }

        Some(result_parts.join("\n"))
    }

    // Streaming
    #[allow(dead_code)]
    pub async fn start_streaming_final_answer(&mut self, _content: String, _usage: TokenUsage) {
        self.chat_history
            .push(mylm_core::llm::chat::ChatMessage::assistant(String::new()));

        if !self.incognito {
            let session = self.build_current_session().await;
            self.session_manager.set_current_session(session);
        }

        // Real streaming - no typewriter, content already streaming to UI
        self.set_state(crate::tui::app::state::AppState::Idle);
    }

    // History management
    #[allow(dead_code)]
    pub fn set_history(&mut self, history: Vec<mylm_core::llm::chat::ChatMessage>) {
        self.chat_history = history;
    }

    #[allow(dead_code)]
    pub fn add_assistant_message(&mut self, content: String, usage: TokenUsage) {
        use mylm_core::llm::chat::ChatMessage;
        self.chat_history.push(ChatMessage::assistant(content.clone()));

        let input_price = self.input_price;
        let output_price = self.output_price;

        self.session_monitor.add_usage(&usage, input_price, output_price);
        
        // Update context manager with new message for token tracking
        self.context_manager.set_history(&self.chat_history);

        if self.chat_auto_scroll {
            self.chat_scroll = 0;
        }
    }

    #[allow(dead_code)]
    pub fn add_system_message(&mut self, content: &str) {
        use mylm_core::llm::chat::ChatMessage;
        self.chat_history.push(ChatMessage::system(content.to_string()));
        
        // Update context manager with new message for token tracking
        self.context_manager.set_history(&self.chat_history);

        if self.chat_auto_scroll {
            self.chat_scroll = 0;
        }
    }

    /// Add assistant message without token usage (for simple UI messages)
    #[allow(dead_code)]
    pub fn add_assistant_message_simple(&mut self, content: &str) {
        use mylm_core::llm::chat::ChatMessage;
        self.chat_history.push(ChatMessage::assistant(content.to_string()));
        
        // Update context manager with new message for token tracking
        self.context_manager.set_history(&self.chat_history);

        if self.chat_auto_scroll {
            self.chat_scroll = 0;
        }
    }

    pub fn handle_terminal_input(&mut self, bytes: &[u8]) {
        let _ = self.pty_manager.write_all(bytes);
        self.terminal_scroll = 0;
        self.terminal_auto_scroll = true;
    }

    /// Build a Session object from current state
    pub async fn build_current_session(&self) -> crate::tui::session::Session {
        use mylm_core::llm::chat::MessageRole;
        let stats = self.session_monitor.get_stats();
        let preview = self
            .chat_history
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant)
            .map(|m| m.content.chars().take(100).collect::<String>())
            .unwrap_or_else(|| "New Session".to_string());

        crate::tui::session::Session {
            id: self.session_id.clone(),
            timestamp: chrono::Utc::now(),
            history: self.chat_history.clone(),
            metadata: crate::tui::session::SessionMetadata {
                last_message_preview: preview,
                message_count: self.chat_history.len(),
                total_tokens: stats.total_tokens as u32,
                input_tokens: stats.input_tokens as u32,
                output_tokens: stats.output_tokens as u32,
                cost: stats.cost,
                elapsed_seconds: self.session_monitor.duration().as_secs(),
            },
            terminal_history: self.raw_buffer.clone(),
            agent_session_id: String::new(), // No legacy agent in new architecture
            agent_history: Vec::new(), // No legacy agent in new architecture
        }
    }
}
