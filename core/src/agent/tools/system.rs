use crate::agent::tool::{Tool, ToolKind, ToolOutput};
use async_trait::async_trait;
use std::error::Error as StdError;
use sysinfo::{CpuRefreshKind, Networks, ProcessRefreshKind, RefreshKind, System};

/// A tool for monitoring system resources, processes, and network statistics.
pub struct SystemMonitorTool;

impl SystemMonitorTool {
    pub fn new() -> Self {
        Self
    }

    async fn system_resources(&self) -> String {
        let mut sys = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(sysinfo::MemoryRefreshKind::everything())
        );
        
        // Refresh CPU twice with a small delay to get accurate usage
        sys.refresh_cpu_usage();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        sys.refresh_cpu_usage();

        let total_mem = sys.total_memory() / 1024 / 1024;
        let used_mem = sys.used_memory() / 1024 / 1024;
        let cpu_usage = sys.global_cpu_info().cpu_usage();
        let uptime = System::uptime();
        let days = uptime / 86400;
        let hours = (uptime % 86400) / 3600;
        let minutes = (uptime % 3600) / 60;

        format!(
            "📊 System Resources:\n\
             - CPU Usage: {:.2}%\n\
             - RAM Usage: {}/{} MB ({:.2}%)\n\
             - Uptime: {}d {}h {}m ({} seconds)",
            cpu_usage,
            used_mem,
            total_mem,
            (used_mem as f32 / total_mem as f32) * 100.0,
            days, hours, minutes, uptime
        )
    }

    fn process_list(&self, limit: usize) -> String {
        let mut sys = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything())
        );
        sys.refresh_processes();

        let mut processes: Vec<_> = sys.processes().values().collect();
        // Sort by CPU usage descending
        processes.sort_by(|a, b| b.cpu_usage().partial_cmp(&a.cpu_usage()).unwrap_or(std::cmp::Ordering::Equal));

        let mut output = format!("🔝 Top {} Processes by CPU:\n\n", limit);
        output.push_str(&format!("{:<8} {:<25} {:<10} {:<10}\n", "PID", "Name", "CPU %", "Mem MB"));
        output.push_str(&format!("{:-<8} {:-<25} {:-<10} {:-<10}\n", "", "", "", ""));
        
        for p in processes.iter().take(limit) {
            output.push_str(&format!(
                "{:<8} {:<25} {:<10.2} {:<10.2}\n",
                p.pid(),
                p.name().chars().take(25).collect::<String>(),
                p.cpu_usage(),
                p.memory() as f32 / 1024.0 / 1024.0
            ));
        }
        output
    }

    fn network_stats(&self) -> String {
        let networks = Networks::new_with_refreshed_list();
        let mut output = "🌐 Network Interface Stats:\n\n".to_string();
        output.push_str(&format!("{:<15} {:<15} {:<15}\n", "Interface", "Received", "Transmitted"));
        output.push_str(&format!("{:-<15} {:-<15} {:-<15}\n", "", "", ""));

        for (interface_name, data) in &networks {
            output.push_str(&format!(
                "{:<15} {:<15} {:<15}\n",
                interface_name.chars().take(15).collect::<String>(),
                format_bytes(data.received()),
                format_bytes(data.transmitted())
            ));
        }
        output
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f32 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes as f32 / 1024.0 / 1024.0)
    } else {
        format!("{:.2} GB", bytes as f32 / 1024.0 / 1024.0 / 1024.0)
    }
}

#[async_trait]
impl Tool for SystemMonitorTool {
    fn name(&self) -> &str {
        "system_monitor"
    }

    fn description(&self) -> &str {
        "Monitor system resources (CPU, RAM, Uptime), list top processes, or check network interface statistics."
    }

    fn usage(&self) -> &str {
        "Arguments: 'resources', 'processes [limit]', or 'network'"
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let args = args.trim().to_lowercase();
        let output = if args.is_empty() || args.contains("resources") {
            self.system_resources().await
        } else if args.starts_with("processes") {
            let limit = args.split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(10);
            self.process_list(limit)
        } else if args.contains("network") {
            self.network_stats()
        } else {
            format!(
                "Unknown system_monitor command: '{}'.\nUsage: {}",
                args,
                self.usage()
            )
        };
        Ok(ToolOutput::Immediate(serde_json::Value::String(output)))
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}
