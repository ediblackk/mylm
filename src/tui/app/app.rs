//! Terminal Application - Main orchestration module
//!
//! This module has been refactored into focused submodules:
//! - `app/state.rs` - State container and core state management
//! - `app/input.rs` - Input handling (cursor, text editing, selection)
//! - `app/commands.rs` - Slash command processing
//! - `app/clipboard.rs` - Clipboard operations
//! - `app/session.rs` - Session persistence

use crate::tui::app::state::AppStateContainer;
use crate::tui::types::{AppState, TuiEvent, TimestampedChatMessage};

use mylm_core::agent::contract::session::UserInput;
use mylm_core::context::pack::ContextBuilder;
use mylm_core::context::SmartPruning;
use mylm_core::llm::chat::ChatMessage;
use tokio::sync::mpsc::UnboundedSender;

pub use crate::tui::app::state::AppStateContainer as App;

impl AppStateContainer {
    /// Submit a message to the agent for processing
    pub async fn submit_message(&mut self, _event_tx: UnboundedSender<TuiEvent>) {
        mylm_core::info_log!("[APP] submit_message called");
        
        if self.chat_input.is_empty() {
            mylm_core::debug_log!("[APP] submit_message: input is empty, returning");
            return;
        }
        
        mylm_core::debug_log!("[APP] Input channel status: input_tx.is_some()={}", self.input_tx.is_some());
        
        self.abort_current_task();
        self.status_message = None;
        let input = self.chat_input.clone();
        
        // Check for auto-restore: does user message reference pruned content?
        let auto_restore_result = self.context_manager.check_auto_restore(&input);
        if auto_restore_result.found {
            let segment_count = auto_restore_result.segments.len();
            let keywords = auto_restore_result.keywords.join(", ");
            
            mylm_core::info_log!(
                "[APP] Auto-restore triggered: {} segments matched keywords: {}",
                segment_count, keywords
            );
            
            // Show "Remembering..." message
            self.chat_history.push(TimestampedChatMessage::assistant(
                format!("Remembering... (restoring {} context segment{})", 
                    segment_count, 
                    if segment_count > 1 { "s" } else { "" }
                )
            ));
            
            // Restore the segments to both context_manager and chat_history
            for segment in auto_restore_result.segments {
                for msg in segment.messages {
                    let chat_msg = match msg.role {
                        mylm_core::agent::cognition::history::MessageRole::User => {
                            TimestampedChatMessage::user(&msg.content)
                        }
                        mylm_core::agent::cognition::history::MessageRole::Assistant => {
                            TimestampedChatMessage::assistant(&msg.content)
                        }
                        mylm_core::agent::cognition::history::MessageRole::System => {
                            TimestampedChatMessage::system(&msg.content)
                        }
                        mylm_core::agent::cognition::history::MessageRole::Tool => {
                            TimestampedChatMessage::assistant(format!("🔧 Tool: {}", &msg.content))
                        }
                    };
                    self.chat_history.push(chat_msg);
                }
            }
            
            // Update context_manager with restored history
            let chat_msgs: Vec<ChatMessage> = self.chat_history.iter().map(|m| m.message.clone()).collect();
            self.context_manager.set_history(&chat_msgs);
            
            // Remove the pruned segments from the archive since they're now restored
            // (The segments are consumed from the iterator above, but we should clear them from archive)
            // Note: In a full implementation, we'd track which segments were restored
            // and remove only those. For now, this is handled by the restore() method.
        }
        mylm_core::info_log!(
            "[APP] Processing message (len={}): preview='{}...'", 
            input.len(), 
            &input[..input.len().min(30)]
        );

        // Handle slash commands
        if input.starts_with('/') {
            mylm_core::info_log!("[APP] Detected slash command: {}", input);
            self.handle_slash_command(&input, _event_tx);
            self.chat_input.clear();
            self.reset_cursor();
            return;
        }

        // Build context with terminal snapshot deduplication
        mylm_core::debug_log!("[APP] Building terminal snapshot");
        let history_height = 5000;
        let width = self.terminal_size.1;
        let mut temp_parser = vt100::Parser::new(history_height, width, 0);
        temp_parser.process(&self.raw_buffer);
        let terminal_content = temp_parser.screen().contents();
        mylm_core::debug_log!("[APP] Terminal content length: {}", terminal_content.len());

        let builder = ContextBuilder::new(mylm_core::config::ContextProfile::Balanced);
        let mut final_message = input.clone();

        // Only include terminal snapshot if it has changed from the last one
        let should_include_snapshot = self.last_terminal_snapshot.as_ref()
            .map(|last| last != &terminal_content)
            .unwrap_or(true); // Include if no previous snapshot

        mylm_core::debug_log!("[APP] Should include snapshot: {}", should_include_snapshot);

        if should_include_snapshot {
            if let Some(pack) = builder.build_terminal_pack(&terminal_content) {
                mylm_core::debug_log!("[APP] Adding terminal pack to message");
                final_message.push_str(&pack.render());
                self.last_terminal_snapshot = Some(terminal_content);
            }
        }

        mylm_core::debug_log!("[APP] Final message length: {}", final_message.len());
        self.chat_history.push(TimestampedChatMessage::user(&final_message));
        mylm_core::info_log!(
            "[APP] Added message to chat history, now have {} messages", 
            self.chat_history.len()
        );
        
        // Update context manager with new message for token tracking
        let chat_msgs: Vec<ChatMessage> = self.chat_history.iter().map(|m| m.message.clone()).collect();
        self.context_manager.set_history(&chat_msgs);
        
        // Set conversation topic from first user message to prevent context jumping
        if self.chat_history.len() <= 2 {
            let topic = input.split_whitespace()
                .take(5)
                .collect::<Vec<_>>()
                .join(" ");
            if !topic.is_empty() {
                mylm_core::debug_log!("[APP] Setting conversation topic: {}", topic);
                self.context_manager.set_topic(&topic);
            }
        }
        
        // Pre-flight check for token count warning
        if let Some(warning) = self.context_manager.preflight_check(Some(&final_message)) {
            mylm_core::warn_log!("[APP] Token warning: {}", warning);
            self.status_message = Some(format!("⚠️ {}", warning));
        }

        // Auto-save session
        if !self.incognito {
            mylm_core::debug_log!("[APP] Auto-saving session");
            let session = self.build_current_session().await;
            self.session_manager.set_current_session(session);
        }

        self.chat_input.clear();
        self.reset_cursor();
        self.input_scroll = 0;
        self.state = AppState::Thinking("...".to_string());
        self.chat_scroll = 0;
        self.chat_auto_scroll = true;

        // Submit message to agent session via input channel
        mylm_core::info_log!("[APP] Submitting to session...");
        self.submit_to_session(_event_tx).await;
        mylm_core::info_log!("[APP] submit_message complete");
    }
    
