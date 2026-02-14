//! Approval runtime

use crate::agent::types::common::Approval;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

/// Approval request pending user response
#[derive(Debug)]
pub struct PendingApproval {
    pub tool: String,
    pub args: String,
    response_tx: oneshot::Sender<Approval>,
}

impl PendingApproval {
    pub fn new(tool: String, args: String, response_tx: oneshot::Sender<Approval>) -> Self {
        Self {
            tool,
            args,
            response_tx,
        }
    }
    
    pub fn respond(self, approval: Approval) {
        let _ = self.response_tx.send(approval);
    }
}

/// Approval runtime - manages pending approvals
#[derive(Debug, Default)]
pub struct ApprovalRuntime {
    pending: Arc<Mutex<Option<PendingApproval>>>,
}

impl ApprovalRuntime {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Create new pending approval
    pub async fn request(&self, tool: String, args: String) -> oneshot::Receiver<Approval> {
        let (tx, rx) = oneshot::channel();
        let pending = PendingApproval::new(tool, args, tx);
        *self.pending.lock().await = Some(pending);
        rx
    }
    
    /// Respond to pending approval
    pub async fn respond(&self, approval: Approval) -> Result<(), ()> {
        if let Some(pending) = self.pending.lock().await.take() {
            pending.respond(approval);
            Ok(())
        } else {
            Err(())
        }
    }
    
    /// Check if has pending
    pub async fn has_pending(&self) -> bool {
        self.pending.lock().await.is_some()
    }
}
