//! Authority Enforcement
//!
//! Runtime-level permission checking for agents. Main has full authority.
//! Workers have restricted permissions and must escalate.
//!
//! # Usage
//!
//! ```rust
//! use mylm_core::agent::governance::{Authority, AuthorityMatrix};
//! use mylm_core::agent::AgentId;
//!
//! let matrix = AuthorityMatrix::default();
//! let authority = Authority::new(matrix);
//!
//! let worker = AgentId::worker("task-1");
//! let access = authority.check_tool(&worker, "shell");
//! ```

use crate::agent::identity::{AgentId, AgentType};
use std::collections::HashSet;

/// Tool access decision.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolAccess {
    /// Execute directly without approval
    Allowed,
    /// Request Main agent approval with reason
    Escalate(String),
    /// Never allowed
    Forbidden(String),
}

/// Shell command access decision.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellAccess {
    /// Execute directly
    Allowed,
    /// Escalate to Main for approval
    Escalate(String),
    /// Forbidden (e.g., sudo, rm -rf /)
    Forbidden(String),
}

/// Main agent permissions.
///
/// Main has full authority. This struct exists for completeness
/// and future policy customization.
#[derive(Debug, Clone)]
pub struct MainPermissions {
    /// If true, Main can override any restriction (default: true)
    pub can_override: bool,
}

impl Default for MainPermissions {
    fn default() -> Self {
        Self { can_override: true }
    }
}

/// Worker agent permissions.
///
/// Workers operate in a restricted sandbox. They can read freely
/// but write/shell operations require escalation.
#[derive(Debug, Clone)]
pub struct WorkerPermissions {
    /// Tools allowed without escalation
    pub allowed_tools: HashSet<String>,
    /// Tools that require Main approval
    pub escalate_tools: HashSet<String>,
    /// Tools that are never allowed
    pub forbidden_tools: HashSet<String>,
    /// Shell command patterns allowed directly
    pub allowed_shell_patterns: Vec<String>,
    /// Shell command patterns requiring escalation
    pub escalate_shell_patterns: Vec<String>,
    /// Shell command patterns that are forbidden
    pub forbidden_shell_patterns: Vec<String>,
}

impl Default for WorkerPermissions {
    /// Standard restrictive permissions for workers.
    fn default() -> Self {
        let mut allowed_tools = HashSet::new();
        allowed_tools.insert("read_file".to_string());
        allowed_tools.insert("list_files".to_string());
        allowed_tools.insert("git_status".to_string());
        allowed_tools.insert("git_log".to_string());
        allowed_tools.insert("git_diff".to_string());
        allowed_tools.insert("web_search".to_string());
        allowed_tools.insert("scratchpad".to_string()); // Coordination

        let mut escalate_tools = HashSet::new();
        escalate_tools.insert("shell".to_string());
        escalate_tools.insert("write_file".to_string());
        escalate_tools.insert("delegate".to_string()); // Workers can't spawn workers

        let mut forbidden_tools = HashSet::new();
        forbidden_tools.insert("delete_file".to_string()); // Explicit removal

        Self {
            allowed_tools,
            escalate_tools,
            forbidden_tools,
            allowed_shell_patterns: Self::default_allowed_shell(),
            escalate_shell_patterns: Self::default_escalate_shell(),
            forbidden_shell_patterns: Self::default_forbidden_shell(),
        }
    }
}

impl WorkerPermissions {
    /// Permissive permissions for testing/debugging.
    pub fn permissive() -> Self {
        let mut allowed_tools = HashSet::new();
        allowed_tools.insert("*".to_string()); // All tools

        Self {
            allowed_tools,
            escalate_tools: HashSet::new(),
            forbidden_tools: Self::default_forbidden_shell().into_iter().collect(),
            allowed_shell_patterns: vec!["*".to_string()],
            escalate_shell_patterns: vec![],
            forbidden_shell_patterns: Self::default_forbidden_shell(),
        }
    }

    /// Maximum security - workers can only read.
    pub fn restrictive() -> Self {
        let mut allowed_tools = HashSet::new();
        allowed_tools.insert("read_file".to_string());
        allowed_tools.insert("list_files".to_string());

        Self {
            allowed_tools,
            escalate_tools: HashSet::new(),
            forbidden_tools: vec!["*".to_string()].into_iter().collect(),
            allowed_shell_patterns: vec!["ls *".to_string(), "cat *".to_string()],
            escalate_shell_patterns: vec![],
            forbidden_shell_patterns: vec!["*".to_string()],
        }
    }

