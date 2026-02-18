//! Worker Shell Tool - Restricted shell execution for worker agents
//!
//! This tool provides shell access for workers with strict permission controls:
//! - **Allowed commands**: Execute directly (ls, cat, grep, cargo check, etc.)
//! - **Restricted commands**: Escalate to main agent for approval (rm, mv, curl, etc.)
//! - **Forbidden commands**: Always blocked (sudo, rm -rf /, etc.)
//!
//! # Escalation Flow
//! 1. Worker tries to execute restricted command
//! 2. WorkerShellTool sends escalation request via channel
//! 3. Main agent receives request and decides (approve/deny)
//! 4. If approved, command executes; if denied, error returned

use crate::agent::runtime::capability::{Capability, ToolCapability};
use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::ToolError;
use crate::agent::runtime::terminal::TerminalExecutor;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_OUTPUT_SIZE: usize = 100_000; // 100KB max output
const ESCALATION_TIMEOUT_SECS: u64 = 120; // 2 minutes for user to respond

/// Escalation request sent to main agent
#[derive(Debug, Clone)]
pub struct EscalationRequest {
    /// Worker ID requesting escalation
    pub worker_id: String,
    /// Job ID
    pub job_id: String,
    /// Command to execute
    pub command: String,
    /// Reason for escalation
    pub reason: String,
}

/// Response from main agent
#[derive(Debug, Clone)]
pub struct EscalationResponse {
    /// Whether command is approved
    pub approved: bool,
    /// Optional reason for decision
    pub reason: Option<String>,
}

/// How to handle restricted commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscalationMode {
    /// Allow all commands (debug mode)
    AllowAll,
    /// Block restricted commands without escalation
    BlockRestricted,
    /// Escalate to main agent for approval
    EscalateToMain,
}

impl Default for EscalationMode {
    fn default() -> Self {
        EscalationMode::EscalateToMain
    }
}

/// Permission configuration for worker shell
#[derive(Clone, Debug)]
pub struct WorkerShellPermissions {
    /// Commands that are always allowed (e.g., "ls *", "cat *")
    pub allowed_patterns: Vec<String>,
    /// Commands that require escalation (e.g., "rm *", "mv *")
    pub restricted_patterns: Vec<String>,
    /// Commands that are never allowed (e.g., "sudo *", "rm -rf /")
    pub forbidden_patterns: Vec<String>,
    /// How to handle restricted commands
    pub escalation_mode: EscalationMode,
}

impl Default for WorkerShellPermissions {
    fn default() -> Self {
        Self::standard()
    }
}

