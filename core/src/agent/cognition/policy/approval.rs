//! Approval policy for tool execution
//!
//! Determines which tools/commands require user approval before execution.

/// Default dangerous tools that always require approval
const DANGEROUS_TOOLS: &[&str] = &["shell", "write_file", "rm", "sudo"];

/// Dangerous command patterns that require approval
const DANGEROUS_PATTERNS: &[&str] = &["rm -rf", "sudo", "curl | sh", "wget | sh"];

/// Check if a tool requires approval based on policy
pub fn requires_approval(tool: &str, args: &str) -> bool {
    if DANGEROUS_TOOLS.contains(&tool) {
        return true;
    }
    
    let command = format!("{} {}", tool, args);
    DANGEROUS_PATTERNS.iter().any(|p| command.contains(p))
}

/// Approval policy configuration
#[derive(Debug, Clone)]
pub struct ApprovalPolicy {
    /// Tools that always require approval
    pub dangerous_tools: Vec<String>,
    /// Patterns that trigger approval
    pub dangerous_patterns: Vec<String>,
    /// Whether to auto-approve non-dangerous tools
    pub auto_approve_safe: bool,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            dangerous_tools: DANGEROUS_TOOLS.iter().map(|s| s.to_string()).collect(),
            dangerous_patterns: DANGEROUS_PATTERNS.iter().map(|s| s.to_string()).collect(),
            auto_approve_safe: true,
        }
    }
}

impl ApprovalPolicy {
    /// Check if tool/args requires approval under this policy
    pub fn check(&self, tool: &str, args: &str) -> bool {
        if self.dangerous_tools.iter().any(|t| t == tool) {
            return true;
        }
        
        let command = format!("{} {}", tool, args);
        self.dangerous_patterns.iter().any(|p| command.contains(p))
    }
}
