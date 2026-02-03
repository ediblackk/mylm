use crate::agent::tool::{Tool, ToolKind, ToolOutput};
use crate::agent::v2::jobs::{JobRegistry, JobStatus};
use async_trait::async_trait;
use std::error::Error as StdError;

/// A tool for listing the status of background jobs.
pub struct ListJobsTool {
    job_registry: JobRegistry,
}

impl ListJobsTool {
    pub fn new(job_registry: JobRegistry) -> Self {
        Self { job_registry }
    }
}

#[async_trait]
impl Tool for ListJobsTool {
    fn name(&self) -> &str {
        "list_jobs"
    }

    fn description(&self) -> &str {
        "List all background jobs and their current status (Running, Completed, Failed)."
    }

    fn usage(&self) -> &str {
        "No arguments required. Returns a JSON list of jobs."
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }

    async fn call(&self, _args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let jobs = self.job_registry.list_all_jobs();
        
        let mut summary = String::from("Background Jobs:\n");
        if jobs.is_empty() {
            summary.push_str("No active or recent jobs.");
        } else {
            for job in jobs {
                let status_icon = match job.status {
                    JobStatus::Running => "⏳",
                    JobStatus::Completed => "✅",
                    JobStatus::Failed => "❌",
                };
                
                let duration = if let Some(end) = job.finished_at {
                    format!("{:.1}s", (end - job.started_at).num_milliseconds() as f64 / 1000.0)
                } else {
                    format!("{:.1}s", (chrono::Utc::now() - job.started_at).num_milliseconds() as f64 / 1000.0)
                };

                summary.push_str(&format!(
                    "- [{}] {} ({}) - {} [{}]\n",
                    status_icon,
                    job.id,
                    job.tool_name,
                    job.description,
                    duration
                ));
            }
        }

        Ok(ToolOutput::Immediate(serde_json::Value::String(summary)))
    }
}