impl WorkerShellPermissions {
    /// Standard permissions for workers
    pub fn standard() -> Self {
        Self {
            allowed_patterns: vec![
                "ls *".to_string(),
                "cat *".to_string(),
                "grep *".to_string(),
                "find *".to_string(),
                "head *".to_string(),
                "tail *".to_string(),
                "wc *".to_string(),
                "pwd".to_string(),
                "echo *".to_string(),
                "dirname *".to_string(),
                "basename *".to_string(),
                "sort *".to_string(),
                "uniq *".to_string(),
                "awk *".to_string(),
                "sed *".to_string(),
                "tr *".to_string(),
                "cut *".to_string(),
                "ps *".to_string(),
                "pgrep *".to_string(),
                "df *".to_string(),
                "du *".to_string(),
                "free *".to_string(),
                "uptime *".to_string(),
                "whoami".to_string(),
                "id *".to_string(),
                "uname *".to_string(),
                "date".to_string(),
                "date *".to_string(),
                "which *".to_string(),
                "whereis *".to_string(),
                "type *".to_string(),
                "file *".to_string(),
                "stat *".to_string(),
                "readlink *".to_string(),
                "realpath *".to_string(),
                "git status*".to_string(),
                "git log*".to_string(),
                "git diff*".to_string(),
                "git show*".to_string(),
                "git branch*".to_string(),
                "git remote*".to_string(),
                "git config*".to_string(),
                "cargo check*".to_string(),
                "cargo build*".to_string(),
                "cargo test*".to_string(),
                "cargo run*".to_string(),
                "cargo fmt*".to_string(),
                "cargo clippy*".to_string(),
                "rustc *".to_string(),
                "make *".to_string(),
                "cmake *".to_string(),
                "go build*".to_string(),
                "go test*".to_string(),
                "go run*".to_string(),
                "go fmt*".to_string(),
                "npm *".to_string(),
                "yarn *".to_string(),
                "pnpm *".to_string(),
                "node *".to_string(),
                "python* *".to_string(),
                "pip* *".to_string(),
                "pytest *".to_string(),
            ],
            restricted_patterns: vec![
                "rm *".to_string(),
                "mv *".to_string(),
                "cp *".to_string(),
                "chmod *".to_string(),
                "chown *".to_string(),
                "dd *".to_string(),
                "mkfs.*".to_string(),
                "fdisk *".to_string(),
                "mount *".to_string(),
                "umount *".to_string(),
                "tar *".to_string(),
                "zip *".to_string(),
                "unzip *".to_string(),
                "gzip *".to_string(),
                "gunzip *".to_string(),
                "curl *".to_string(),
                "wget *".to_string(),
                "ssh *".to_string(),
                "scp *".to_string(),
                "rsync *".to_string(),
                "ftp *".to_string(),
                "sftp *".to_string(),
                "nc *".to_string(),
                "netcat *".to_string(),
                "telnet *".to_string(),
                "nmap *".to_string(),
                "ping *".to_string(),
                "traceroute *".to_string(),
                "docker *".to_string(),
                "kubectl *".to_string(),
                "helm *".to_string(),
                "terraform *".to_string(),
                "ansible* *".to_string(),
                "vagrant *".to_string(),
                "git push*".to_string(),
                "git pull*".to_string(),
                "git fetch*".to_string(),
                "git clone*".to_string(),
                "git checkout*".to_string(),
                "git reset*".to_string(),
                "git clean*".to_string(),
                "git stash*".to_string(),
                "git cherry-pick*".to_string(),
                "git rebase*".to_string(),
                "git merge*".to_string(),
            ],
            forbidden_patterns: vec![
                "sudo *".to_string(),
                "su *".to_string(),
                "su - *".to_string(),
                "passwd *".to_string(),
                "chpasswd *".to_string(),
                "useradd *".to_string(),
                "userdel *".to_string(),
                "usermod *".to_string(),
                "groupadd *".to_string(),
                "groupdel *".to_string(),
                "groupmod *".to_string(),
                "rm -rf /".to_string(),
                "rm -rf /*".to_string(),
                "rm -rf ~".to_string(),
                "rm -rf ~/*".to_string(),
                ":(){:|:&};:".to_string(),
                "> /dev/sda*".to_string(),
                "> /dev/hda*".to_string(),
                "mkfs.ext* /dev/sda*".to_string(),
                "mkfs.xfs /dev/sda*".to_string(),
                "mkfs.btrfs /dev/sda*".to_string(),
                "mkfs.vfat /dev/sda*".to_string(),
                "mkfs.ntfs /dev/sda*".to_string(),
                "dd if=/dev/zero of=/dev/sda*".to_string(),
                "dd if=/dev/random of=/dev/sda*".to_string(),
                "dd if=/dev/urandom of=/dev/sda*".to_string(),
                "shutdown *".to_string(),
                "reboot *".to_string(),
                "halt *".to_string(),
                "poweroff *".to_string(),
                "init 0".to_string(),
                "init 6".to_string(),
                "systemctl poweroff*".to_string(),
                "systemctl reboot*".to_string(),
                "kill -9 1".to_string(),
                "kill -SIGKILL 1".to_string(),
            ],
            escalation_mode: EscalationMode::EscalateToMain,
        }
    }
    
    /// Create permissive config (for testing/debugging only)
    pub fn permissive() -> Self {
        Self {
            allowed_patterns: vec!["*".to_string()],
            restricted_patterns: vec![],
            forbidden_patterns: vec!["sudo *".to_string(), "su *".to_string(), "rm -rf /".to_string()],
            escalation_mode: EscalationMode::AllowAll,
        }
    }
    
