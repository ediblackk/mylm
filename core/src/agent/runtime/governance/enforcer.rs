//! Approval Enforcer
//!
//! Bridges governance policy (Authority) with approval capabilities.
//! Enforces permission rules before approval requests.

use crate::agent::types::common::Approval;
use crate::agent::runtime::governance::{Authority, ToolAccess, ShellAccess};
use crate::agent::identity::AgentId;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

/// User approval request pending response
#[derive(Debug)]
pub struct RuntimePendingApproval {
    pub tool: String,
    pub args: String,
    response_tx: oneshot::Sender<Approval>,
}

impl RuntimePendingApproval {
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

/// Approval enforcer - manages pending user approvals with policy enforcement
#[derive(Debug, Default)]
pub struct ApprovalEnforcer {
    pending: Arc<Mutex<Option<RuntimePendingApproval>>>,
}

impl ApprovalEnforcer {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Check if tool requires approval based on authority
    pub fn check_tool_access(
        &self,
        authority: &Authority,
        agent_id: &AgentId,
        tool: &str,
    ) -> ToolAccess {
        authority.check_tool(agent_id, tool)
    }
    
    /// Check if shell command requires approval
    pub fn check_shell_access(
        &self,
        authority: &Authority,
        agent_id: &AgentId,
        cmd: &str,
    ) -> ShellAccess {
        authority.check_shell(agent_id, cmd)
    }
    
    /// Create new pending approval request
    pub async fn request(&self, tool: String, args: String) -> oneshot::Receiver<Approval> {
        let (tx, rx) = oneshot::channel();
        let pending = RuntimePendingApproval::new(tool, args, tx);
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
    
    /// Check if has pending approval
    pub async fn has_pending(&self) -> bool {
        self.pending.lock().await.is_some()
    }
}
