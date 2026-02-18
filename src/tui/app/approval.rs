//! TUI Approval Capability
//!
//! Bridges the core approval system to the TUI.
//! Uses oneshot channels for request/response pattern.

use async_trait::async_trait;
use mylm_core::agent::runtime::{
    capability::{ApprovalCapability, Capability},
    context::RuntimeContext,
    error::ApprovalError,
};
use mylm_core::agent::types::{
    intents::ApprovalRequest,
    events::ApprovalOutcome,
};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

/// Pending approval request
#[derive(Debug)]
pub struct PendingApproval {
    /// The approval request details
    pub request: ApprovalRequest,
    /// Channel to send the response back
    pub response_tx: oneshot::Sender<ApprovalOutcome>,
}

/// TUI-based approval capability
///
/// This capability stores pending approvals and waits for the UI
/// to respond via the `ApprovalHandle`.
pub struct TuiApprovalCapability {
    /// Current pending approval (if any)
    current: Arc<Mutex<Option<PendingApproval>>>,
    /// Auto-approve flag - when true, all approvals are auto-granted
    auto_approve: Arc<std::sync::atomic::AtomicBool>,
}

impl TuiApprovalCapability {
    /// Create a new TUI approval capability
    pub fn new() -> Self {
        Self {
            current: Arc::new(Mutex::new(None)),
            auto_approve: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Get the auto_approve flag for external toggling
    pub fn auto_approve(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.auto_approve)
    }

    /// Respond to the current pending approval
    pub async fn respond(&self, outcome: ApprovalOutcome) -> Result<(), String> {
        let mut current = self.current.lock().await;

        if let Some(pending) = current.take() {
            pending
                .response_tx
                .send(outcome)
                .map_err(|_| "Failed to send approval response - receiver dropped".to_string())?;
            Ok(())
        } else {
            Err("No pending approval request".to_string())
        }
    }

    /// Check if there's a pending approval
    pub async fn has_pending(&self) -> bool {
        self.current.lock().await.is_some()
    }

    /// Get the current pending approval (without consuming it)
    pub async fn get_pending(&self) -> Option<ApprovalRequest> {
        self.current.lock().await.as_ref().map(|p| p.request.clone())
    }
}

impl Capability for TuiApprovalCapability {
    fn name(&self) -> &'static str {
        "tui-approval"
    }
}

#[async_trait]
impl ApprovalCapability for TuiApprovalCapability {
    async fn request(
        &self,
        _ctx: &RuntimeContext,
        req: ApprovalRequest,
    ) -> Result<ApprovalOutcome, ApprovalError> {
        // Check if auto-approve is enabled
        if self.auto_approve.load(std::sync::atomic::Ordering::SeqCst) {
            mylm_core::info_log!(
                "[TUI_APPROVAL] Auto-approve enabled, granting approval for '{}'",
                req.tool
            );
            return Ok(ApprovalOutcome::Granted);
        }

        // Create oneshot channel for response
        let (response_tx, response_rx) = oneshot::channel();

        // Create and store pending approval
        let pending = PendingApproval {
            request: req,
            response_tx,
        };

        {
            let mut current = self.current.lock().await;
            *current = Some(pending);
        }

        // Wait for user response via ApprovalHandle.respond()
        match response_rx.await {
            Ok(outcome) => Ok(outcome),
            Err(_) => Err(ApprovalError::new("Approval response channel closed")),
        }
    }

    fn would_auto_approve(&self, _tool: &str, _args: &str) -> bool {
        self.auto_approve.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Shared handle for responding to approvals from the UI
#[derive(Clone)]
pub struct ApprovalHandle {
    capability: Arc<TuiApprovalCapability>,
}

impl ApprovalHandle {
    pub fn new(capability: Arc<TuiApprovalCapability>) -> Self {
        Self { capability }
    }

    /// Approve the pending request
    pub async fn approve(&self) -> Result<(), String> {
        self.capability.respond(ApprovalOutcome::Granted).await
    }

    /// Deny the pending request
    pub async fn deny(&self, reason: Option<String>) -> Result<(), String> {
        self.capability.respond(ApprovalOutcome::Denied { reason }).await
    }

    /// Check if there's a pending approval
    pub async fn has_pending(&self) -> bool {
        self.capability.has_pending().await
    }

    /// Get pending approval details
    pub async fn get_pending(&self) -> Option<ApprovalRequest> {
        self.capability.get_pending().await
    }
}