    /// Create restrictive config (max security)
    pub fn restrictive() -> Self {
        Self {
            allowed_patterns: vec!["ls *".to_string(), "cat *".to_string(), "pwd".to_string()],
            restricted_patterns: vec![],
            forbidden_patterns: vec!["*".to_string()], // Everything else forbidden
            escalation_mode: EscalationMode::BlockRestricted,
        }
    }
}

/// Classification of a command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandClassification {
    Allowed,
    Restricted,
    Forbidden,
}

/// Shell tool for workers with permission controls and escalation
pub struct WorkerShellTool {
    permissions: WorkerShellPermissions,
    worker_id: String,
    job_id: String,
    escalation_tx: Option<mpsc::Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
}

impl WorkerShellTool {
    /// Create a new WorkerShellTool
    pub fn new(
        permissions: WorkerShellPermissions,
        worker_id: String,
        job_id: String,
        escalation_tx: Option<mpsc::Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
    ) -> Self {
        Self {
            permissions,
            worker_id,
            job_id,
            escalation_tx,
        }
    }
    
    /// Check if command matches any pattern in the list
    fn matches_pattern(&self, cmd: &str, patterns: &[String]) -> bool {
        let cmd_lower = cmd.to_lowercase();
        for pattern in patterns {
            if Self::pattern_matches(&cmd_lower, &pattern.to_lowercase()) {
                return true;
            }
        }
        false
    }
    
    /// Simple glob pattern matching
    fn pattern_matches(cmd: &str, pattern: &str) -> bool {
        if pattern.ends_with('*') {
            let prefix = &pattern[..pattern.len()-1];
            cmd.starts_with(prefix)
        } else if pattern.starts_with('*') {
            let suffix = &pattern[1..];
            cmd.ends_with(suffix)
        } else {
            cmd == pattern
        }
    }
    
    /// Classify a command
    fn classify_command(&self, cmd: &str) -> CommandClassification {
        let cmd_trimmed = cmd.trim();
        
        // Check forbidden first (highest priority)
        if self.matches_pattern(cmd_trimmed, &self.permissions.forbidden_patterns) {
            return CommandClassification::Forbidden;
        }
        
        // Check allowed
        if self.matches_pattern(cmd_trimmed, &self.permissions.allowed_patterns) {
            return CommandClassification::Allowed;
        }
        
        // Check restricted
        if self.matches_pattern(cmd_trimmed, &self.permissions.restricted_patterns) {
            return CommandClassification::Restricted;
        }
        
        // Default: unknown commands are restricted
        CommandClassification::Restricted
    }
    
    /// Execute command directly
    async fn execute_direct(
        &self,
        command: &str,
        terminal: Option<&dyn TerminalExecutor>,
    ) -> Result<ToolResult, ToolError> {
        if let Some(term) = terminal {
            self.execute_with_terminal(term, command).await
        } else {
            self.execute_standalone(command).await
        }
    }
    
    /// Execute using terminal executor (PTY)
    async fn execute_with_terminal(
        &self,
        terminal: &dyn TerminalExecutor,
        command: &str,
    ) -> Result<ToolResult, ToolError> {
        let result = terminal.execute_command(
            command.to_string(),
            Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        ).await;

        match result {
            Ok(output) => {
                let output = if output.len() > MAX_OUTPUT_SIZE {
                    let mut truncated = output;
                    truncated.truncate(MAX_OUTPUT_SIZE);
                    truncated.push_str("\n... [output truncated]");
                    truncated
                } else {
                    output
                };

                Ok(ToolResult::Success {
                    output,
                    structured: None,
                })
            }
            Err(e) => Ok(ToolResult::Error {
                message: format!("Command failed: {}", e),
                code: Some("EXEC_ERROR".to_string()),
                retryable: false,
            }),
        }
    }
    
