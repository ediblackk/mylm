//! Terminal context collection
//!
//! Collects various types of context from the terminal environment:
//! - Working directory and file structure
//! - Git status and repository information
//! - System information (CPU, memory, processes)
//! - Shell history and command patterns

use anyhow::Result;
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

pub mod git;
pub mod system;
pub mod terminal;

/// Collected terminal context
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct TerminalContext {
    /// Working directory
    pub cwd: Option<PathBuf>,

    /// Current user
    pub user: Option<String>,

    /// Hostname
    pub hostname: Option<String>,

    /// Operating system
    pub os: Option<String>,

    /// Git repository information
    #[serde(default)]
    pub git: GitContext,

    /// System information
    #[serde(default)]
    pub system: SystemContext,

    /// Terminal information
    #[serde(default)]
    pub terminal: TerminalContextInfo,

    /// Collection timestamp
    pub collected_at: DateTime<chrono::Utc>,
}

/// Git-specific context
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct GitContext {
    /// Whether we're in a git repository
    pub is_repo: bool,

    /// Current branch name
    pub branch: Option<String>,

    /// Latest commit hash
    pub commit: Option<String>,

    /// Latest commit message
    pub commit_message: Option<String>,

    /// Untracked files count
    pub untracked_count: Option<u32>,

    /// Modified files count
    pub modified_count: Option<u32>,

    /// Staged files count
    pub staged_count: Option<u32>,

    /// Status summary string
    pub status_summary: Option<String>,
}

/// System information context
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct SystemContext {
    /// Total RAM in bytes
    pub total_memory: Option<u64>,

    /// Used RAM in bytes
    pub used_memory: Option<u64>,

    /// Total swap in bytes
    pub total_swap: Option<u64>,

    /// Used swap in bytes
    pub used_swap: Option<u64>,

    /// Number of CPU cores
    pub cpu_count: Option<usize>,

    /// CPU usage percentage
    pub cpu_usage: Option<f32>,

    /// Number of running processes
    pub process_count: Option<u32>,

    /// Load average (1 min)
    pub load_average_1m: Option<f64>,

    /// Load average (5 min)
    pub load_average_5m: Option<f64>,

    /// Load average (15 min)
    pub load_average_15m: Option<f64>,
}

/// Terminal and shell information
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct TerminalContextInfo {
    /// Terminal type (e.g., "xterm-256color")
    pub term: Option<String>,

    /// Shell name (e.g., "bash", "zsh", "fish")
    pub shell: Option<String>,

    /// Shell version
    pub shell_version: Option<String>,

    /// Recent command history (last 10)
    #[serde(default)]
    pub recent_commands: Vec<String>,
}

