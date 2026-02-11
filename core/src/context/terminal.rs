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
    /// Raw terminal scrollback (from tmux)
    pub raw_scrollback: Option<String>,
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
    fn log_debug(msg: &str) {
        if let Some(home) = dirs::home_dir() {
            let log_dir = home.join(".config").join("mylm");
            let _ = std::fs::create_dir_all(&log_dir);
            let log_file = log_dir.join("debug.log");
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file)
            {
                use std::io::Write;
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                let _ = writeln!(file, "[{}] {}", timestamp, msg);
            }
        }
    }

    /// Create a new TerminalContext by collecting all information
    pub fn new() -> Result<Self> {
        let current_dir = env::current_dir()
            .unwrap_or_else(|_| {
                Self::log_debug("Failed to get current_dir, falling back to '.'");
                PathBuf::from(".")
            });

        let current_dir_str = current_dir
            .to_string_lossy()
            .to_string();

        Self::log_debug(&format!("Collecting context for CWD: {}", current_dir_str));

        // Get directory listing (fall back to error message if fails)
        let directory_listing = Self::get_directory_listing(&current_dir)
            .unwrap_or_else(|e| {
                Self::log_debug(&format!("Error listing directory: {}", e));
                format!("Error listing directory: {}", e)
            });

        // Get command history from shell (optional)
        let command_history = Self::get_shell_history().unwrap_or_default();

        // Get file-based shell history (optional)
        let shell_history = Self::get_file_based_history().unwrap_or_default();

        // Get running processes (optional)
        let processes = Self::get_processes().unwrap_or_default();

        // Get network connections (optional)
        let network_connections = Self::get_network_connections().unwrap_or_default();

        // Get tmux scrollback (optional)
        let raw_scrollback = Self::get_tmux_scrollback();

        Ok(TerminalContext {
            current_dir,
            current_dir_str,
            directory_listing,
            command_history,
            shell_history,
            processes,
            network_connections,
            raw_scrollback,
        })
    }

    /// Get directory listing with detailed information
    fn get_directory_listing(path: &PathBuf) -> Result<String> {
        use std::fs;
        use std::os::unix::fs::MetadataExt;
        use chrono::{DateTime, Local};

        let mut entries = Vec::new();

        match fs::read_dir(path) {
            Ok(read_dir) => {
                for entry in read_dir.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let metadata = entry.metadata().ok();
                    
                    let (size, modified, mode) = if let Some(m) = metadata {
                        let size = m.len();
                        let modified: DateTime<Local> = m.modified()
                            .map(DateTime::from)
                            .unwrap_or_else(|_| DateTime::from(std::time::SystemTime::UNIX_EPOCH));
                        let mode = m.mode();
                        (size, modified.format("%b %d %H:%M").to_string(), mode)
                    } else {
                        (0, "unknown".to_string(), 0)
                    };

                    let file_type = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { "d" } else { "-" };
                    
                    entries.push(format!(
                        "{}{:o} {:>8} {} {}",
                        file_type,
                        mode & 0o777,
                        size,
                        modified,
                        name
                    ));
                }
            }
            Err(e) => return Ok(format!("Error reading directory: {}", e)),
        }

        if entries.is_empty() {
            return Ok("(Directory is empty)".to_string());
        }

        entries.sort();
        Ok(entries.join("\n"))
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
            .args(["aux", "--no-headers"])
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
            .args(["-tuln", "-p"])
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

    /// Check if currently running inside a tmux session
    pub fn is_inside_tmux() -> bool {
        env::var("TMUX").is_ok()
    }

    /// Get tmux scrollback history if running inside tmux
    fn get_tmux_scrollback() -> Option<String> {
        if !Self::is_inside_tmux() {
            return None;
        }

        let output = Command::new("tmux")
            .args(["capture-pane", "-p", "-S", "-300", "-e", "-J"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;

        if output.status.success() {
            let scrollback = String::from_utf8_lossy(&output.stdout).to_string();
            if scrollback.trim().is_empty() {
                None
            } else {
                // Trim trailing whitespace from each line to avoid phantom width
                let trimmed = scrollback
                    .lines()
                    .map(|line| line.trim_end())
                    .collect::<Vec<_>>()
                    .join("\r\n");
                Some(trimmed)
            }
        } else {
            None
        }
    }

    /// Get a formatted summary for AI context
    
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
            raw_scrollback: None,
        }
    }
}

/// Get terminal context (lightweight, non-blocking)

pub fn get_terminal_context() -> Result<TerminalContext> {
    TerminalContext::new()
        .context("Failed to collect terminal context")
}

/// Get current working directory

pub fn get_current_dir() -> Result<PathBuf> {
    env::current_dir().context("Failed to get current directory")
}

pub async fn collect_terminal_context() -> Result<TerminalContext> {
    tokio::task::spawn_blocking(TerminalContext::new)
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("Task failed: {}", e)))
}