    /// Execute standalone (no terminal)
    async fn execute_standalone(&self, command: &str) -> Result<ToolResult, ToolError> {
        use tokio::process::Command;
        use tokio::time::timeout;
        
        let output = timeout(
            Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            async {
                if cfg!(target_os = "windows") {
                    Command::new("cmd")
                        .args(["/C", command])
                        .output()
                        .await
                } else {
                    Command::new("sh")
                        .args(["-c", command])
                        .output()
                        .await
                }
            }
        ).await;

        match output {
            Ok(Ok(output)) => {
                let mut result = String::new();
                
                if !output.stdout.is_empty() {
                    result.push_str(&String::from_utf8_lossy(&output.stdout));
                }
                
                if !output.stderr.is_empty() {
                    if !result.is_empty() {
                        result.push_str("\n\n[stderr]:\n");
                    } else {
                        result.push_str("[stderr]:\n");
                    }
                    result.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                
                if result.len() > MAX_OUTPUT_SIZE {
                    result.truncate(MAX_OUTPUT_SIZE);
                    result.push_str("\n... [output truncated]");
                }
                
                if output.status.success() {
                    Ok(ToolResult::Success {
                        output: result,
                        structured: None,
                    })
                } else {
                    let exit_code = output.status.code().unwrap_or(-1);
                    Ok(ToolResult::Error {
                        message: format!("Exit code {}: {}", exit_code, result),
                        code: Some("EXIT_ERROR".to_string()),
                        retryable: false,
                    })
                }
            }
            Ok(Err(e)) => Ok(ToolResult::Error {
                message: format!("Failed to execute: {}", e),
                code: Some("EXEC_ERROR".to_string()),
                retryable: false,
            }),
            Err(_) => Ok(ToolResult::Error {
                message: format!("Command timed out after {} seconds", DEFAULT_TIMEOUT_SECS),
                code: Some("TIMEOUT".to_string()),
                retryable: true,
            }),
        }
    }
    
    /// Request escalation to main agent
    async fn request_escalation(&self, cmd: &str) -> Result<bool, String> {
        let Some(ref tx) = self.escalation_tx else {
            return Err("Escalation channel not available".to_string());
        };
        
        let request = EscalationRequest {
            worker_id: self.worker_id.clone(),
            job_id: self.job_id.clone(),
            command: cmd.to_string(),
            reason: format!("Command '{}' matches restricted pattern", cmd),
        };
        
        let (resp_tx, resp_rx) = oneshot::channel();
        
        tx.send((request, resp_tx)).await
            .map_err(|_| "Failed to send escalation request".to_string())?;
        
        // Wait for response with timeout
        let response = tokio::time::timeout(
            Duration::from_secs(ESCALATION_TIMEOUT_SECS),
            resp_rx
        ).await
            .map_err(|_| "Escalation timed out - no response from main agent".to_string())?
            .map_err(|_| "Escalation response channel closed".to_string())?;
        
        Ok(response.approved)
    }
}

impl Capability for WorkerShellTool {
    fn name(&self) -> &'static str {
        "shell"
    }
}

