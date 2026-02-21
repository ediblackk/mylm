//! Terminal-based approval capability
//!
//! Prompts user in terminal for tool approval.

use crate::agent::runtime::core::{
    Capability, ApprovalCapability, RuntimeContext, ApprovalError,
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

/// Worker restricted approval - auto-approves based on allowed/forbidden patterns
/// 
/// - Commands matching `allowed_commands` → Auto-approved
/// - Commands matching `forbidden_commands` → Auto-denied  
/// - Everything else → Auto-denied (escalation to parent can be added later)
pub struct WorkerRestrictedApprovalCapability {
    allowed_patterns: Vec<String>,
    forbidden_patterns: Vec<String>,
}

impl WorkerRestrictedApprovalCapability {
    pub fn new(allowed: Vec<String>, forbidden: Vec<String>) -> Self {
        Self {
            allowed_patterns: allowed,
            forbidden_patterns: forbidden,
        }
    }

    /// Check if command matches any pattern (supports * wildcards)
    fn matches_pattern(&self, command: &str, pattern: &str) -> bool {
        if pattern.contains('*') {
            // Convert glob pattern to regex-like matching
            let regex_pattern = pattern
                .replace(".", "\\.")
                .replace("*", ".*");
            if let Ok(regex) = regex::Regex::new(&format!("^{}$", regex_pattern)) {
                return regex.is_match(command);
            }
        }
        // Exact match or simple contains check
        command.contains(pattern.trim_end_matches('*').trim_start_matches('*'))
    }

    /// Check if command should be auto-approved
    fn is_allowed(&self, tool: &str, args: &str) -> bool {
        let command = format!("{} {}", tool, args);
        
        // First check forbidden - these always deny
        for pattern in &self.forbidden_patterns {
            if self.matches_pattern(&command, pattern) {
                return false;
            }
        }
        
        // Then check allowed
        for pattern in &self.allowed_patterns {
            if self.matches_pattern(&command, pattern) {
                return true;
            }
        }
        
        // Default: not allowed
        false
    }
}

impl Capability for WorkerRestrictedApprovalCapability {
    fn name(&self) -> &'static str {
        "worker-restricted-approval"
    }
}

#[async_trait::async_trait]
impl ApprovalCapability for WorkerRestrictedApprovalCapability {
    async fn request(
        &self,
        _ctx: &RuntimeContext,
        req: ApprovalRequest,
    ) -> Result<ApprovalOutcome, ApprovalError> {
        if self.is_allowed(&req.tool, &req.args) {
            crate::info_log!(
                "[WORKER_APPROVAL] Auto-approved: {} {} (matched allowed pattern)",
                req.tool, req.args
            );
            Ok(ApprovalOutcome::Granted)
        } else {
            crate::info_log!(
                "[WORKER_APPROVAL] Auto-denied: {} {} (no matching allowed pattern)",
                req.tool, req.args
            );
            Ok(ApprovalOutcome::Denied {
                reason: Some(format!(
                    "Command '{}' not in worker's allowed commands list. Allowed: {:?}",
                    req.tool, self.allowed_patterns
                )),
            })
        }
    }
}
