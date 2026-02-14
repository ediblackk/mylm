//! Terminal-based approval capability
//!
//! Prompts user in terminal for tool approval.

use crate::agent::runtime::{
    capability::{Capability, ApprovalCapability},
    context::RuntimeContext,
    error::ApprovalError,
};
use crate::agent::types::intents::ApprovalRequest;
use crate::agent::types::events::ApprovalOutcome;

/// Terminal-based approval - prompts user interactively
pub struct TerminalApprovalCapability;

impl TerminalApprovalCapability {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TerminalApprovalCapability {
    fn default() -> Self {
        Self::new()
    }
}

impl Capability for TerminalApprovalCapability {
    fn name(&self) -> &'static str {
        "terminal-approval"
    }
}

#[async_trait::async_trait]
impl ApprovalCapability for TerminalApprovalCapability {
    async fn request(
        &self,
        _ctx: &RuntimeContext,
        req: ApprovalRequest,
    ) -> Result<ApprovalOutcome, ApprovalError> {
        use std::io::{self, Write};
        
        println!();
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║  TOOL APPROVAL REQUIRED                                      ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║  Tool:  {:<52} ║", req.tool);
        println!("║  Args:  {:<52} ║", 
            if req.args.len() > 50 { &req.args[..50] } else { &req.args });
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║  Reason:                                                     ║");
        
        // Print reason wrapped to fit box
        let reason = &req.reason;
        for chunk in reason.as_bytes().chunks(58) {
            let line = String::from_utf8_lossy(chunk);
            println!("║  {:<58} ║", line);
        }
        
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║  Approve? [y/N]:                                             ║");
        println!("╚══════════════════════════════════════════════════════════════╝");
        print!("> ");
        io::stdout().flush().map_err(|e| ApprovalError::new(e.to_string()))?;
        
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| ApprovalError::new(e.to_string()))?;
        
        let approved = input.trim().eq_ignore_ascii_case("y");
        
        if approved {
            println!("✓ Approved");
            Ok(ApprovalOutcome::Granted)
        } else {
            println!("✗ Rejected");
            Ok(ApprovalOutcome::Denied { reason: Some("User rejected".to_string()) })
        }
    }
}

/// Auto-approve capability for testing/non-interactive mode
pub struct AutoApproveCapability;

impl AutoApproveCapability {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AutoApproveCapability {
    fn default() -> Self {
        Self::new()
    }
}

impl Capability for AutoApproveCapability {
    fn name(&self) -> &'static str {
        "auto-approve"
    }
}

#[async_trait::async_trait]
impl ApprovalCapability for AutoApproveCapability {
    async fn request(
        &self,
        _ctx: &RuntimeContext,
        _req: ApprovalRequest,
    ) -> Result<ApprovalOutcome, ApprovalError> {
        Ok(ApprovalOutcome::Granted)
    }
}