    fn default_allowed_shell() -> Vec<String> {
        vec![
            "ls *".to_string(),
            "cat *".to_string(),
            "grep *".to_string(),
            "find *".to_string(),
            "head *".to_string(),
            "tail *".to_string(),
            "wc *".to_string(),
            "pwd".to_string(),
            "echo *".to_string(),
            "sort *".to_string(),
            "uniq *".to_string(),
            "awk *".to_string(),
            "sed *".to_string(),
            "git status*".to_string(),
            "git log*".to_string(),
            "git diff*".to_string(),
            "cargo check*".to_string(),
            "cargo test*".to_string(),
            "cargo build*".to_string(),
        ]
    }

    fn default_escalate_shell() -> Vec<String> {
        vec![
            "rm *".to_string(),
            "mv *".to_string(),
            "cp *".to_string(),
            "chmod *".to_string(),
            "curl *".to_string(),
            "wget *".to_string(),
            "docker *".to_string(),
            "git push*".to_string(),
            "git pull*".to_string(),
        ]
    }

    fn default_forbidden_shell() -> Vec<String> {
        vec![
            "sudo *".to_string(),
            "su *".to_string(),
            "rm -rf /".to_string(),
            "rm -rf /*".to_string(),
            "> /dev/sda*".to_string(),
            "mkfs.*".to_string(),
            "dd if=/dev/zero of=/dev/sda*".to_string(),
            "shutdown *".to_string(),
            "reboot *".to_string(),
            ":(){:|:&};:".to_string(), // Fork bomb
            // Windows equivalents
            "format C:*".to_string(),
            "format C: *".to_string(),
            "format c:*".to_string(),
            "format c: *".to_string(),
            "diskpart *".to_string(),
            "restart-computer*".to_string(),
            "stop-computer*".to_string(),
            "shutdown /s*".to_string(),
            "shutdown /r*".to_string(),
        ]
    }

    /// Check tool access.
    pub fn check_tool(&self, tool: &str) -> ToolAccess {
        // Check explicit allowed first (overrides forbidden "*")
        if self.allowed_tools.contains(tool) {
            return ToolAccess::Allowed;
        }

        // Check forbidden (including wildcard "*")
        if self.forbidden_tools.contains(tool) || self.forbidden_tools.contains("*") {
            return ToolAccess::Forbidden(format!("Tool '{}' is forbidden", tool));
        }

        // Check allowed wildcard (permissive mode)
        if self.allowed_tools.contains("*") {
            return ToolAccess::Allowed;
        }

        // Check escalate
        if self.escalate_tools.contains(tool) {
            return ToolAccess::Escalate(format!("Tool '{}' requires Main approval", tool));
        }

        // Default: unknown tools are escalated
        ToolAccess::Escalate(format!("Tool '{}' is not in allowed list", tool))
    }

    /// Check shell command access.
    pub fn check_shell(&self, cmd: &str) -> ShellAccess {
        let cmd_lower = cmd.trim().to_lowercase();

        // Check forbidden first
        for pattern in &self.forbidden_shell_patterns {
            if pattern_matches(&cmd_lower, &pattern.to_lowercase()) {
                return ShellAccess::Forbidden(format!(
                    "Command matches forbidden pattern '{}'",
                    pattern
                ));
            }
        }

        // Check allowed
        for pattern in &self.allowed_shell_patterns {
            if pattern_matches(&cmd_lower, &pattern.to_lowercase()) {
                return ShellAccess::Allowed;
            }
        }

        // Check escalate
        for pattern in &self.escalate_shell_patterns {
            if pattern_matches(&cmd_lower, &pattern.to_lowercase()) {
                return ShellAccess::Escalate(format!(
                    "Command matches restricted pattern '{}'",
                    pattern
                ));
            }
        }

        // Default: unknown commands are escalated
        ShellAccess::Escalate("Unknown command pattern".to_string())
    }
}

