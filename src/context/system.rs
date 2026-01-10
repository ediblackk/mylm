//! System information collection module
//!
//! Collects system information using the sysinfo crate for cross-platform
//! monitoring capabilities.

use anyhow::{Context, Result};
use serde::Serialize;
use sysinfo::{Disks, System};
use crate::context::SystemContext;

/// System information container
#[derive(Debug, Serialize, Clone)]
pub struct SystemInfo {
    /// Operating system name
    pub os_name: String,
    /// OS version
    pub os_version: String,
    /// Kernel version
    pub kernel_version: String,
    /// Hostname
    pub hostname: String,
    /// CPU model name
    pub cpu_model: String,
    /// Number of CPU cores
    pub cpu_cores: usize,
    /// CPU usage percentage
    pub cpu_usage: f32,
    /// Total memory in bytes
    pub total_memory: u64,
    /// Used memory in bytes
    pub used_memory: u64,
    /// Total swap in bytes
    pub total_swap: u64,
    /// Used swap in bytes
    pub used_swap: u64,
    /// Disk information
    pub disks: Vec<DiskInfo>,
}

/// Disk information
#[derive(Debug, Serialize, Clone)]
pub struct DiskInfo {
    /// Device name
    pub name: String,
    /// Mount point
    pub mount_point: String,
    /// Total disk space in bytes
    pub total_space: u64,
    /// Available disk space in bytes
    pub available_space: u64,
    /// Used disk space in bytes
    pub used_space: u64,
    /// Disk usage percentage
    pub usage_percent: f64,
}

impl SystemInfo {
    /// Create a new SystemInfo instance by querying the system
    pub fn new() -> Result<Self> {
        let mut system = System::new_all();

        // Refresh all system information
        system.refresh_all();

        // Get CPU information
        let cpu_model = system.global_cpu_info().name().to_string();
        let cpu_cores = system.physical_core_count().unwrap_or(0);
        let cpu_usage = system.global_cpu_info().cpu_usage();

        // Get memory information
        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let total_swap = system.total_swap();
        let used_swap = system.used_swap();

        // Get disk information
        let disks_obj = Disks::new_with_refreshed_list();
        let disks = disks_obj
            .iter()
            .map(|d| DiskInfo {
                name: d.name().to_string_lossy().to_string(),
                mount_point: d.mount_point().to_string_lossy().to_string(),
                total_space: d.total_space(),
                available_space: d.available_space(),
                used_space: d.total_space() - d.available_space(),
                usage_percent: if d.total_space() > 0 {
                    ((d.total_space() - d.available_space()) as f64 / d.total_space() as f64) * 100.0
                } else {
                    0.0
                },
            })
            .collect();

        Ok(SystemInfo {
            os_name: System::name()
                .unwrap_or_else(|| String::from("Unknown")),
            os_version: System::os_version()
                .unwrap_or_else(|| String::from("Unknown")),
            kernel_version: System::kernel_version()
                .unwrap_or_else(|| String::from("Unknown")),
            hostname: System::host_name()
                .unwrap_or_else(|| String::from("Unknown")),
            cpu_model,
            cpu_cores,
            cpu_usage,
            total_memory,
            used_memory,
            total_swap,
            used_swap,
            disks,
        })
    }

    /// Get memory usage as a formatted string
    #[allow(dead_code)]
    pub fn memory_usage(&self) -> String {
        format!(
            "Memory: {}/{} ({:.1}%)",
            Self::format_bytes(self.used_memory),
            Self::format_bytes(self.total_memory),
            if self.total_memory > 0 {
                (self.used_memory as f64 / self.total_memory as f64) * 100.0
            } else {
                0.0
            }
        )
    }

    /// Get swap usage as a formatted string
    #[allow(dead_code)]
    pub fn swap_usage(&self) -> String {
        format!(
            "Swap: {}/{} ({:.1}%)",
            Self::format_bytes(self.used_swap),
            Self::format_bytes(self.total_swap),
            if self.total_swap > 0 {
                (self.used_swap as f64 / self.total_swap as f64) * 100.0
            } else {
                0.0
            }
        )
    }

    /// Get disk usage as a formatted string
    #[allow(dead_code)]
    pub fn disk_usage(&self) -> Vec<String> {
        self.disks
            .iter()
            .map(|d| {
                format!(
                    "{} ({}) : {}/{} ({:.1}%)",
                    d.name,
                    d.mount_point,
                    Self::format_bytes(d.used_space),
                    Self::format_bytes(d.total_space),
                    d.usage_percent
                )
            })
            .collect()
    }

    /// Format bytes to human readable format
    #[allow(dead_code)]
    pub fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        if bytes >= TB {
            format!("{:.2} TB", bytes as f64 / TB as f64)
        } else if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| SystemInfo {
            os_name: String::from("Unknown"),
            os_version: String::from("Unknown"),
            kernel_version: String::from("Unknown"),
            hostname: String::from("Unknown"),
            cpu_model: String::from("Unknown"),
            cpu_cores: 0,
            cpu_usage: 0.0,
            total_memory: 0,
            used_memory: 0,
            total_swap: 0,
            used_swap: 0,
            disks: Vec::new(),
        })
    }
}

/// Get basic system information (lightweight, non-blocking)
#[allow(dead_code)]
pub fn get_system_info() -> Result<SystemInfo> {
    SystemInfo::new()
        .context("Failed to collect system information")
}

/// Get a brief system summary for AI context
#[allow(dead_code)]
pub fn get_system_summary() -> Result<String> {
    let info = SystemInfo::new()?;

    let mut summary = vec![
        format!("OS: {} {} (Kernel: {})", info.os_name, info.os_version, info.kernel_version),
        format!("Hostname: {}", info.hostname),
        format!("CPU: {} ({} cores) - Usage: {:.1}%", info.cpu_model, info.cpu_cores, info.cpu_usage),
        info.memory_usage(),
        info.swap_usage(),
    ];

    // Add disk information
    let disk_lines = info.disk_usage();
    if !disk_lines.is_empty() {
        summary.push("Disks:".to_string());
        for line in disk_lines {
            summary.push(format!("  - {}", line));
        }
    }

    Ok(summary.join("\n"))
}

/// Collect system context for terminal AI
pub async fn collect_system_context() -> Result<SystemContext> {
    let mut system = System::new_all();
    system.refresh_all();

    Ok(SystemContext {
        total_memory: Some(system.total_memory()),
        used_memory: Some(system.used_memory()),
        total_swap: Some(system.total_swap()),
        used_swap: Some(system.used_swap()),
        cpu_count: Some(system.cpus().len()),
        cpu_usage: Some(system.global_cpu_info().cpu_usage()),
        process_count: Some(system.processes().len() as u32),
        load_average_1m: Some(System::load_average().one),
        load_average_5m: Some(System::load_average().five),
        load_average_15m: Some(System::load_average().fifteen),
    })
}
