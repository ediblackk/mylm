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
use tokio::sync::{mpsc, oneshot};
use std::sync::Arc;
use tokio::sync::Mutex;

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
/// This capability sends approval requests to the TUI via a channel
/// and waits for the user's response via a oneshot channel.
pub struct TuiApprovalCapability {
    /// Sender for pending approvals to the UI
    pending_tx: mpsc::Sender<PendingApproval>,
    
    /// Current pending approval (if any)
    current: Arc<Mutex<Option<PendingApproval>>>,
}

impl TuiApprovalCapability {
    /// Create a new TUI approval capability
    /// 
    /// Returns the capability and a receiver for pending approvals
    pub fn new() -> (Self, mpsc::Receiver<PendingApproval>) {
        let (pending_tx, pending_rx) = mpsc::channel(10);
        
        let capability = Self {
            pending_tx,
            current: Arc::new(Mutex::new(None)),
        };
        
        (capability, pending_rx)
    }
    
    /// Respond to the current pending approval
    pub async fn respond(&self, outcome: ApprovalOutcome) -> Result<(), String> {
        let mut current = self.current.lock().await;
        
        if let Some(pending) = current.take() {
            pending.response_tx
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
        // Create oneshot channel for response
        let (response_tx, response_rx) = oneshot::channel();
        
        // Create pending approval
        let pending = PendingApproval {
            request: req.clone(),
            response_tx,
        };
        
        // Store current pending
        {
            let mut current = self.current.lock().await;
            *current = Some(pending);
        }
        
        // Send to UI (non-blocking, UI will poll or receive event)
        // Note: In actual implementation, the UI will be notified via OutputEvent
        // This channel is a backup/fallback mechanism
        let _ = self.pending_tx.send(PendingApproval {
            request: req,
            response_tx: {
                // We already stored the response_tx, send a clone
                let (tx, _) = oneshot::channel();
                tx
            },
        }).await;
        
        // Wait for user response
        match response_rx.await {
            Ok(outcome) => Ok(outcome),
            Err(_) => Err(ApprovalError::new("Approval response channel closed")),
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