impl TerminalContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self {
            collected_at: chrono::Utc::now(),
            ..Default::default()
        }
    }

    /// Collect all context information asynchronously
    pub async fn collect() -> Self {
        let mut ctx = Self::new();

        // Collect in parallel where possible
        let (git_info, sys_info, term_info) = tokio::join!(
            git::collect_git_context(),
            system::collect_system_context(),
            terminal::collect_terminal_context()
        );

        // Merge collected data
        if let Ok(info) = git_info {
            ctx.git = info;
        }

        if let Ok(info) = sys_info {
            ctx.system = info;
        }

        if let Ok(info) = term_info {
            ctx.terminal = info;
        }

        // Get current working directory
        ctx.cwd = std::env::current_dir().ok();

        // Get user and hostname
        ctx.user = std::env::var("USER").ok().or_else(|| {
            std::env::var("USERNAME").ok().map(|u| {
                if let Some(idx) = u.rfind('\\') {
                    u[idx + 1..].to_string()
                } else {
                    u
                }
            })
        });

        ctx.hostname = hostname::get()
            .ok()
            .and_then(|h| h.to_str().map(|s| s.to_string()));

        ctx.os = Some(format!(
            "{} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));

        ctx
    }

    /// Build a prompt for the LLM with all context
    pub fn build_prompt(&self, user_query: &str) -> String {
        let mut prompt = String::new();

        // System context
        prompt.push_str("## System Context\n");
        if let Some(ref cwd) = self.cwd {
            prompt.push_str(&format!("- Working Directory: {}\n", cwd.display()));
        }
        if let Some(ref user) = self.user {
            prompt.push_str(&format!("- User: {}\n", user));
        }
        if let Some(ref hostname) = self.hostname {
            prompt.push_str(&format!("- Host: {}\n", hostname));
        }
        if let Some(ref os) = self.os {
            prompt.push_str(&format!("- OS: {}\n", os));
        }

        // Git context
        prompt.push_str("\n## Git Context\n");
        if self.git.is_repo {
            if let Some(ref branch) = self.git.branch {
                prompt.push_str(&format!("- Branch: {}\n", branch));
            }
            if let Some(ref status) = self.git.status_summary {
                prompt.push_str(&format!("- Status: {}\n", status));
            }
            if let Some(ref commit) = self.git.commit {
                prompt.push_str(&format!("- Latest Commit: {}\n", commit));
            }
        } else {
            prompt.push_str("- Not in a git repository\n");
        }

        // System resources
        prompt.push_str("\n## System Resources\n");
        if let Some(mem) = self.system.total_memory {
            let used = self.system.used_memory.unwrap_or(0);
            let used_gb = used as f64 / (1024.0 * 1024.0 * 1024.0);
            let total_gb = mem as f64 / (1024.0 * 1024.0 * 1024.0);
            prompt.push_str(&format!("- Memory: {:.2} GB / {:.2} GB used\n", used_gb, total_gb));
        }
        if let Some(count) = self.system.cpu_count {
            prompt.push_str(&format!("- CPU Cores: {}\n", count));
        }
        if let Some(usage) = self.system.cpu_usage {
            prompt.push_str(&format!("- CPU Usage: {:.1}%\n", usage));
        }
        if let Some(count) = self.system.process_count {
            prompt.push_str(&format!("- Running Processes: {}\n", count));
        }

        // Terminal info
        prompt.push_str("\n## Terminal Info\n");
        if let Some(ref shell) = self.terminal.shell {
            prompt.push_str(&format!("- Shell: {}\n", shell));
        }
        if let Some(ref term) = self.terminal.term {
            prompt.push_str(&format!("- Terminal: {}\n", term));
        }

        // Recent commands
        if !self.terminal.recent_commands.is_empty() {
            prompt.push_str("\n## Recent Commands\n");
            for cmd in self.terminal.recent_commands.iter().take(5) {
                prompt.push_str(&format!("- {}\n", cmd));
            }
        }

        // User query
        prompt.push_str("\n## User Query\n");
        prompt.push_str(user_query);

        prompt.push_str("\n\nPlease provide a helpful response considering the above context.");

        prompt
    }

    /// Get current working directory as string
    pub fn cwd(&self) -> Option<String> {
        self.cwd.as_ref().map(|p| p.to_string_lossy().to_string())
    }

    /// Get git branch
    pub fn git_branch(&self) -> Option<String> {
        self.git.branch.clone()
    }

    /// Get git status summary
    pub fn git_status(&self) -> Option<String> {
        self.git.status_summary.clone()
    }

    /// Get formatted system summary
    pub fn system_summary(&self) -> String {
        let mut summary = String::new();

        if let Some(mem) = self.system.total_memory {
            let used = self.system.used_memory.unwrap_or(0);
            let used_gb = used as f64 / (1024.0 * 1024.0 * 1024.0);
            let total_gb = mem as f64 / (1024.0 * 1024.0 * 1024.0);
            summary.push_str(&format!("RAM: {:.1}/{:.1}GB, ", used_gb, total_gb));
        }

        if let Some(count) = self.system.cpu_count {
            summary.push_str(&format!("{} cores, ", count));
        }

        if let Some(usage) = self.system.cpu_usage {
            summary.push_str(&format!("CPU: {:.1}%", usage));
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_new() {
        let ctx = TerminalContext::new();
        assert!(ctx.cwd.is_none());
        assert!(!ctx.git.is_repo);
    }

    #[test]
    fn test_build_prompt() {
        let ctx = TerminalContext {
            cwd: Some(PathBuf::from("/home/user/project")),
            user: Some("testuser".to_string()),
            hostname: Some("testhost".to_string()),
            os: Some("linux x86_64".to_string()),
            git: GitContext {
                is_repo: true,
                branch: Some("main".to_string()),
                commit: Some("abc123".to_string()),
                status_summary: Some("clean".to_string()),
                ..Default::default()
            },
            system: SystemContext {
                total_memory: Some(16_000_000_000),
                used_memory: Some(8_000_000_000),
                cpu_count: Some(8),
                cpu_usage: Some(25.5),
                process_count: Some(150),
                ..Default::default()
            },
            terminal: TerminalContextInfo {
                shell: Some("bash".to_string()),
                term: Some("xterm-256color".to_string()),
                recent_commands: vec!["ls -la".to_string(), "git status".to_string()],
            },
            collected_at: chrono::Utc::now(),
        };

        let prompt = ctx.build_prompt("What is this project?");
        assert!(prompt.contains("/home/user/project"));
        assert!(prompt.contains("testuser"));
        assert!(prompt.contains("main"));
        assert!(prompt.contains("What is this project?"));
    }
}
