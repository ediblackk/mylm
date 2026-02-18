//! Environment context collection
//!
//! Collects information about the user's terminal environment:
//! - Working directory and file structure
//! - System information (CPU, memory, processes)
//! - Shell history and command patterns
//! - Terminal content for LLM context

pub mod system;
pub mod terminal;
pub mod pack;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use system::SystemContext;
pub use terminal::TerminalContext;
pub use pack::{ContextPack, ContextBuilder};

/// Collected terminal environment context
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct EnvironmentContext {
    /// Working directory
    pub cwd: Option<PathBuf>,

    /// Current user
    pub user: Option<String>,

    /// Hostname
    pub hostname: Option<String>,

    /// Operating system
    pub os: Option<String>,

    /// System information
    #[serde(default)]
    pub system: SystemContext,

    /// Terminal information
    #[serde(default)]
    pub terminal: TerminalContext,

    /// Collection timestamp
    pub collected_at: DateTime<Utc>,
}

impl EnvironmentContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self {
            collected_at: Utc::now(),
            ..Default::default()
        }
    }

    /// Collect all context information asynchronously
    pub async fn collect() -> Self {
        let mut ctx = Self::new();

        // Collect in parallel where possible
        let (sys_info, term_info) = tokio::join!(
            system::collect_system_context(),
            terminal::collect_terminal_context()
        );

        // Merge collected data
        if let Ok(info) = sys_info {
            ctx.system = info;
        }

        // Get current working directory
        ctx.cwd = std::env::current_dir().ok();

        if let Ok(info) = term_info {
            ctx.terminal = info;
        } else if let Some(ref cwd) = ctx.cwd {
            // Fallback: update terminal info CWD if collection failed
            ctx.terminal.current_dir = cwd.clone();
            ctx.terminal.current_dir_str = cwd.to_string_lossy().to_string();
        }

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
        prompt.push_str(&format!("- Current Dir: {}\n", self.terminal.current_dir_str));

        // Recent commands
        if !self.terminal.command_history.is_empty() {
            prompt.push_str("\n## Recent Commands\n");
            for cmd in self.terminal.command_history.iter().take(5) {
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

    /// Collect all context information synchronously (best effort, non-blocking parts)
    pub fn collect_sync() -> Self {
        let mut ctx = Self::new();
        
        // Use std::env for quick sync access
        ctx.cwd = std::env::current_dir().ok();
        ctx.user = std::env::var("USER").ok();
        ctx.os = Some(format!("{} {}", std::env::consts::OS, std::env::consts::ARCH));

        // Note: system info is skipped or minimal in sync version
        // to avoid blocking the main thread significantly.

        // Collect terminal-specific context synchronously
        if let Ok(term_info) = terminal::TerminalContext::new() {
            ctx.terminal = term_info;
        }

        ctx
    }
}
