//! Command allowlist configuration
//!
//! Defines which commands are allowed to be executed

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Command categories for the allowlist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandAllowlist {
    /// Read-only commands (safe by default)
    read_commands: HashSet<String>,
    /// Limited write commands (require confirmation)
    write_commands: HashSet<String>,
    /// System commands (require --force)
    system_commands: HashSet<String>,
    /// File operations (require --force)
    file_ops_commands: HashSet<String>,
    /// Network commands (require --force)
    network_commands: HashSet<String>,
    /// Custom allowed commands
    custom_allowed: HashSet<String>,
    /// Custom blocked commands
    custom_blocked: HashSet<String>,
}

impl Default for CommandAllowlist {
    fn default() -> Self {
        CommandAllowlist {
            // Safe read-only commands
            read_commands: HashSet::from([
                "ls".to_string(),
                "cat".to_string(),
                "head".to_string(),
                "tail".to_string(),
                "grep".to_string(),
                "find".to_string(),
                "which".to_string(),
                "whereis".to_string(),
                "file".to_string(),
                "stat".to_string(),
                "wc".to_string(),
                "cut".to_string(),
                "sort".to_string(),
                "uniq".to_string(),
                "tr".to_string(),
                "date".to_string(),
                "whoami".to_string(),
                "hostname".to_string(),
                "pwd".to_string(),
                "echo".to_string(),
                "printf".to_string(),
            ]),
            // Limited write commands
            write_commands: HashSet::from([
                "echo".to_string(),
                "printf".to_string(),
            ]),
            // System information commands
            system_commands: HashSet::from([
                "ps".to_string(),
                "top".to_string(),
                "htop".to_string(),
                "free".to_string(),
                "df".to_string(),
                "du".to_string(),
                "uname".to_string(),
                "uptime".to_string(),
                "vmstat".to_string(),
                "iostat".to_string(),
                "lspci".to_string(),
                "lsusb".to_string(),
                "dmidecode".to_string(),
                "cpuinfo".to_string(),
                "meminfo".to_string(),
            ]),
            // File operations (potentially dangerous)
            file_ops_commands: HashSet::from([
                "mkdir".to_string(),
                "rmdir".to_string(),
                "touch".to_string(),
                "cp".to_string(),
                "mv".to_string(),
                "rm".to_string(),
                "chmod".to_string(),
                "chown".to_string(),
                "chgrp".to_string(),
                "ln".to_string(),
                "unlink".to_string(),
            ]),
            // Network commands
            network_commands: HashSet::from([
                "ping".to_string(),
                "traceroute".to_string(),
                "mtr".to_string(),
                "nslookup".to_string(),
                "dig".to_string(),
                "curl".to_string(),
                "wget".to_string(),
                "ss".to_string(),
                "netstat".to_string(),
                "ip".to_string(),
                "ifconfig".to_string(),
            ]),
            // Custom allowlist
            custom_allowed: HashSet::new(),
            // Custom blocklist
            custom_blocked: HashSet::new(),
        }
    }
}

#[allow(dead_code)]
impl CommandAllowlist {
    /// Create a new allowlist with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply allow/deny lists from configuration (exact command names).
    pub fn apply_config(&mut self, cfg: &crate::config::CommandConfig) {
        self.custom_allowed.extend(cfg.allowed_commands.iter().cloned());
        self.custom_blocked.extend(cfg.blocked_commands.iter().cloned());
    }

    /// Check if a command is allowed
    pub fn is_allowed(&self, command: &str) -> bool {
        let cmd = command.trim();

        // Check custom blocklist first
        if self.custom_blocked.contains(cmd) {
            return false;
        }

        // Check custom allowlist
        if self.custom_allowed.contains(cmd) {
            return true;
        }

        // Check built-in categories
        self.read_commands.contains(cmd)
            || self.write_commands.contains(cmd)
            || self.system_commands.contains(cmd)
            || self.file_ops_commands.contains(cmd)
            || self.network_commands.contains(cmd)
    }

    /// Get the safety level of a command
    pub fn get_safety_level(&self, command: &str) -> AllowlistLevel {
        let cmd = command.trim();

        if self.custom_blocked.contains(cmd) {
            return AllowlistLevel::Blocked;
        }

        if self.custom_allowed.contains(cmd) {
            return AllowlistLevel::CustomAllowed;
        }

        if self.read_commands.contains(cmd) {
            return AllowlistLevel::Safe;
        }

        if self.system_commands.contains(cmd) {
            return AllowlistLevel::SystemInfo;
        }

        if self.write_commands.contains(cmd) {
            return AllowlistLevel::LimitedWrite;
        }

        // File ops and network require confirmation
        if self.file_ops_commands.contains(cmd) || self.network_commands.contains(cmd) {
            return AllowlistLevel::RequiresForce;
        }

        AllowlistLevel::Unknown
    }

    /// Add a command to the custom allowlist
    pub fn add_allowed(&mut self, command: impl Into<String>) {
        self.custom_allowed.insert(command.into());
    }

    /// Add a command to the custom blocklist
    pub fn add_blocked(&mut self, command: impl Into<String>) {
        self.custom_blocked.insert(command.into());
    }

    /// Remove a command from custom allowlist
    pub fn remove_allowed(&mut self, command: &str) {
        self.custom_allowed.remove(command);
    }

    /// Remove a command from custom blocklist
    pub fn remove_blocked(&mut self, command: &str) {
        self.custom_blocked.remove(command);
    }

    /// Get all allowed read commands
    pub fn read_commands(&self) -> impl Iterator<Item = &str> {
        self.read_commands.iter().map(|s| s.as_str())
    }

    /// Get all file operation commands
    pub fn file_ops_commands(&self) -> impl Iterator<Item = &str> {
        self.file_ops_commands.iter().map(|s| s.as_str())
    }
}

/// Allowlist safety levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AllowlistLevel {
    /// Completely blocked
    Blocked,
    /// Safe to execute
    Safe,
    /// System information commands
    SystemInfo,
    /// Limited write operations
    LimitedWrite,
    /// Requires --force flag
    RequiresForce,
    /// Custom allowed command
    CustomAllowed,
    /// Unknown command
    Unknown,
}

#[allow(dead_code)]
impl AllowlistLevel {
    /// Check if this level requires force
    pub fn requires_force(&self) -> bool {
        match self {
            AllowlistLevel::RequiresForce | AllowlistLevel::Unknown => true,
            _ => false,
        }
    }

    /// Get a human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            AllowlistLevel::Blocked => "blocked",
            AllowlistLevel::Safe => "safe",
            AllowlistLevel::SystemInfo => "system info",
            AllowlistLevel::LimitedWrite => "limited write",
            AllowlistLevel::RequiresForce => "requires --force",
            AllowlistLevel::CustomAllowed => "custom allowed",
            AllowlistLevel::Unknown => "unknown",
        }
    }
}
