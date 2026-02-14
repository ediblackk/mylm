//! Background job management and metrics tracking for agent V2.
//!
//! Manages the lifecycle of background jobs including creation, execution,
//! cancellation, and completion tracking. Collects metrics such as token usage,
//! request counts, and error rates for monitoring job health.
//!
//! # Main Types
//! - `JobRegistry`: Thread-safe registry for managing all jobs
//! - `BackgroundJob`: Represents a single background task with metadata
//! - `JobMetrics`: Tracks token usage and performance metrics
//! - `ActionEntry`: Log entry for job actions and events

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use chrono::{DateTime, Utc, TimeZone};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use crate::agent_old::event_bus::CoreEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    /// Job timed out but waiting for grace period before final cleanup
    TimeoutPending,
    /// Job exceeded action budget without returning final answer - requires main agent decision
    Stalled,
}

/// Type of agent running the job - Main agent or a specific worker
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentType {
    /// Main orchestrator agent
    Main,
    /// Worker agent with a specific name/type
    Worker(String),
}

impl Default for AgentType {
    fn default() -> Self {
        AgentType::Main
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
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
    /// Maximum context tokens allowed (snapshot from config)
    pub max_context_tokens: usize,
    /// Number of errors encountered
    pub error_count: u32,
    /// Number of rate limit hits (429 errors)
    pub rate_limit_hits: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundJob {
    pub id: String,
    pub tool_name: String,
    /// Full description of the job
    pub description: String,
    /// Short title for display (max 15 chars)
    pub short_title: String,
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
    /// Cleanup claim tracking to prevent premature deletion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_claim: Option<CleanupClaim>,
    /// For TimeoutPending jobs: when the grace period expires (15 seconds after timeout)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_expires_at: Option<DateTime<Utc>>,
    /// Per-job status message for real-time updates (e.g., "Processing step 3...")
    pub status_message: Option<String>,
    /// Parent job ID for worker hierarchy tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_job_id: Option<String>,
    /// Model used for this job (e.g., "gpt-4", "claude-3-sonnet")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Agent type running this job (Main or Worker with name)
    pub agent_type: AgentType,
}

impl BackgroundJob {
    /// Get the current action - first checks status_message for real-time status,
    /// then falls back to most recent non-system action log entry
    pub fn current_action(&self) -> Option<String> {
        // First check for real-time status message
        if let Some(ref status) = self.status_message {
            if !status.is_empty() {
                return Some(status.clone());
            }
        }
        
        // Fall back to action log (most recent non-system entry)
        self.action_log.iter()
            .rev()
            .find(|entry| entry.action_type != ActionType::System)
            .map(|entry| match entry.action_type {
                ActionType::Thought => {
                    // Show actual thought content, truncated if needed
                    let thought = entry.content.trim();
                    if thought.is_empty() {
                        "Thinking...".to_string()
                    } else if thought.len() > 60 {
                        format!("{}...", &thought[..57])
                    } else {
                        thought.to_string()
                    }
                },
                ActionType::ToolCall => format!("Using tool: {}", entry.content.chars().take(30).collect::<String>()),
                ActionType::ToolResult => "Processing result".to_string(),
                ActionType::Error => format!("Error: {}", entry.content.chars().take(40).collect::<String>()),
                ActionType::FinalAnswer => "Completing".to_string(),
                ActionType::System => entry.content.clone(),
            })
    }

    /// Get context window usage as (used, max) for progress bar
    pub fn context_window(&self) -> (usize, usize) {
        (self.metrics.context_tokens, self.metrics.max_context_tokens)
    }

    /// Get a short display string for the job
    pub fn display_short(&self) -> String {
        format!("#{} {} {}", 
            &self.id[..8],
            self.status_icon(),
            self.short_title
        )
    }

    fn status_icon(&self) -> &'static str {
        match self.status {
            JobStatus::Running => "▶",
            JobStatus::Completed => "✓",
            JobStatus::Failed => "✗",
            JobStatus::Cancelled => "⊘",
            JobStatus::TimeoutPending => "⏳",
            JobStatus::Stalled => "⚠",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionEntry {
    pub timestamp: DateTime<Utc>,
    pub action_type: ActionType,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionType {
    Thought,
    ToolCall,
    ToolResult,
    Error,
    FinalAnswer,
    System,
}

/// Cleanup claim tracking to prevent premature deletion
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CleanupClaim {
    /// Job is pending cleanup verification
    Pending(DateTime<Utc>),
    /// Job has been verified and is ready for final removal
    Processed(DateTime<Utc>),
}

#[derive(Clone, Default)]
pub struct JobRegistry {
    jobs: Arc<RwLock<HashMap<String, BackgroundJob>>>,
    /// Cancellation tokens for running jobs
    cancellation_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
    /// File metadata cache: job_id -> (modification_time, file_size)
    file_metadata: Arc<RwLock<HashMap<String, (std::time::SystemTime, u64)>>>,
    /// Event bus for publishing job status changes (shared via Arc)
    event_bus: Arc<RwLock<Option<Arc<crate::agent_old::event_bus::EventBus>>>>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            cancellation_tokens: Arc::new(RwLock::new(HashMap::new())),
            file_metadata: Arc::new(RwLock::new(HashMap::new())),
            event_bus: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the event bus for publishing job status changes
    pub fn set_event_bus(&self, event_bus: Arc<crate::agent_old::event_bus::EventBus>) {
        *self.event_bus.write() = Some(event_bus);
    }

    /// Get the event bus if available
    pub fn get_event_bus(&self) -> Option<Arc<crate::agent_old::event_bus::EventBus>> {
        self.event_bus.read().clone()
    }

    /// Publish a job status change event if event bus is available
    fn publish_event(&self, event: CoreEvent) {
        if let Some(bus) = &*self.event_bus.read() {
            let _ = bus.publish(event);
        }
    }

    /// Remove finished jobs older than the specified duration.
    /// Uses a two-phase cleanup to prevent race conditions:
    /// 1. First call: claim jobs for cleanup (set cleanup_claim = Pending)
    /// 2. Subsequent calls (after grace period): remove claimed jobs (cleanup_claim = Processed)
    /// Returns the number of jobs removed.
    pub fn cleanup_finished_jobs(&mut self, older_than: Duration) -> usize {
        let cutoff = Utc::now().checked_sub_signed(chrono::Duration::from_std(older_than).unwrap_or_default())
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap());
        
        let mut removed = 0;
        let mut jobs = self.jobs.write();
        let mut tokens = self.cancellation_tokens.write();
        let mut file_metadata = self.file_metadata.write();
        
        // Phase 1: Claim finished jobs that haven't been claimed yet
        for (_id, job) in jobs.iter_mut() {
            if matches!(job.status, JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled) {
                if let Some(finished) = job.finished_at {
                    if finished < cutoff && job.cleanup_claim.is_none() {
                        // Claim this job for cleanup
                        job.cleanup_claim = Some(CleanupClaim::Pending(Utc::now()));
                    }
                }
            }
        }
        
        // Phase 2: Remove jobs that have been claimed and are ready (Processed)
        // We'll collect IDs to remove to avoid borrowing issues
        let ids_to_remove: Vec<String> = jobs.iter()
            .filter(|(_, job)| {
                matches!(job.cleanup_claim, Some(CleanupClaim::Processed(_)))
            })
            .map(|(id, _)| id.clone())
            .collect();
        
        for id in ids_to_remove {
            if jobs.remove(&id).is_some() {
                tokens.remove(&id);
                file_metadata.remove(&id);
                removed += 1;
            }
        }
        
        removed
    }

    /// Advance cleanup claims from Pending to Processed after grace period.
    /// This should be called periodically to promote claims for final removal.
    /// Returns the number of jobs promoted to Processed state.
    pub fn advance_cleanup_claims(&self, grace_period: Duration) -> usize {
        let mut jobs = self.jobs.write();
        self._advance_cleanup_claims_locked(&mut jobs, grace_period)
    }
    
    /// Internal helper that operates on already-locked jobs map.
    /// This avoids deadlock when called from within a lock.
    fn _advance_cleanup_claims_locked(&self, jobs: &mut HashMap<String, BackgroundJob>, grace_period: Duration) -> usize {
        let now = Utc::now();
        let mut promoted = 0;
        
        for (_, job) in jobs.iter_mut() {
            if let Some(CleanupClaim::Pending(claimed_at)) = job.cleanup_claim {
                if now.signed_duration_since(claimed_at) >= chrono::Duration::from_std(grace_period).unwrap_or_default() {
                    job.cleanup_claim = Some(CleanupClaim::Processed(now));
                    promoted += 1;
                }
            }
        }
        
        promoted
    }

    pub fn create_job(&self, tool_name: &str, description: &str) -> String {
        self.create_job_with_options(tool_name, description, false, None, None, AgentType::Main)
    }

    pub fn create_job_with_options(
        &self,
        tool_name: &str,
        description: &str,
        is_worker: bool,
        parent_job_id: Option<String>,
        model: Option<String>,
        agent_type: AgentType,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        
        // Generate short title from description (max 15 chars)
        let short_title = if description.len() > 15 {
            format!("{}...", &description[..12])
        } else {
            description.to_string()
        };
        
        // Determine effective is_worker from agent_type
        let effective_is_worker = is_worker || matches!(agent_type, AgentType::Worker(_));
        
        let job = BackgroundJob {
            id: id.clone(),
            tool_name: tool_name.to_string(),
            description: description.to_string(),
            short_title,
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
            is_worker: effective_is_worker,
            cleanup_claim: None,
            timeout_expires_at: None,
            status_message: None,
            parent_job_id: parent_job_id.clone(),
            model: model.clone(),
            agent_type: agent_type.clone(),
        };
        
        // Create cancellation token for this job
        let cancel_token = CancellationToken::new();
        self.cancellation_tokens.write().insert(id.clone(), cancel_token);
        
        self.jobs.write().insert(id.clone(), job.clone());
        
        // Log with FULL UUID and structured fields
        let agent_type_str = match &agent_type {
            AgentType::Main => "main",
            AgentType::Worker(name) => name.as_str(),
        };
        crate::info_log!(
            "Job created: id={}, tool={}, description={}, is_worker={}, parent_job_id={}, model={}, agent_type={}",
            id,
            tool_name,
            description,
            effective_is_worker,
            parent_job_id.as_deref().unwrap_or("none"),
            model.as_deref().unwrap_or("default"),
            agent_type_str
        );
        id
    }

    pub fn update_job_output(&self, id: &str, output: &str) {
        if let Some(job) = self.jobs.write().get_mut(id) {
            job.output.push_str(output);
            job.last_activity = Utc::now();
        }
    }

    pub fn complete_job(&self, id: &str, result: serde_json::Value) {
        let job_info = if let Some(job) = self.jobs.write().get_mut(id) {
            let started_at = job.started_at;
            let finished_at = Some(Utc::now());
            let description = job.description.clone();
            let tool = job.tool_name.clone();
            let metrics = job.metrics.clone();
            let parent_job_id = job.parent_job_id.clone();
            let model = job.model.clone();
            let agent_type = job.agent_type.clone();
            
            job.status = JobStatus::Completed;
            job.finished_at = finished_at;
            job.result = Some(result.clone());
            job.last_activity = Utc::now();
            
            Some((description, tool, started_at, finished_at.unwrap(), metrics, parent_job_id, model, agent_type))
        } else {
            None
        };

        if let Some((description, tool, started_at, finished_at, metrics, parent_job_id, model, agent_type)) = job_info {
            let duration = finished_at.signed_duration_since(started_at);
            let duration_secs = duration.num_seconds();
            
            let agent_type_str = match &agent_type {
                AgentType::Main => "main",
                AgentType::Worker(name) => name.as_str(),
            };
            
            crate::info_log!(
                "Job completed: id={}, tool={}, description={}, duration_secs={}, tokens={}/{}/{}, requests={}, errors={}, parent_job_id={}, model={}, agent_type={}",
                id,
                tool,
                description,
                duration_secs,
                metrics.prompt_tokens,
                metrics.completion_tokens,
                metrics.total_tokens,
                metrics.request_count,
                metrics.error_count,
                parent_job_id.as_deref().unwrap_or("none"),
                model.as_deref().unwrap_or("default"),
                agent_type_str
            );
        } else {
            crate::warn_log!("Job complete called for unknown job: {}", id);
        }

        // Clean up cancellation token
        self.cancellation_tokens.write().remove(id);

        // Publish WorkerCompleted event for TUI to update jobs panel
        self.publish_event(CoreEvent::WorkerCompleted {
            job_id: id.to_string(),
            result: result.to_string(),
        });
    }

    pub fn fail_job(&self, id: &str, error: &str) {
        let job_info = if let Some(job) = self.jobs.write().get_mut(id) {
            let started_at = job.started_at;
            let finished_at = Some(Utc::now());
            let description = job.description.clone();
            let tool = job.tool_name.clone();
            let metrics = job.metrics.clone();
            let parent_job_id = job.parent_job_id.clone();
            let model = job.model.clone();
            let agent_type = job.agent_type.clone();
            
            job.status = JobStatus::Failed;
            job.finished_at = finished_at;
            job.error = Some(error.to_string());
            job.last_activity = Utc::now();
            job.metrics.error_count += 1;
            
            Some((description, tool, started_at, finished_at.unwrap(), metrics, parent_job_id, model, agent_type))
        } else {
            None
        };

        if let Some((description, tool, started_at, finished_at, metrics, parent_job_id, model, agent_type)) = job_info {
            let duration = finished_at.signed_duration_since(started_at);
            let duration_secs = duration.num_seconds();
            
            let agent_type_str = match &agent_type {
                AgentType::Main => "main",
                AgentType::Worker(name) => name.as_str(),
            };
            
            crate::error_log!(
                "Job failed: id={}, tool={}, description={}, error={}, duration_secs={}, tokens={}/{}/{}, requests={}, errors={}, parent_job_id={}, model={}, agent_type={}",
                id,
                tool,
                description,
                error,
                duration_secs,
                metrics.prompt_tokens,
                metrics.completion_tokens,
                metrics.total_tokens,
                metrics.request_count,
                metrics.error_count,
                parent_job_id.as_deref().unwrap_or("none"),
                model.as_deref().unwrap_or("default"),
                agent_type_str
            );
        } else {
            crate::warn_log!("Job fail called for unknown job: {}", id);
        }

        // Clean up cancellation token
        self.cancellation_tokens.write().remove(id);

        // Publish WorkerCompleted event for TUI to update jobs panel (with error as result)
        self.publish_event(CoreEvent::WorkerCompleted {
            job_id: id.to_string(),
            result: error.to_string(),
        });
    }

    pub fn cancel_job(&self, id: &str) -> bool {
        // Signal cancellation
        if let Some(token) = self.cancellation_tokens.read().get(id) {
            token.cancel();
        }
        
        let cancel_info = if let Some(job) = self.jobs.write().get_mut(id) {
            if job.status == JobStatus::Running {
                let started_at = job.started_at;
                let finished_at = Some(Utc::now());
                let description = job.description.clone();
                let tool = job.tool_name.clone();
                let metrics = job.metrics.clone();
                let parent_job_id = job.parent_job_id.clone();
                let model = job.model.clone();
                let agent_type = job.agent_type.clone();
                
                job.status = JobStatus::Cancelled;
                job.finished_at = finished_at;
                job.error = Some("Cancelled by user".to_string());
                job.last_activity = Utc::now();
                job.action_log.push(ActionEntry {
                    timestamp: Utc::now(),
                    action_type: ActionType::System,
                    content: "Job cancelled by user".to_string(),
                });
                
                Some((description, tool, started_at, finished_at.unwrap(), metrics, parent_job_id, model, agent_type))
            } else {
                None
            }
        } else {
            None
        };
        
        if let Some((description, tool, started_at, finished_at, metrics, parent_job_id, model, agent_type)) = cancel_info {
            let duration = finished_at.signed_duration_since(started_at);
            let duration_secs = duration.num_seconds();
            
            let agent_type_str = match &agent_type {
                AgentType::Main => "main",
                AgentType::Worker(name) => name.as_str(),
            };
            
            crate::info_log!(
                "Job cancelled: id={}, tool={}, description={}, duration_secs={}, tokens={}/{}/{}, requests={}, errors={}, parent_job_id={}, model={}, agent_type={}",
                id,
                tool,
                description,
                duration_secs,
                metrics.prompt_tokens,
                metrics.completion_tokens,
                metrics.total_tokens,
                metrics.request_count,
                metrics.error_count,
                parent_job_id.as_deref().unwrap_or("none"),
                model.as_deref().unwrap_or("default"),
                agent_type_str
            );

            // Publish WorkerCompleted event for TUI to update jobs panel
            self.publish_event(CoreEvent::WorkerCompleted {
                job_id: id.to_string(),
                result: "Job cancelled by user".to_string(),
            });

            return true;
        }
        false
    }

    pub fn cancel_all_jobs(&self) -> usize {
        let job_ids: Vec<String> = self.jobs.read()
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
        self.jobs.read().get(id).cloned()
    }

    pub fn get_cancellation_token(&self, id: &str) -> Option<CancellationToken> {
        self.cancellation_tokens.read().get(id).cloned()
    }

    pub fn is_cancelled(&self, id: &str) -> bool {
        self.cancellation_tokens.read()
            .get(id)
            .map(|t| t.is_cancelled())
            .unwrap_or(true) // If no token, treat as cancelled
    }

    pub fn list_active_jobs(&self) -> Vec<BackgroundJob> {
        self.jobs.read().values()
            .filter(|j| j.status == JobStatus::Running)
            .cloned()
            .collect()
    }

    pub fn list_all_jobs(&self) -> Vec<BackgroundJob> {
        let mut jobs: Vec<BackgroundJob> = self.jobs.read().values().cloned().collect();
        // Sort by creation time (newest first)
        jobs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        jobs
    }

    pub fn poll_updates(&self) -> Vec<BackgroundJob> {
        let mut jobs = self.jobs.write();
        let mut updates = Vec::new();
    
        // Advance cleanup claims: Pending -> Processed after grace period (15 seconds)
        // This is called frequently (heartbeat interval ~5s) so claims get promoted
        self._advance_cleanup_claims_locked(&mut jobs, std::time::Duration::from_secs(15));
    
        // Check for TimeoutPending jobs that have exceeded their grace period
        let now = Utc::now();
        for (_, job) in jobs.iter_mut() {
            if let JobStatus::TimeoutPending = job.status {
                if let Some(expires_at) = job.timeout_expires_at {
                    if now >= expires_at {
                        // Grace period expired - mark as failed
                        job.status = JobStatus::Failed;
                        job.error = Some(job.error.take().unwrap_or_else(|| "Timeout grace period expired".to_string()));
                        job.last_activity = now;
                    }
                }
            }
            
            // Only return jobs that haven't been observed yet.
            // Jobs transition from Running -> terminal status, and we want to return them
            // exactly ONCE when they first become terminal.
            if !job.observed {
                job.observed = true;
                updates.push(job.clone());
            }
        }
    
        // Log which job IDs are being returned with their status and terminal state
        if !updates.is_empty() {
            let job_details: Vec<String> = updates
                .iter()
                .map(|j| {
                    let is_terminal = matches!(j.status, JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled | JobStatus::Stalled);
                    format!("{}:{:?}:{}", j.id, j.status, if is_terminal { "terminal" } else { "active" })
                })
                .collect();
            crate::debug_log!(
                "poll_updates returning {} jobs: [{}]",
                updates.len(),
                job_details.join(", ")
            );
        }
    
        updates
    }

    /// Update job metrics after an LLM request
    pub fn update_metrics(&self, id: &str, prompt_tokens: u32, completion_tokens: u32, context_tokens: usize, max_context_tokens: usize) {
        if let Some(job) = self.jobs.write().get_mut(id) {
            let old_prompt = job.metrics.prompt_tokens;
            let old_completion = job.metrics.completion_tokens;
            let old_total = job.metrics.total_tokens;
            
            job.metrics.prompt_tokens += prompt_tokens;
            job.metrics.completion_tokens += completion_tokens;
            job.metrics.total_tokens = job.metrics.prompt_tokens + job.metrics.completion_tokens;
            job.metrics.request_count += 1;
            job.metrics.context_tokens = context_tokens;
            job.metrics.max_context_tokens = max_context_tokens;
            job.last_activity = Utc::now();
            
            // Debug log to trace metrics accumulation
            crate::info_log!(
                "[METRICS] Job {}: added prompt={}, completion={}. Old: {}/{}/{}, New: {}/{}/{}, Request #{}",
                &id[..8.min(id.len())],
                prompt_tokens, completion_tokens,
                old_prompt, old_completion, old_total,
                job.metrics.prompt_tokens, job.metrics.completion_tokens, job.metrics.total_tokens,
                job.metrics.request_count
            );
        }
    }

    /// Record a rate limit hit
    pub fn record_rate_limit_hit(&self, id: &str) {
        if let Some(job) = self.jobs.write().get_mut(id) {
            job.metrics.rate_limit_hits += 1;
            job.last_activity = Utc::now();
        }
    }

    /// Record an error
    pub fn record_error(&self, id: &str) {
        if let Some(job) = self.jobs.write().get_mut(id) {
            job.metrics.error_count += 1;
            job.last_activity = Utc::now();
        }
    }

    /// Add an action entry to the job log
    pub fn add_action(&self, id: &str, action_type: ActionType, content: &str) {
        if let Some(job) = self.jobs.write().get_mut(id) {
            job.action_log.push(ActionEntry {
                timestamp: Utc::now(),
                action_type: action_type.clone(),
                content: content.to_string(),
            });
            job.last_activity = Utc::now();
            // Keep only last 100 entries to prevent memory bloat
            if job.action_log.len() > 100 {
                job.action_log.remove(0);
            }
        }
        // Publish event to trigger UI refresh for real-time job journey updates
        self.publish_event(CoreEvent::StatusUpdate {
            message: format!("Job {}: {:?} - {}", &id[..8.min(id.len())], action_type, content),
        });
    }

    /// Update the status message for a job
    /// Also logs the previous status to action_log for history tracking
    pub fn update_status_message(&self, id: &str, message: &str) {
        if let Some(job) = self.jobs.write().get_mut(id) {
            // Archive previous status to action_log if exists
            if let Some(prev_status) = job.status_message.take() {
                if !prev_status.is_empty() {
                    job.action_log.push(ActionEntry {
                        timestamp: Utc::now(),
                        action_type: ActionType::System,
                        content: prev_status,
                    });
                }
            }
            job.status_message = Some(message.to_string());
            job.last_activity = Utc::now();
        }
    }

    /// Clear the status message for a job
    pub fn clear_status_message(&self, id: &str) {
        if let Some(job) = self.jobs.write().get_mut(id) {
            job.status_message = None;
        }
    }

    /// Detect stuck jobs (no activity + no token progress)
    /// Returns list of stuck job IDs and their details
    pub fn detect_stuck_jobs(&self, timeout_seconds: i64) -> Vec<(String, String, DateTime<Utc>)> {
        let now = Utc::now();
        let jobs = self.jobs.read();
        
        jobs.values()
            .filter(|j| j.status == JobStatus::Running)
            .filter(|j| {
                let inactive_duration = now.signed_duration_since(j.last_activity).num_seconds();
                inactive_duration >= timeout_seconds && j.metrics.request_count == 0
            })
            .map(|j| (j.id.clone(), j.description.clone(), j.last_activity))
            .collect()
    }

    /// Mark a job as stalled - exceeded action budget without final answer
    /// This pauses the job and waits for main agent decision
    pub fn stall_job(&self, id: &str, reason: &str, action_count: usize) {
        let stall_info = if let Some(job) = self.jobs.write().get_mut(id) {
            let started_at = job.started_at;
            let now = Utc::now();
            let description = job.description.clone();
            let tool = job.tool_name.clone();
            let metrics = job.metrics.clone();
            let parent_job_id = job.parent_job_id.clone();
            let model = job.model.clone();
            let agent_type = job.agent_type.clone();
            
            job.status = JobStatus::Stalled;
            job.observed = false; // Reset so poll_updates will return this job
            job.last_activity = now;
            job.action_log.push(ActionEntry {
                timestamp: now,
                action_type: ActionType::System,
                content: format!("Job stalled after {} actions: {}", action_count, reason),
            });
            
            Some((description, tool, started_at, now, metrics, parent_job_id, model, agent_type))
        } else {
            None
        };
        
        if let Some((description, tool, started_at, stalled_at, metrics, parent_job_id, model, agent_type)) = stall_info {
            let duration = stalled_at.signed_duration_since(started_at);
            let duration_secs = duration.num_seconds();
            
            let agent_type_str = match &agent_type {
                AgentType::Main => "main",
                AgentType::Worker(name) => name.as_str(),
            };
            
            crate::info_log!(
                "Job stalled: id={}, tool={}, description={}, reason={}, action_count={}, duration_secs={}, tokens={}/{}/{}, requests={}, errors={}, parent_job_id={}, model={}, agent_type={}",
                id,
                tool,
                description,
                reason,
                action_count,
                duration_secs,
                metrics.prompt_tokens,
                metrics.completion_tokens,
                metrics.total_tokens,
                metrics.request_count,
                metrics.error_count,
                parent_job_id.as_deref().unwrap_or("none"),
                model.as_deref().unwrap_or("default"),
                agent_type_str
            );
        } else {
            crate::warn_log!("Job stall called for unknown job: {}", id);
        }
    }

    /// Continue a stalled job - called when main agent grants more budget
    pub fn continue_stalled_job(&self, id: &str) -> bool {
        if let Some(job) = self.jobs.write().get_mut(id) {
            if job.status == JobStatus::Stalled {
                job.status = JobStatus::Running;
                job.last_activity = Utc::now();
                job.action_log.push(ActionEntry {
                    timestamp: Utc::now(),
                    action_type: ActionType::System,
                    content: "Job continued by main agent".to_string(),
                });
                
                crate::info_log!("Job continued: id={}, tool={}, description={}", 
                    &id[..8.min(id.len())], job.tool_name, job.description);
                return true;
            }
        }
        false
    }

    /// Mark a job as notified about being stuck (prevents duplicate notifications)
    pub fn mark_stuck_notified(&self, id: &str) {
        if let Some(job) = self.jobs.write().get_mut(id) {
            job.action_log.push(ActionEntry {
                timestamp: Utc::now(),
                action_type: ActionType::System,
                content: "Stuck job detected - notified orchestrator".to_string(),
            });
        }
    }

    /// Claim a finished job for cleanup. Returns true if claim was successful.
    /// This sets the cleanup_claim to Pending, indicating the job is scheduled for cleanup.
    pub fn claim_job_for_cleanup(&self, id: &str) -> bool {
        if let Some(job) = self.jobs.write().get_mut(id) {
            if matches!(job.status, JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled) {
                if job.cleanup_claim.is_none() {
                    job.cleanup_claim = Some(CleanupClaim::Pending(Utc::now()));
                    return true;
                }
            }
        }
        false
    }

    /// Mark a claimed job as processed and ready for final removal.
    /// Returns true if the job was marked and can now be cleaned up.
    pub fn mark_job_processed(&self, id: &str) -> bool {
        if let Some(job) = self.jobs.write().get_mut(id) {
            if let Some(CleanupClaim::Pending(_)) = job.cleanup_claim {
                job.cleanup_claim = Some(CleanupClaim::Processed(Utc::now()));
                return true;
            }
        }
        false
    }

    /// Set job status to TimeoutPending (grace period before final failure).
    /// Used when a worker times out but we need to wait for file I/O to flush.
    pub fn set_timeout_pending(&self, id: &str, error: &str) -> bool {
        if let Some(job) = self.jobs.write().get_mut(id) {
            if job.status == JobStatus::Running {
                job.status = JobStatus::TimeoutPending;
                job.finished_at = Some(Utc::now());
                job.error = Some(error.to_string());
                job.last_activity = Utc::now();
                job.metrics.error_count += 1;
                // Set grace period expiration (15 seconds from now)
                job.timeout_expires_at = Some(Utc::now() + chrono::Duration::seconds(15));
                return true;
            }
        }
        false
    }

    /// Cache file metadata for a job to avoid repeated filesystem queries.
    /// This should be called when a file is first detected to exist.
    pub fn cache_file_metadata(&self, id: &str, path: &str) {
        if let Ok(metadata) = std::fs::metadata(path) {
            let mtime = metadata.modified().unwrap_or_else(|_| std::time::SystemTime::now());
            let size = metadata.len();
            self.file_metadata.write().insert(id.to_string(), (mtime, size));
        }
    }

    /// Check if the cached file metadata indicates the file is unchanged.
    /// Returns true if file still exists and metadata matches cache.
    pub fn is_file_unchanged(&self, id: &str, path: &str) -> bool {
        let cache = self.file_metadata.read();
        if let Some((cached_mtime, cached_size)) = cache.get(id) {
            if let Ok(metadata) = std::fs::metadata(path) {
                let current_mtime = metadata.modified().unwrap_or_else(|_| std::time::SystemTime::now());
                let current_size = metadata.len();
                return &current_mtime == cached_mtime && &current_size == cached_size;
            }
        }
        false
    }

    /// Clean up file metadata cache for a job (called after job removal).
    pub fn remove_file_metadata(&self, id: &str) {
        self.file_metadata.write().remove(id);
    }
}
