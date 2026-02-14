//! Session persistence operations
use crate::tui::app::state::AppStateContainer;
use crate::tui::types::TuiEvent;

impl AppStateContainer {
    pub async fn save_session(
        &self,
        _custom_name: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.incognito {
            return Ok(());
        }

        // Session persistence is now handled by the session manager
        // This is a stub implementation for compatibility
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn trigger_manual_condensation(
        &mut self,
        _event_tx: tokio::sync::mpsc::UnboundedSender<TuiEvent>,
    ) {
        // Manual condensation is not supported in the new architecture
        // Memory management is handled by the core agent
    }
}
