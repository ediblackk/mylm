//! Worker Shell Tool - Restricted shell execution for worker agents
//!
//! This tool provides shell access for workers with strict permission controls:
//! - Allowed commands: Execute directly
//! - Restricted commands: Escalate to main agent for approval
//! - Forbidden commands: Always blocked

use crate::agent::tool::{Tool, ToolOutput, ToolKind};
pub use crate::config::v2::types::EscalationMode;
use crate::executor::CommandExecutor;
use async_trait::async_trait;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::sync::oneshot;

/// Escalation request sent to main agent
#[derive(Debug, Clone)]
pub struct EscalationRequest {
    pub worker_id: String,
    pub job_id: String,
    pub command: String,
    pub reason: String,
}

/// Response from main agent
#[derive(Debug, Clone)]
pub struct EscalationResponse {
    pub approved: bool,
    pub reason: Option<String>,
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
                "pkill *".to_string(),
                "top *".to_string(),
                "htop *".to_string(),
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
                "fork bomb".to_string(),
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
}

impl WorkerShellPermissions {
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

/// Shell tool for workers with permission controls
pub struct WorkerShellTool {
    executor: Arc<CommandExecutor>,
    permissions: WorkerShellPermissions,
    worker_id: String,
    job_id: String,
    /// Channel to send escalation requests to main agent
    escalation_tx: Option<tokio::sync::mpsc::Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
}

impl WorkerShellTool {
    /// Create a new WorkerShellTool
    pub fn new(
        executor: Arc<CommandExecutor>,
        permissions: WorkerShellPermissions,
        worker_id: String,
        job_id: String,
        escalation_tx: Option<tokio::sync::mpsc::Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
    ) -> Self {
        Self {
            executor,
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
    async fn execute_direct(&self, cmd: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        // Parse the command
        let parts = match shell_words::split(cmd) {
            Ok(p) if !p.is_empty() => p,
            _ => {
                return Ok(ToolOutput::Immediate(serde_json::json!({
                    "status": "error",
                    "error": "Failed to parse command"
                })));
            }
        };
        
        let command = &parts[0];
        let args = &parts[1..];
        
        // Use current working directory
        let cwd = std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string());
        
        match self.executor.execute_raw(command, args, cwd.as_deref()).await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let success = output.status.success();
                
                Ok(ToolOutput::Immediate(serde_json::json!({
                    "status": if success { "success" } else { "error" },
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": output.status.code()
                })))
            }
            Err(e) => Ok(ToolOutput::Immediate(serde_json::json!({
                "status": "error",
                "error": format!("Command failed: {}", e)
            }))),
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
        
        let response = resp_rx.await
            .map_err(|_| "Escalation response channel closed".to_string())?;
        
        Ok(response.approved)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandClassification {
    Allowed,
    Restricted,
    Forbidden,
}

#[async_trait]
impl Tool for WorkerShellTool {
    fn name(&self) -> &str {
        "execute_command"
    }
    
    fn description(&self) -> &str {
        "Execute a shell command with permission controls. Workers have restricted access - some commands require main agent approval."
    }
    
    fn usage(&self) -> &str {
        r#"Execute shell command: { "command": "ls -la" }

PERMISSION LEVELS:
- ALLOWED: Executed immediately (ls, cat, grep, git status, etc.)
- RESTRICTED: Requires main agent approval (rm, mv, cp, curl, ssh, etc.)
- FORBIDDEN: Never allowed (sudo, su, rm -rf /, etc.)"#
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                }
            },
            "required": ["command"]
        })
    }
    
    fn kind(&self) -> ToolKind {
        ToolKind::Terminal
    }
    
    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        // Parse arguments - just extract the command string
        let command = args.trim().trim_matches('"').to_string();
        
        if command.is_empty() {
            return Ok(ToolOutput::Immediate(serde_json::json!({
                "status": "error",
                "error": "Empty command"
            })));
        }
        
        // Classify command
        let classification = self.classify_command(&command);
        
        match classification {
            CommandClassification::Forbidden => {
                crate::warn_log!(
                    "WorkerShellTool [{}]: Forbidden command blocked: {}",
                    self.worker_id, command
                );
                Ok(ToolOutput::Immediate(serde_json::json!({
                    "status": "forbidden",
                    "error": format!("Command '{}' is forbidden and cannot be executed", command)
                })))
            }
            
            CommandClassification::Allowed => {
                crate::info_log!(
                    "WorkerShellTool [{}]: Executing allowed command: {}",
                    self.worker_id, command
                );
                self.execute_direct(&command).await
            }
            
            CommandClassification::Restricted => {
                crate::info_log!(
                    "WorkerShellTool [{}]: Restricted command requires approval: {}",
                    self.worker_id, command
                );
                
                match self.permissions.escalation_mode {
                    EscalationMode::AllowAll => {
                        // Debug mode - allow anyway
                        self.execute_direct(&command).await
                    }
                    EscalationMode::BlockRestricted => {
                        Ok(ToolOutput::Immediate(serde_json::json!({
                            "status": "blocked",
                            "error": format!("Command '{}' is restricted and escalation is disabled", command)
                        })))
                    }
                    EscalationMode::EscalateToMain => {
                        // Request approval from main agent
                        match self.request_escalation(&command).await {
                            Ok(true) => {
                                crate::info_log!(
                                    "WorkerShellTool [{}]: Escalation approved for: {}",
                                    self.worker_id, command
                                );
                                self.execute_direct(&command).await
                            }
                            Ok(false) => {
                                Ok(ToolOutput::Immediate(serde_json::json!({
                                    "status": "rejected",
                                    "error": format!("Command '{}' was rejected by main agent", command)
                                })))
                            }
                            Err(e) => {
                                crate::error_log!(
                                    "WorkerShellTool [{}]: Escalation failed: {}",
                                    self.worker_id, e
                                );
                                Ok(ToolOutput::Immediate(serde_json::json!({
                                    "status": "escalation_failed",
                                    "error": format!("Failed to escalate command: {}", e)
                                })))
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pattern_matching() {
        let tool = WorkerShellTool::new(
            Arc::new(CommandExecutor::new(
                crate::executor::allowlist::CommandAllowlist::new(),
                crate::executor::safety::SafetyChecker::new(),
            )),
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
            Arc::new(CommandExecutor::new(
                crate::executor::allowlist::CommandAllowlist::new(),
                crate::executor::safety::SafetyChecker::new(),
            )),
            WorkerShellPermissions::default(),
            "test".to_string(),
            "test-job".to_string(),
            None,
        );
        
        // Allowed
        assert_eq!(tool.classify_command("ls -la"), CommandClassification::Allowed);
        assert_eq!(tool.classify_command("cat file.txt"), CommandClassification::Allowed);
        
        // Forbidden
        assert_eq!(tool.classify_command("sudo ls"), CommandClassification::Forbidden);
        assert_eq!(tool.classify_command("rm -rf /"), CommandClassification::Forbidden);
        
        // Restricted
        assert_eq!(tool.classify_command("rm file.txt"), CommandClassification::Restricted);
        assert_eq!(tool.classify_command("curl http://example.com"), CommandClassification::Restricted);
        
        // Unknown defaults to restricted
        assert_eq!(tool.classify_command("unknown_command"), CommandClassification::Restricted);
    }
}
