//! TUI Approval Capability
//!
//! Bridges the core approval system to the TUI.
//! Uses oneshot channels for request/response pattern.

use async_trait::async_trait;
use mylm_core::agent::runtime::core::{
    ApprovalCapability, Capability, RuntimeContext, ApprovalError,
};
use mylm_core::agent::types::{
    intents::ApprovalRequest,
    events::ApprovalOutcome,
};
use tokio::sync::{mpsc, oneshot};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Pending approval request (sent to UI)
#[derive(Debug)]
pub struct PendingApproval {
    /// The approval request details
    pub request: ApprovalRequest,
    /// Channel sender for the response
    pub response_tx: oneshot::Sender<ApprovalOutcome>,
}

/// Internal storage for current pending approval
#[derive(Debug, Clone)]
struct CurrentPending {
    /// The approval request details (stored for querying)
    request: ApprovalRequest,
    /// Whether a response has been sent
    responded: bool,
}

/// TUI-based approval capability
///
/// This capability sends approval requests to the TUI via a channel
/// and waits for the UI to respond via the `ApprovalHandle`.
#[derive(Clone)]
pub struct TuiApprovalCapability {
    /// Sender for pending approvals to the UI
    pending_tx: mpsc::Sender<PendingApproval>,
    /// Current pending approval request (without sender - stored separately)
    current: Arc<Mutex<Option<CurrentPending>>>,
    /// Auto-approve flag - when true, all approvals are auto-granted
    auto_approve: Arc<std::sync::atomic::AtomicBool>,
}

impl TuiApprovalCapability {
    /// Create a new TUI approval capability
    ///
    /// Returns the capability and a receiver for pending approvals
    pub fn new() -> (Self, mpsc::Receiver<PendingApproval>) {
        let (pending_tx, pending_rx) = mpsc::channel(10);
        let current = Arc::new(Mutex::new(None));
        
        (Self {
            pending_tx,
            current,
            auto_approve: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }, pending_rx)
    }
    
    /// Get the auto_approve flag for external toggling
    pub fn auto_approve(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.auto_approve)
    }
    
    /// Respond to the current pending approval
    pub async fn respond(&self, outcome: ApprovalOutcome) -> Result<(), String> {
        let mut current = self.current.lock().await;
        if let Some(ref mut pending) = *current {
            if pending.responded {
                return Err("Approval already responded to".to_string());
            }
            pending.responded = true;
            // Note: The actual response is sent through the oneshot channel by the UI
            // This method just marks it as responded in our tracking
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
    
    /// Clear the current pending approval
    pub async fn clear_pending(&self) {
        *self.current.lock().await = None;
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
            mylm_core::info_log!("[TUI_APPROVAL] Auto-approve enabled, granting approval for '{}'", req.tool);
            return Ok(ApprovalOutcome::Granted);
        }
        
        // Create oneshot channel for response
        let (tx, rx) = oneshot::channel();
        
        // Create pending approval with sender
        let pending = PendingApproval {
            request: req.clone(),
            response_tx: tx,
        };
        
        // Store request info in current (without sender)
        *self.current.lock().await = Some(CurrentPending {
            request: req,
            responded: false,
        });
        
        // Send to UI
        if self.pending_tx.send(pending).await.is_err() {
            self.clear_pending().await;
            return Err(ApprovalError::new("Approval channel closed".to_string()));
        }
        
        // Wait for response
        match rx.await {
            Ok(outcome) => {
                self.clear_pending().await;
                Ok(outcome)
            }
            Err(_) => {
                self.clear_pending().await;
                Err(ApprovalError::new("Approval response channel closed".to_string()))
            }
        }
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
    
    /// Get the underlying capability
    pub fn capability(&self) -> &TuiApprovalCapability {
        &self.capability
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
