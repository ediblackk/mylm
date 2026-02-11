//! Session persistence operations
use crate::terminal::app::state::AppStateContainer;
use crate::terminal::session::Session;
use crate::terminal::session_manager::SessionManager;


impl AppStateContainer {
    pub async fn save_session(
        &self,
        _custom_name: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.incognito {
            return Ok(());
        }

        let session = self.build_current_session().await;
        self.session_manager.set_current_session(session);
        Ok(())
    }

    pub async fn load_session(id: Option<&str>) -> Result<Session, Box<dyn std::error::Error>> {
        if id.is_none() {
            if let Some(latest) = SessionManager::load_latest().await {
                return Ok(latest);
            }
            return Err("No latest session found".into());
        }

        let target_id = id.unwrap();
        let target_clean = if target_id.ends_with(".json") {
            target_id.trim_end_matches(".json")
        } else {
            target_id
        };

        let sessions = SessionManager::load_sessions();

        for session in sessions {
            if session.id == target_clean
                || session.id.ends_with(&format!("_{}", target_clean))
                || session.id.contains(target_clean)
            {
                return Ok(session);
            }
        }

        Err("Session not found".into())
    }

    pub async fn trigger_manual_condensation(
        &mut self,
        event_tx: tokio::sync::mpsc::UnboundedSender<crate::terminal::app::TuiEvent>,
    ) {
        use crate::terminal::app::TuiEvent;
        if self.state == mylm_core::terminal::app::AppState::Idle {
            let agent = self.agent.clone();
            let history = self.chat_history.clone();
            tokio::spawn(async move {
                if let Ok(new_history) = agent.condense_history(&history).await {
                    let _ = event_tx.send(TuiEvent::CondensedHistory(new_history));
                }
            });
        }
    }
}
