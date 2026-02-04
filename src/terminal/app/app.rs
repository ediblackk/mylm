//! Terminal Application - Main orchestration module
//!
//! This module has been refactored into focused submodules:
//! - `app/state.rs` - State container and core state management
//! - `app/input.rs` - Input handling (cursor, text editing, selection)
//! - `app/commands.rs` - Slash command processing
//! - `app/clipboard.rs` - Clipboard operations
//! - `app/session.rs` - Session persistence
//! - `agent_runner.rs` - Agent loop and PaCoRe execution

use crate::terminal::agent_runner::{run_agent_loop, run_pacore_task};
use crate::terminal::app::state::AppStateContainer;

use mylm_core::context::pack::ContextBuilder;
use mylm_core::llm::chat::ChatMessage;
use std::sync::atomic::Ordering;

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

            // Check if PaCoRe is enabled
            if self.pacore_enabled {
                let agent = self.agent.clone();
                let history = self.chat_history.clone();
                let event_tx_clone = event_tx.clone();
                let interrupt_flag = self.interrupt_flag.clone();
                let pacore_rounds = self.pacore_rounds.clone();
                let config = self.config.clone();

                interrupt_flag.store(false, Ordering::SeqCst);

                let task = tokio::spawn(async move {
                    run_pacore_task(
                        agent,
                        history,
                        event_tx_clone,
                        interrupt_flag,
                        &pacore_rounds,
                        config,
                    )
                    .await;
                });

                self.active_task = Some(task);
            } else {
                // Standard agent loop
                let agent = self.agent.clone();
                let history = self.chat_history.clone();
                self.context_manager.set_history(&history);
                let context_ratio = self.context_manager.get_context_ratio();
                let event_tx_clone = event_tx.clone();
                let interrupt_flag = self.interrupt_flag.clone();
                let auto_approve = self.auto_approve.clone();
                let max_driver_loops = 30;
                interrupt_flag.store(false, Ordering::SeqCst);

                let task = tokio::spawn(async move {
                    let final_history = {
                        let agent_lock = agent.lock().await;
                        if context_ratio > agent_lock.llm_client.config().condense_threshold {
                            match agent_lock.condense_history(&history).await {
                                Ok(new_history) => new_history,
                                Err(_) => history,
                            }
                        } else {
                            history
                        }
                    };

                    {
                        let mut agent_lock = agent.lock().await;
                        agent_lock.reset(final_history).await;
                    }

                    run_agent_loop(
                        agent,
                        event_tx_clone,
                        interrupt_flag,
                        auto_approve,
                        max_driver_loops,
                        None,
                    )
                    .await;
                });

                self.active_task = Some(task);
            }
        }
    }
}
