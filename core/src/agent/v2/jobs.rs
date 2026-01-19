use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundJob {
    pub id: String,
    pub tool_name: String,
    pub description: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: JobStatus,
    pub output: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Clone, Default)]
pub struct JobRegistry {
    jobs: Arc<RwLock<HashMap<String, BackgroundJob>>>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn create_job(&self, tool_name: &str, description: &str) -> String {
        let id = Uuid::new_v4().to_string();
        let job = BackgroundJob {
            id: id.clone(),
            tool_name: tool_name.to_string(),
            description: description.to_string(),
            started_at: Utc::now(),
            finished_at: None,
            status: JobStatus::Running,
            output: String::new(),
            result: None,
            error: None,
        };
        self.jobs.write().unwrap().insert(id.clone(), job);
        id
    }

    pub fn update_job_output(&self, id: &str, output: &str) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.output.push_str(output);
        }
    }

    pub fn complete_job(&self, id: &str, result: serde_json::Value) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.status = JobStatus::Completed;
            job.finished_at = Some(Utc::now());
            job.result = Some(result);
        }
    }

    pub fn fail_job(&self, id: &str, error: &str) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.status = JobStatus::Failed;
            job.finished_at = Some(Utc::now());
            job.error = Some(error.to_string());
        }
    }

    pub fn get_job(&self, id: &str) -> Option<BackgroundJob> {
        self.jobs.read().unwrap().get(id).cloned()
    }

    pub fn list_active_jobs(&self) -> Vec<BackgroundJob> {
        self.jobs.read().unwrap().values()
            .filter(|j| j.status == JobStatus::Running)
            .cloned()
            .collect()
    }

    pub fn poll_updates(&self) -> Vec<BackgroundJob> {
        // In a real implementation, we might track which jobs had updates since last poll
        // For now, return all active jobs
        self.list_active_jobs()
    }
}
