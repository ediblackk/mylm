//! Terminal Application - Main orchestration module
//!
//! This module has been refactored into focused submodules:
//! - `app/state.rs` - State container and core state management
//! - `app/input.rs` - Input handling (cursor, text editing, selection)
//! - `app/commands.rs` - Slash command processing
//! - `app/clipboard.rs` - Clipboard operations
//! - `app/session.rs` - Session persistence
//! - `agent_runner.rs` - Legacy agent loop (deprecated)

use crate::terminal::app::state::AppStateContainer;

use mylm_core::agent::ChatSessionMessage;
use mylm_core::context::pack::ContextBuilder;
use mylm_core::llm::chat::ChatMessage;
use tokio::sync::mpsc::UnboundedSender;

pub use mylm_core::terminal::app::{AppState, TuiEvent};
pub use crate::terminal::app::state::AppStateContainer as App;

impl AppStateContainer {
    /// Submit a message to the agent for processing
    pub async fn submit_message(&mut self, event_tx: UnboundedSender<TuiEvent>) {
        if !self.chat_input.is_empty() {
            self.abort_current_task();
            self.status_message = None;
            let input = self.chat_input.clone();

            // Handle slash commands
            if input.starts_with('/') {
                self.handle_slash_command(&input, event_tx);
                self.chat_input.clear();
                self.reset_cursor();
                return;
            }

            // Build context with terminal snapshot deduplication
            let history_height = 5000;
            let width = self.terminal_size.1;
            let mut temp_parser = vt100::Parser::new(history_height, width, 0);
            temp_parser.process(&self.raw_buffer);
            let terminal_content = temp_parser.screen().contents();

            let builder = ContextBuilder::new(mylm_core::config::ContextProfile::Balanced);
            let mut final_message = input.clone();

            // Only include terminal snapshot if it has changed from the last one
            let should_include_snapshot = self.last_terminal_snapshot.as_ref()
                .map(|last| last != &terminal_content)
                .unwrap_or(true); // Include if no previous snapshot

            if should_include_snapshot {
                if let Some(pack) = builder.build_terminal_pack(&terminal_content) {
                    final_message.push_str(&pack.render());
                    // Update the last snapshot
                    self.last_terminal_snapshot = Some(terminal_content);
                }
            }

            self.chat_history.push(ChatMessage::user(&final_message));
            
            // Update context manager with new message for token tracking
            self.context_manager.set_history(&self.chat_history);
            
            // Set conversation topic from first user message to prevent context jumping
            if self.chat_history.len() <= 2 {
                // Extract key terms from the message as the topic
                let topic = input.split_whitespace()
                    .take(5)
                    .collect::<Vec<_>>()
                    .join(" ");
                if !topic.is_empty() {
                    self.context_manager.set_topic(&topic);
                }
            }
            
            // Pre-flight check for token count warning
            if let Some(warning) = self.context_manager.preflight_check(Some(&final_message)) {
                self.status_message = Some(format!("⚠️ {}", warning));
            }

            // Auto-save session
            if !self.incognito {
                let session = self.build_current_session().await;
                self.session_manager.set_current_session(session);
            }

            self.chat_input.clear();
            self.reset_cursor();
            self.input_scroll = 0;
            self.state = AppState::Thinking("...".to_string());
            self.chat_scroll = 0;
            self.chat_auto_scroll = true;

            // Use orchestrator chat session mode for proper job tracking
            // and interleaved user chat + background worker processing
            self.run_chat_session(event_tx).await;
        }
    }
    
    /// Run chat session using the orchestrator
    /// 
    /// This manages:
    /// - Starting the chat session if not already active
    /// - Submitting user messages to the queue
    /// - Background worker coordination via orchestrator
    async fn run_chat_session(&mut self, event_tx: UnboundedSender<TuiEvent>) {
        if let Some(orchestrator) = &self.orchestrator {
            if self.chat_session_handle.is_none() {
                // Start new chat session
                // Simplified: removed session start log
                let history = self.chat_history.clone();
                let (_task_handle, session_handle) = orchestrator.start_chat_session(history).await;
                self.chat_session_handle = Some(session_handle);
            } else {
                // Chat session already running, just submit the new message
                // Simplified: removed message submit log
                let last_message = self.chat_history.last()
                    .cloned()
                    .unwrap_or_else(|| ChatMessage::user(""));
                if let Some(handle) = &self.chat_session_handle {
                    mylm_core::info_log!("run_chat_session: Sending message via handle");
                    handle.send(ChatSessionMessage::UserMessage(last_message)).await;
                    mylm_core::info_log!("run_chat_session: Message sent successfully");
                } else {
                    mylm_core::error_log!("run_chat_session: No session handle available!");
                }
            }
        } else {
            // Fallback if orchestrator not available
            mylm_core::error_log!("run_chat_session: No orchestrator available");
            let _ = event_tx.send(TuiEvent::AgentResponse(
                "Error: Chat session not available".to_string(),
                mylm_core::llm::TokenUsage::default(),
            ));
        }
    }
}
