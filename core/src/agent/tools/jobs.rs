use crate::agent::tool::{Tool, ToolKind, ToolOutput};
use crate::agent::v2::jobs::{JobRegistry, JobStatus};
use async_trait::async_trait;
use serde::Deserialize;
use std::error::Error as StdError;

#[derive(Deserialize)]
struct ListJobsArgs {
    all: Option<bool>,
}

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
        "List status of background jobs spawned via delegate. Use this to check worker progress, not shell commands."
    }

    fn usage(&self) -> &str {
        "Optional arguments: {\"all\": boolean} - if true (default), lists all jobs. If false, lists only running jobs."
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let args: ListJobsArgs = serde_json::from_str(args).unwrap_or(ListJobsArgs { all: None });
        let all = args.all.unwrap_or(true);

        let jobs = if all {
            self.job_registry.list_all_jobs()
        } else {
            self.job_registry.list_active_jobs()
        };
        
        let mut summary = String::from("Background Jobs:\n");
        if jobs.is_empty() {
            summary.push_str("No matching jobs.");
        } else {
            for job in jobs {
                let (status_icon, status_text) = match job.status {
                    JobStatus::Running => ("⏳", "Running"),
                    JobStatus::Completed => ("✅", "Completed"),
                    JobStatus::Failed => ("❌", "Failed"),
                    JobStatus::Cancelled => ("🛑", "Cancelled"),
                    JobStatus::TimeoutPending => ("⏳", "TimeoutPending"),
                    JobStatus::Stalled => ("⚠️", "Stalled"),
                };
                
                let duration = if let Some(end) = job.finished_at {
                    format!("{:.1}s", (end - job.started_at).num_milliseconds() as f64 / 1000.0)
                } else {
                    format!("{:.1}s", (chrono::Utc::now() - job.started_at).num_milliseconds() as f64 / 1000.0)
                };

                summary.push_str(&format!(
                    "- [{} {}] {} ({}) - {} [{}]\n",
                    status_icon,
                    status_text,
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
