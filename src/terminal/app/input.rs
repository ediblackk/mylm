//! Input handling - cursor movement, text editing, and selection
use crate::terminal::app::state::{AppStateContainer, Focus, PendingStream};
use mylm_core::llm::TokenUsage;
use std::time::Instant;

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
        let result = self.get_selected_text();
        self.selection_start = None;
        self.selection_end = None;
        self.selection_pane = None;
        result
    }

    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.selection_pane = None;
        self.is_selecting = false;
    }

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

    fn get_selected_text(&self) -> Option<String> {
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
        _start_x: u16,
        _start_y: u16,
        _end_x: u16,
        _end_y: u16,
    ) -> Option<String> {
        let mut all_lines = Vec::new();
        for m in &self.chat_history {
            let prefix = match m.role {
                mylm_core::llm::chat::MessageRole::User => "You: ",
                mylm_core::llm::chat::MessageRole::Assistant => "AI: ",
                mylm_core::llm::chat::MessageRole::System => "Sys: ",
                _ => "AI: ",
            };
            all_lines.push(format!("{}{}", prefix, m.content));
        }

        if all_lines.is_empty() {
            return None;
        }

        Some(all_lines.join("\n\n"))
    }

    // Streaming
    pub async fn start_streaming_final_answer(&mut self, content: String, usage: TokenUsage) {
        self.chat_history
            .push(mylm_core::llm::chat::ChatMessage::assistant(String::new()));

        if !self.incognito {
            let session = self.build_current_session().await;
            self.session_manager.set_current_session(session);
        }

        let msg_index = self.chat_history.len().saturating_sub(1);
        self.pending_stream = Some(PendingStream {
            started_at: Instant::now(),
            chars: content.chars().collect(),
            rendered: 0,
            msg_index,
            usage,
        });
        self.set_state(mylm_core::terminal::app::AppState::Streaming("Answer".to_string()));
    }

    // History management
    pub fn set_history(&mut self, history: Vec<mylm_core::llm::chat::ChatMessage>) {
        self.chat_history = history;
    }

    pub fn add_assistant_message(&mut self, content: String, usage: TokenUsage) {
        use mylm_core::llm::chat::ChatMessage;
        self.chat_history.push(ChatMessage::assistant(content));

        let input_price = self.input_price;
        let output_price = self.output_price;

        self.session_monitor.add_usage(&usage, input_price, output_price);

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
    pub async fn build_current_session(&self) -> crate::terminal::session::Session {
        use mylm_core::llm::chat::MessageRole;
        let stats = self.session_monitor.get_stats();
        let preview = self
            .chat_history
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant)
            .map(|m| m.content.chars().take(100).collect::<String>())
            .unwrap_or_else(|| "New Session".to_string());

        let agent = self.agent.lock().await;

        crate::terminal::session::Session {
            id: self.session_id.clone(),
            timestamp: chrono::Utc::now(),
            history: self.chat_history.clone(),
            metadata: crate::terminal::session::SessionMetadata {
                last_message_preview: preview,
                message_count: self.chat_history.len(),
                total_tokens: stats.total_tokens,
                input_tokens: stats.input_tokens,
                output_tokens: stats.output_tokens,
                cost: stats.cost,
                elapsed_seconds: self.session_monitor.duration().as_secs(),
            },
            terminal_history: self.raw_buffer.clone(),
            agent_session_id: agent.session_id.clone(),
            agent_history: agent.history.clone(),
        }
    }
}