#[async_trait::async_trait]
impl ToolCapability for WorkerShellTool {
    async fn execute(
        &self,
        ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse arguments
        let command = call.arguments.get("command")
            .and_then(|v| v.as_str())
            .or_else(|| call.arguments.as_str())
            .ok_or_else(|| ToolError::new(
                "Expected command string or {\"command\": \"...\"}"
            ))?;

        if command.is_empty() {
            return Ok(ToolResult::Error {
                message: "Empty command".to_string(),
                code: Some("EMPTY_COMMAND".to_string()),
                retryable: false,
            });
        }

        // Classify command
        let classification = self.classify_command(command);

        match classification {
            CommandClassification::Forbidden => {
                crate::warn_log!(
                    "WorkerShellTool [{}]: Forbidden command blocked: {}",
                    self.worker_id, command
                );
                Ok(ToolResult::Error {
                    message: format!("Command '{}' is forbidden and cannot be executed", command),
                    code: Some("FORBIDDEN".to_string()),
                    retryable: false,
                })
            }

            CommandClassification::Allowed => {
                crate::info_log!(
                    "WorkerShellTool [{}]: Executing allowed command: {}",
                    self.worker_id, command
                );
                self.execute_direct(command, ctx.terminal()).await
            }

            CommandClassification::Restricted => {
                crate::info_log!(
                    "WorkerShellTool [{}]: Restricted command requires approval: {}",
                    self.worker_id, command
                );

                match self.permissions.escalation_mode {
                    EscalationMode::AllowAll => {
                        // Debug mode - allow anyway
                        crate::info_log!(
                            "WorkerShellTool [{}]: AllowAll mode - executing restricted command",
                            self.worker_id
                        );
                        self.execute_direct(command, ctx.terminal()).await
                    }
                    EscalationMode::BlockRestricted => {
                        Ok(ToolResult::Error {
                            message: format!(
                                "Command '{}' is restricted and escalation is disabled. \
                                 This command requires manual execution by the main agent.",
                                command
                            ),
                            code: Some("RESTRICTED".to_string()),
                            retryable: false,
                        })
                    }
                    EscalationMode::EscalateToMain => {
                        // Request approval from main agent
                        match self.request_escalation(command).await {
                            Ok(true) => {
                                crate::info_log!(
                                    "WorkerShellTool [{}]: Escalation approved for: {}",
                                    self.worker_id, command
                                );
                                self.execute_direct(command, ctx.terminal()).await
                            }
                            Ok(false) => {
                                crate::info_log!(
                                    "WorkerShellTool [{}]: Escalation denied for: {}",
                                    self.worker_id, command
                                );
                                Ok(ToolResult::Error {
                                    message: format!(
                                        "Command '{}' was rejected by main agent",
                                        command
                                    ),
                                    code: Some("ESCALATION_DENIED".to_string()),
                                    retryable: false,
                                })
                            }
                            Err(e) => {
                                crate::error_log!(
                                    "WorkerShellTool [{}]: Escalation failed: {}",
                                    self.worker_id, e
                                );
                                Ok(ToolResult::Error {
                                    message: format!("Failed to escalate command: {}", e),
                                    code: Some("ESCALATION_FAILED".to_string()),
                                    retryable: true,
                                })
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Builder for creating escalation channels
pub fn create_escalation_channel() -> (
    mpsc::Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>,
    mpsc::Receiver<(EscalationRequest, oneshot::Sender<EscalationResponse>)>
) {
    mpsc::channel(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_matching() {
        let tool = WorkerShellTool::new(
            WorkerShellPermissions::default(),
            "test".to_string(),
            "test-job".to_string(),
            None,
        );

        assert!(tool.matches_pattern("ls -la", &["ls *".to_string()]));
        assert!(tool.matches_pattern("cat file.txt", &["cat *".to_string()]));
        assert!(!tool.matches_pattern("rm file", &["cat *".to_string()]));
    }

    #[test]
    fn test_command_classification() {
        let tool = WorkerShellTool::new(
            WorkerShellPermissions::default(),
            "test".to_string(),
            "test-job".to_string(),
            None,
        );

        // Allowed
        assert_eq!(tool.classify_command("ls -la"), CommandClassification::Allowed);
        assert_eq!(tool.classify_command("cat file.txt"), CommandClassification::Allowed);
        assert_eq!(tool.classify_command("cargo check"), CommandClassification::Allowed);

        // Forbidden
        assert_eq!(tool.classify_command("sudo ls"), CommandClassification::Forbidden);
        assert_eq!(tool.classify_command("rm -rf /"), CommandClassification::Forbidden);

        // Restricted
        assert_eq!(tool.classify_command("rm file.txt"), CommandClassification::Restricted);
        assert_eq!(tool.classify_command("curl http://example.com"), CommandClassification::Restricted);
        assert_eq!(tool.classify_command("mv old new"), CommandClassification::Restricted);

        // Unknown defaults to restricted
        assert_eq!(tool.classify_command("unknown_command"), CommandClassification::Restricted);
    }

    #[test]
    fn test_permissions_permissive() {
        let perms = WorkerShellPermissions::permissive();
        assert_eq!(perms.escalation_mode, EscalationMode::AllowAll);
        assert!(perms.allowed_patterns.contains(&"*".to_string()));
    }

    #[test]
    fn test_permissions_restrictive() {
        let perms = WorkerShellPermissions::restrictive();
        assert_eq!(perms.escalation_mode, EscalationMode::BlockRestricted);
        assert!(perms.forbidden_patterns.contains(&"*".to_string()));
    }
}
