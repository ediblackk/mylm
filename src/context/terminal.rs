//! Terminal context collection module
//!
//! Collects terminal and shell context information including current directory,
//! file listing, command history, and process information.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Terminal context information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TerminalContext {
    /// Current working directory
    pub current_dir: PathBuf,
    /// Current directory as string
    pub current_dir_str: String,
    /// Directory listing
    pub directory_listing: String,
    /// Command history (last 20 commands)
    pub command_history: Vec<String>,
    /// Recent shell history file content
    pub shell_history: Vec<String>,
    /// Running processes
    pub processes: Vec<ProcessInfo>,
    /// Network connections
    pub network_connections: Vec<NetworkInfo>,
}

/// Process information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProcessInfo {
    /// Process ID
    pub pid: String,
    /// User running the process
    pub user: String,
    /// CPU percentage
    pub cpu: String,
    /// Memory percentage
    pub mem: String,
    /// Command line
    pub command: String,
}

/// Network connection information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkInfo {
    /// Protocol (TCP/UDP)
    pub proto: String,
    /// Local address
    pub local_addr: String,
    /// Remote address
    pub remote_addr: String,
    /// State
    pub state: String,
    /// Process ID if available
    pub pid: Option<String>,
}

impl TerminalContext {
    /// Create a new TerminalContext by collecting all information
    pub fn new() -> Result<Self> {
        let current_dir = env::current_dir()
            .context("Failed to get current directory")?;

        let current_dir_str = current_dir
            .to_string_lossy()
            .to_string();

        // Get directory listing
        let directory_listing = Self::get_directory_listing(&current_dir)?;

        // Get command history from shell
        let command_history = Self::get_shell_history()?;

        // Get file-based shell history
        let shell_history = Self::get_file_based_history()?;

        // Get running processes
        let processes = Self::get_processes()?;

        // Get network connections
        let network_connections = Self::get_network_connections()?;

        Ok(TerminalContext {
            current_dir,
            current_dir_str,
            directory_listing,
            command_history,
            shell_history,
            processes,
            network_connections,
        })
    }

    /// Get directory listing with detailed information
    fn get_directory_listing(path: &PathBuf) -> Result<String> {
        let output = Command::new("ls")
            .args(&["-la", "--color=never"])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .context("Failed to list directory")?;

        String::from_utf8(output.stdout)
            .context("Directory listing is not valid UTF-8")
    }

    /// Get command history from the shell
    fn get_shell_history() -> Result<Vec<String>> {
        // Try to get history from bash or zsh
        let output = Command::new("history")
            .arg("1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .context("Failed to get shell history")?;

        let history_text = String::from_utf8(output.stdout)
            .context("History output is not valid UTF-8")?;

        // Parse history entries (format: "  command_number command")
        let history: Vec<String> = history_text
            .lines()
            .rev()
            .take(20)
            .map(|line| {
                // Remove line numbers and trim
                let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
                if parts.len() > 1 {
                    parts[1].to_string()
                } else {
                    line.trim().to_string()
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        Ok(history)
    }

    /// Get shell history from history file
    fn get_file_based_history() -> Result<Vec<String>> {
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let history_files = vec![
            home_dir.join(".bash_history"),
            home_dir.join(".zsh_history"),
            home_dir.join(".history"),
        ];

        for history_file in &history_files {
            if history_file.exists() {
                if let Ok(content) = std::fs::read_to_string(history_file) {
                    let commands: Vec<String> = content
                        .lines()
                        .rev()
                        .take(50)
                        .map(|s| s.to_string())
                        .collect();
                    return Ok(commands);
                }
            }
        }

        Ok(Vec::new())
    }

    /// Get running processes
    fn get_processes() -> Result<Vec<ProcessInfo>> {
        let output = Command::new("ps")
            .args(&["aux", "--no-headers"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .context("Failed to get process list")?;

        let processes_text = String::from_utf8(output.stdout)
            .context("Process output is not valid UTF-8")?;

        let processes: Vec<ProcessInfo> = processes_text
            .lines()
            .take(20)
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 11 {
                    Some(ProcessInfo {
                        pid: parts[1].to_string(),
                        user: parts[0].to_string(),
                        cpu: parts[2].to_string(),
                        mem: parts[3].to_string(),
                        command: parts[10..].join(" "),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(processes)
    }

    /// Get network connections
    fn get_network_connections() -> Result<Vec<NetworkInfo>> {
        let output = Command::new("ss")
            .args(&["-tuln", "-p"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .context("Failed to get network connections")?;

        let connections_text = String::from_utf8(output.stdout)
            .context("Network output is not valid UTF-8")?;

        let connections: Vec<NetworkInfo> = connections_text
            .lines()
            .skip(1) // Skip header line
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    // Parse local address (format: IP:PORT)
                    let local_addr = parts[4].to_string();

                    Some(NetworkInfo {
                        proto: parts[0].to_string(),
                        local_addr,
                        remote_addr: String::new(),
                        state: String::new(),
                        pid: None,
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(connections)
    }

    /// Get a formatted summary for AI context
    #[allow(dead_code)]
    pub fn get_summary(&self) -> String {
        let mut summary = vec![
            format!("Current Directory: {}", self.current_dir_str),
            format!("Directory Listing:\n{}", self.directory_listing),
        ];

        if !self.command_history.is_empty() {
            summary.push(format!(
                "Recent Commands:\n{}",
                self.command_history
                    .iter()
                    .map(|c| format!("  - {}", c))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.shell_history.is_empty() {
            summary.push(format!(
                "Shell History (last 5):\n{}",
                self.shell_history
                    .iter()
                    .rev()
                    .take(5)
                    .map(|c| format!("  - {}", c))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.processes.is_empty() {
            summary.push(format!(
                "Top Processes:\n{}",
                self.processes
                    .iter()
                    .take(10)
                    .map(|p| format!(
                        "  {} {} - {} (CPU: {}, MEM: {})",
                        p.pid, p.user, p.command, p.cpu, p.mem
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.network_connections.is_empty() {
            summary.push(format!(
                "Listening Ports:\n{}",
                self.network_connections
                    .iter()
                    .map(|n| format!("  {} - {}", n.proto, n.local_addr))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        summary.join("\n\n")
    }
}

impl Default for TerminalContext {
    fn default() -> Self {
        TerminalContext {
            current_dir: PathBuf::from("/"),
            current_dir_str: String::from("/"),
            directory_listing: String::new(),
            command_history: Vec::new(),
            shell_history: Vec::new(),
            processes: Vec::new(),
            network_connections: Vec::new(),
        }
    }
}

/// Get terminal context (lightweight, non-blocking)
#[allow(dead_code)]
pub fn get_terminal_context() -> Result<TerminalContext> {
    TerminalContext::new()
        .context("Failed to collect terminal context")
}

/// Get current working directory
#[allow(dead_code)]
pub fn get_current_dir() -> Result<PathBuf> {
    env::current_dir().context("Failed to get current directory")
}

pub async fn collect_terminal_context() -> Result<TerminalContext> {
    tokio::task::spawn_blocking(TerminalContext::new)
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("Task failed: {}", e)))
}
