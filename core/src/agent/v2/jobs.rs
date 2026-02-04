use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetrics {
    /// Total prompt tokens used
    pub prompt_tokens: u32,
    /// Total completion tokens used
    pub completion_tokens: u32,
    /// Total tokens used
    pub total_tokens: u32,
    /// Number of LLM requests made
    pub request_count: u32,
    /// Current context window size in tokens
    pub context_tokens: usize,
    /// Number of errors encountered
    pub error_count: u32,
    /// Number of rate limit hits (429 errors)
    pub rate_limit_hits: u32,
}

impl Default for JobMetrics {
    fn default() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            request_count: 0,
            context_tokens: 0,
            error_count: 0,
            rate_limit_hits: 0,
        }
    }
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
    pub observed: bool,
    /// Metrics tracking
    pub metrics: JobMetrics,
    /// Last activity timestamp (updated on token usage, requests, etc.)
    pub last_activity: DateTime<Utc>,
    /// Detailed action log for job inspection
    pub action_log: Vec<ActionEntry>,
    /// Is this a worker job ( spawned via delegate)?
    pub is_worker: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionEntry {
    pub timestamp: DateTime<Utc>,
    pub action_type: ActionType,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionType {
    Thought,
    ToolCall,
    ToolResult,
    Error,
    FinalAnswer,
    System,
}

#[derive(Clone, Default)]
pub struct JobRegistry {
    jobs: Arc<RwLock<HashMap<String, BackgroundJob>>>,
    /// Cancellation tokens for running jobs
    cancellation_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            cancellation_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn create_job(&self, tool_name: &str, description: &str) -> String {
        self.create_job_with_options(tool_name, description, false)
    }

    pub fn create_job_with_options(&self, tool_name: &str, description: &str, is_worker: bool) -> String {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let job = BackgroundJob {
            id: id.clone(),
            tool_name: tool_name.to_string(),
            description: description.to_string(),
            started_at: now,
            finished_at: None,
            status: JobStatus::Running,
            output: String::new(),
            result: None,
            error: None,
            observed: false,
            metrics: JobMetrics::default(),
            last_activity: now,
            action_log: Vec::new(),
            is_worker,
        };
        
        // Create cancellation token for this job
        let cancel_token = CancellationToken::new();
        self.cancellation_tokens.write().unwrap().insert(id.clone(), cancel_token);
        
        self.jobs.write().unwrap().insert(id.clone(), job);
        id
    }

    pub fn update_job_output(&self, id: &str, output: &str) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.output.push_str(output);
            job.last_activity = Utc::now();
        }
    }

    pub fn complete_job(&self, id: &str, result: serde_json::Value) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.status = JobStatus::Completed;
            job.finished_at = Some(Utc::now());
            job.result = Some(result);
            job.last_activity = Utc::now();
        }
        // Clean up cancellation token
        self.cancellation_tokens.write().unwrap().remove(id);
    }

    pub fn fail_job(&self, id: &str, error: &str) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.status = JobStatus::Failed;
            job.finished_at = Some(Utc::now());
            job.error = Some(error.to_string());
            job.last_activity = Utc::now();
            job.metrics.error_count += 1;
        }
        // Clean up cancellation token
        self.cancellation_tokens.write().unwrap().remove(id);
    }

    pub fn cancel_job(&self, id: &str) -> bool {
        // Signal cancellation
        if let Some(token) = self.cancellation_tokens.read().unwrap().get(id) {
            token.cancel();
        }
        
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            if job.status == JobStatus::Running {
                job.status = JobStatus::Cancelled;
                job.finished_at = Some(Utc::now());
                job.error = Some("Cancelled by user".to_string());
                job.last_activity = Utc::now();
                job.action_log.push(ActionEntry {
                    timestamp: Utc::now(),
                    action_type: ActionType::System,
                    content: "Job cancelled by user".to_string(),
                });
                return true;
            }
        }
        false
    }

    pub fn cancel_all_jobs(&self) -> usize {
        let job_ids: Vec<String> = self.jobs.read().unwrap()
            .values()
            .filter(|j| j.status == JobStatus::Running)
            .map(|j| j.id.clone())
            .collect();
        
        let mut cancelled_count = 0;
        for id in job_ids {
            if self.cancel_job(&id) {
                cancelled_count += 1;
            }
        }
        cancelled_count
    }

    pub fn get_job(&self, id: &str) -> Option<BackgroundJob> {
        self.jobs.read().unwrap().get(id).cloned()
    }

    pub fn get_cancellation_token(&self, id: &str) -> Option<CancellationToken> {
        self.cancellation_tokens.read().unwrap().get(id).cloned()
    }

    pub fn is_cancelled(&self, id: &str) -> bool {
        self.cancellation_tokens.read().unwrap()
            .get(id)
            .map(|t| t.is_cancelled())
            .unwrap_or(true) // If no token, treat as cancelled
    }

    pub fn list_active_jobs(&self) -> Vec<BackgroundJob> {
        self.jobs.read().unwrap().values()
            .filter(|j| j.status == JobStatus::Running)
            .cloned()
            .collect()
    }

    pub fn list_all_jobs(&self) -> Vec<BackgroundJob> {
        let mut jobs: Vec<BackgroundJob> = self.jobs.read().unwrap().values().cloned().collect();
        // Sort by creation time (newest first)
        jobs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        jobs
    }

    pub fn poll_updates(&self) -> Vec<BackgroundJob> {
        let mut jobs = self.jobs.write().unwrap();
        let mut updates = Vec::new();

        for (_, job) in jobs.iter_mut() {
            if !job.observed || job.status != JobStatus::Running {
                job.observed = true;
                updates.push(job.clone());
            }
        }

        updates
    }

    /// Update job metrics after an LLM request
    pub fn update_metrics(&self, id: &str, prompt_tokens: u32, completion_tokens: u32, context_tokens: usize) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.metrics.prompt_tokens += prompt_tokens;
            job.metrics.completion_tokens += completion_tokens;
            job.metrics.total_tokens = job.metrics.prompt_tokens + job.metrics.completion_tokens;
            job.metrics.request_count += 1;
            job.metrics.context_tokens = context_tokens;
            job.last_activity = Utc::now();
        }
    }

    /// Record a rate limit hit
    pub fn record_rate_limit_hit(&self, id: &str) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.metrics.rate_limit_hits += 1;
            job.last_activity = Utc::now();
        }
    }

    /// Record an error
    pub fn record_error(&self, id: &str) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.metrics.error_count += 1;
            job.last_activity = Utc::now();
        }
    }

    /// Add an action entry to the job log
    pub fn add_action(&self, id: &str, action_type: ActionType, content: &str) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.action_log.push(ActionEntry {
                timestamp: Utc::now(),
                action_type,
                content: content.to_string(),
            });
            job.last_activity = Utc::now();
            // Keep only last 100 entries to prevent memory bloat
            if job.action_log.len() > 100 {
                job.action_log.remove(0);
            }
        }
    }

    /// Detect stuck jobs (no activity + no token progress)
    /// Returns list of stuck job IDs and their details
    pub fn detect_stuck_jobs(&self, timeout_seconds: i64) -> Vec<(String, String, DateTime<Utc>)> {
        let now = Utc::now();
        let jobs = self.jobs.read().unwrap();
        
        jobs.values()
            .filter(|j| j.status == JobStatus::Running)
            .filter(|j| {
                let inactive_duration = now.signed_duration_since(j.last_activity).num_seconds();
                inactive_duration >= timeout_seconds && j.metrics.request_count == 0
            })
            .map(|j| (j.id.clone(), j.description.clone(), j.last_activity))
            .collect()
    }

    /// Mark a job as notified about being stuck (prevents duplicate notifications)
    pub fn mark_stuck_notified(&self, id: &str) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(id) {
            job.action_log.push(ActionEntry {
                timestamp: Utc::now(),
                action_type: ActionType::System,
                content: "Stuck job detected - notified orchestrator".to_string(),
            });
        }
    }
}