/// Pattern matching for shell commands.
///
/// Supports simple wildcards:
/// - `*` matches any sequence
/// - `?` matches single character
fn pattern_matches(cmd: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if !pattern.contains('*') && !pattern.contains('?') {
        return cmd == pattern;
    }

    // Simple wildcard matching
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        return simple_char_match(cmd, pattern);
    }

    let mut pos = 0;
    let mut part_iter = parts.iter().enumerate();

    // First part must match at start
    if let Some((0, first)) = part_iter.next() {
        if !first.is_empty() {
            if !cmd.starts_with(first) {
                return false;
            }
            pos = first.len();
        }
    }

    // Middle parts must appear in order
    for (_, part) in part_iter {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = cmd[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }

    // Last part must match at end
    if let Some((last_idx, last)) = parts.iter().enumerate().next_back() {
        if last_idx > 0 && !last.is_empty() && !cmd.ends_with(last) {
            return false;
        }
    }

    true
}

fn simple_char_match(cmd: &str, pattern: &str) -> bool {
    if cmd.len() != pattern.len() {
        return false;
    }
    cmd.chars()
        .zip(pattern.chars())
        .all(|(c, p)| p == '?' || c == p)
}

/// Authority matrix combining Main and Worker permissions.
#[derive(Debug, Clone)]
pub struct AuthorityMatrix {
    pub main: MainPermissions,
    pub worker: WorkerPermissions,
}

impl Default for AuthorityMatrix {
    fn default() -> Self {
        Self {
            main: MainPermissions::default(),
            worker: WorkerPermissions::default(),
        }
    }
}

impl AuthorityMatrix {
    /// Permissive matrix for testing.
    pub fn permissive() -> Self {
        Self {
            main: MainPermissions::default(),
            worker: WorkerPermissions::permissive(),
        }
    }

    /// Maximum security matrix.
    pub fn restrictive() -> Self {
        Self {
            main: MainPermissions::default(),
            worker: WorkerPermissions::restrictive(),
        }
    }
}

/// Authority enforcement engine.
///
/// Runtime uses this to check permissions before executing tools
/// or shell commands. The AgentId determines which permissions apply.
#[derive(Debug, Clone)]
pub struct Authority {
    matrix: AuthorityMatrix,
}

impl Authority {
    /// Create authority with given matrix.
    pub fn new(matrix: AuthorityMatrix) -> Self {
        Self { matrix }
    }

    /// Create with default permissions.
    pub fn default() -> Self {
        Self::new(AuthorityMatrix::default())
    }

    /// Create permissive authority for testing.
    pub fn permissive() -> Self {
        Self::new(AuthorityMatrix::permissive())
    }

    /// Check tool access for agent.
    pub fn check_tool(&self, agent_id: &AgentId, tool: &str) -> ToolAccess {
        match agent_id.agent_type {
            AgentType::Main => ToolAccess::Allowed,
            AgentType::Worker(_) => self.matrix.worker.check_tool(tool),
        }
    }

    /// Check shell command access for agent.
    pub fn check_shell(&self, agent_id: &AgentId, cmd: &str) -> ShellAccess {
        match agent_id.agent_type {
            AgentType::Main => ShellAccess::Allowed,
            AgentType::Worker(_) => self.matrix.worker.check_shell(cmd),
        }
    }

    /// Check if agent is allowed to spawn workers.
    pub fn can_spawn_workers(&self, agent_id: &AgentId) -> bool {
        matches!(agent_id.agent_type, AgentType::Main)
    }

    /// Check if agent can approve escalations.
    pub fn can_approve(&self, agent_id: &AgentId) -> bool {
        matches!(agent_id.agent_type, AgentType::Main)
    }

