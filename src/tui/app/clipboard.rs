//! Clipboard operations
use crate::tui::app::state::AppStateContainer;
use mylm_core::llm::chat::MessageRole;

impl AppStateContainer {
    pub fn copy_text_to_clipboard(&mut self, text: String) {
        if let Some(clipboard) = &mut self.clipboard {
            if clipboard.set_text(text.clone()).is_ok() {
                self.status_message = Some("Copied to clipboard".into());
                return;
            }
        }

        let path = "/tmp/mylm-clipboard.txt";
        match std::fs::write(path, &text) {
            Ok(_) => {
                self.status_message = Some(format!("Clipboard unavailable; wrote to {}", path));
            }
            Err(e) => {
                self.status_message = Some(format!("Clipboard error & file write failed: {}", e));
            }
        }
    }

    pub fn copy_last_ai_response_to_clipboard(&mut self) {
        if let Some(msg) = self
            .chat_history
            .iter()
            .rev()
            .find(|m| m.message.role == MessageRole::Assistant)
        {
            self.copy_text_to_clipboard(msg.message.content.clone());
        } else {
            self.status_message = Some("⚠️ No AI response to copy".to_string());
        }
    }

    pub fn copy_terminal_buffer_to_clipboard(&mut self) {
        let history_height = 5000;
        let width = self.terminal_size.1;
        let mut temp_parser = vt100::Parser::new(history_height, width, 0);
        temp_parser.process(&self.raw_buffer);
        let content = temp_parser.screen().contents();
        self.copy_text_to_clipboard(content);
    }

    pub fn copy_visible_conversation_to_clipboard(&mut self) {
        let mut transcript = String::new();
        for msg in &self.chat_history {
            match msg.message.role {
                MessageRole::User => {
                    if !transcript.is_empty() {
                        transcript.push_str("\n\n");
                    }
                    transcript.push_str("User: ");
                    transcript.push_str(&msg.message.content);
                }
                MessageRole::Assistant => {
                    if !transcript.is_empty() {
                        transcript.push_str("\n\n");
                    }
                    transcript.push_str("AI: ");
                    transcript.push_str(&msg.message.content);
                }
                _ => {}
            }
        }
        if transcript.is_empty() {
            self.status_message = Some("⚠️ No conversation to copy".to_string());
        } else {
            self.copy_text_to_clipboard(transcript);
        }
    }
}