    /// Submit user message to the active agent session
    /// 
    /// Uses the input_tx channel to send UserInput::Message to the session
    async fn submit_to_session(&mut self, _event_tx: UnboundedSender<TuiEvent>) {
        mylm_core::info_log!("[APP] submit_to_session called");
        
        // Check if session is still active
        if !self.session_active {
            mylm_core::error_log!("[APP] Session has halted - cannot send message");
            self.status_message = Some("Error: Session has ended. Press Esc to return to hub.".to_string());
            self.state = AppState::Idle;
            return;
        }
        
        if let Some(input_tx) = &self.input_tx {
            let last_message = self.chat_history.last()
                .map(|m| m.message.content.clone())
                .unwrap_or_default();
            
            let msg_preview = &last_message[..last_message.len().min(50)];
            mylm_core::info_log!(
                "[APP] Sending user message via input channel (len={}, preview='{}...')", 
                last_message.len(), 
                msg_preview
            );
            
            match input_tx.send(UserInput::Message(last_message)).await {
                Ok(_) => {
                    mylm_core::info_log!("[APP] Message sent successfully to session");
                }
                Err(e) => {
                    mylm_core::error_log!("[APP] Failed to send message to session: {}", e);
                    self.status_message = Some(format!("Error: Failed to send message - {}", e));
                    self.state = AppState::Idle;
                }
            }
        } else {
            // No active session - need to start one
            mylm_core::error_log!("[APP] submit_to_session: No active input channel available!");
            mylm_core::error_log!("[APP] This means the session was not properly initialized with input_tx");
            self.status_message = Some("Error: No active agent session. Please restart.".to_string());
            self.state = AppState::Idle;
        }
    }
}