    /// Get reference to permission matrix.
    pub fn matrix(&self) -> &AuthorityMatrix {
        &self.matrix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_main_has_full_access() {
        let auth = Authority::default();
        let main = AgentId::main();

        assert_eq!(auth.check_tool(&main, "shell"), ToolAccess::Allowed);
        assert_eq!(auth.check_tool(&main, "write_file"), ToolAccess::Allowed);
        assert_eq!(auth.check_tool(&main, "delete_file"), ToolAccess::Allowed);
        assert_eq!(auth.check_shell(&main, "sudo rm -rf /"), ShellAccess::Allowed);
        assert!(auth.can_spawn_workers(&main));
        assert!(auth.can_approve(&main));
    }

    #[test]
    fn test_worker_allowed_tools() {
        let auth = Authority::default();
        let worker = AgentId::worker("test");

        assert_eq!(auth.check_tool(&worker, "read_file"), ToolAccess::Allowed);
        assert_eq!(auth.check_tool(&worker, "list_files"), ToolAccess::Allowed);
        assert_eq!(auth.check_tool(&worker, "git_status"), ToolAccess::Allowed);
    }

    #[test]
    fn test_worker_escalate_tools() {
        let auth = Authority::default();
        let worker = AgentId::worker("test");

        match auth.check_tool(&worker, "shell") {
            ToolAccess::Escalate(_) => {} // Expected
            other => panic!("Expected Escalate, got {:?}", other),
        }

        match auth.check_tool(&worker, "write_file") {
            ToolAccess::Escalate(_) => {}
            other => panic!("Expected Escalate, got {:?}", other),
        }

        match auth.check_tool(&worker, "delegate") {
            ToolAccess::Escalate(_) => {}
            other => panic!("Expected Escalate, got {:?}", other),
        }
    }

    #[test]
    fn test_worker_cannot_spawn_workers() {
        let auth = Authority::default();
        let worker = AgentId::worker("test");

        assert!(!auth.can_spawn_workers(&worker));
        assert!(!auth.can_approve(&worker));
    }

    #[test]
    fn test_shell_allowed_patterns() {
        let perms = WorkerPermissions::default();

        assert_eq!(perms.check_shell("ls -la"), ShellAccess::Allowed);
        assert_eq!(perms.check_shell("cat file.txt"), ShellAccess::Allowed);
        assert_eq!(perms.check_shell("git status"), ShellAccess::Allowed);
        assert_eq!(perms.check_shell("cargo test"), ShellAccess::Allowed);
    }

    #[test]
    fn test_shell_escalate_patterns() {
        let perms = WorkerPermissions::default();

        match perms.check_shell("rm file.txt") {
            ShellAccess::Escalate(_) => {}
            other => panic!("Expected Escalate, got {:?}", other),
        }

        match perms.check_shell("curl https://example.com") {
            ShellAccess::Escalate(_) => {}
            other => panic!("Expected Escalate, got {:?}", other),
        }
    }

    #[test]
    fn test_shell_forbidden_patterns() {
        let perms = WorkerPermissions::default();

        match perms.check_shell("sudo ls") {
            ShellAccess::Forbidden(_) => {}
            other => panic!("Expected Forbidden, got {:?}", other),
        }

        match perms.check_shell("rm -rf /") {
            ShellAccess::Forbidden(_) => {}
            other => panic!("Expected Forbidden, got {:?}", other),
        }
    }

    #[test]
    fn test_pattern_matching() {
        assert!(pattern_matches("ls -la", "ls *"));
        assert!(pattern_matches("cat file.txt", "cat *"));
        assert!(!pattern_matches("rm file", "cat *"));
        assert!(pattern_matches("anything", "*"));
        assert!(pattern_matches("git status", "git status*"));
        assert!(pattern_matches("git status --short", "git status*"));
    }

    #[test]
    fn test_permissive_worker() {
        let auth = Authority::permissive();
        let worker = AgentId::worker("test");

        assert_eq!(auth.check_tool(&worker, "shell"), ToolAccess::Allowed);
        assert_eq!(auth.check_tool(&worker, "write_file"), ToolAccess::Allowed);
        
        // But forbidden patterns are still blocked
        match auth.check_shell(&worker, "sudo ls") {
            ShellAccess::Forbidden(_) => {}
            other => panic!("Expected Forbidden for sudo, got {:?}", other),
        }
    }

    #[test]
    fn test_restrictive_worker() {
        let auth = Authority::new(AuthorityMatrix::restrictive());
        let worker = AgentId::worker("test");

        assert_eq!(auth.check_tool(&worker, "read_file"), ToolAccess::Allowed);
        
        match auth.check_tool(&worker, "shell") {
            ToolAccess::Forbidden(_) => {}
            other => panic!("Expected Forbidden, got {:?}", other),
        }
    }

    #[test]
    fn test_unknown_tool_escalates() {
        let perms = WorkerPermissions::default();

        match perms.check_tool("unknown_tool") {
            ToolAccess::Escalate(_) => {}
            other => panic!("Expected Escalate for unknown, got {:?}", other),
        }
    }

    #[test]
    fn test_worker_permissions_clone() {
        let perms = WorkerPermissions::default();
        let cloned = perms.clone();
        
        assert_eq!(perms.allowed_tools, cloned.allowed_tools);
        assert_eq!(perms.escalate_tools, cloned.escalate_tools);
    }
}
