//! Worker Stall - Handle stalled worker jobs
//!
//! When workers hit limits (max actions, context threshold, failures),
//! they transition to Stalled state. Main agent must resolve these stalls.
//!
//! # Resolution Strategies
//!
//! 1. **Condense**: Summarize context and continue
//! 2. **Auto**: Switch to Auto mode (no approval needed)
//! 3. **Archive**: Complete job, offload to session memory
//! 4. **Leave**: Keep stalled (requires manual intervention)

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::agent::runtime::orchestrator::commonbox::{Commonbox, CommonboxEvent, JobId, JobResult};
use crate::agent::identity::AgentId;

/// Strategy for resolving a stalled worker.
#[derive(Debug, Clone, PartialEq)]
pub enum StallResolution {
    /// Condense context and continue
    Condense { summary: String },
    /// Switch to Auto mode (bypass approval)
    Auto,
    /// Archive job (complete with summary)
    Archive,
    /// Leave stalled (manual intervention required)
    Leave,
}

/// A stalled job awaiting resolution.
#[derive(Debug, Clone)]
pub struct StalledJob {
    /// Job ID of the stalled worker
    pub job_id: JobId,
    /// Agent ID of the stalled worker
    pub agent_id: AgentId,
    /// Reason for stalling
    pub reason: String,
    /// Timestamp when stalled
    pub stalled_at: chrono::DateTime<chrono::Utc>,
}

/// Handler for stalled worker jobs.
///
/// Main agent subscribes to this to receive stalled jobs and
/// decide resolution strategies.
pub struct WorkerStall {
    /// Reference to commonbox for event subscription
    commonbox: Arc<Commonbox>,
    /// Queue of stalled jobs awaiting resolution
    stalled_queue: Arc<RwLock<VecDeque<StalledJob>>>,
    /// Event receiver (kept alive)
    _receiver: broadcast::Receiver<CommonboxEvent>,
    /// Background task handle
    _task: tokio::task::JoinHandle<()>,
}

impl WorkerStall {
    /// Create a new worker stall handler and start background event listener.
    pub fn new(commonbox: Arc<Commonbox>) -> Self {
        let receiver = commonbox.subscribe();
        let stalled_queue = Arc::new(RwLock::new(VecDeque::new()));
        
        let task = tokio::spawn(Self::event_loop(
            receiver.resubscribe(),
            stalled_queue.clone(),
        ));
        
        Self {
            commonbox,
            stalled_queue,
            _receiver: receiver,
            _task: task,
        }
    }
    
    /// Background event loop that listens for JobStalled events.
    async fn event_loop(
        mut receiver: broadcast::Receiver<CommonboxEvent>,
        queue: Arc<RwLock<VecDeque<StalledJob>>>,
    ) {
        loop {
            match receiver.recv().await {
                Ok(CommonboxEvent::JobStalled { job_id, reason }) => {
                    // Fetch job details to get agent_id
                    // For now, we can't easily get agent_id from the event
                    // The stalled job will be tracked by job_id
                    crate::info_log!("[STALL_SCHEDULER] Job {} stalled: {}", job_id, reason);
                    
                    // TODO: Get agent_id from commonbox lookup
                    // For now, placeholder
                    let stalled_job = StalledJob {
                        job_id,
                        agent_id: crate::agent::identity::AgentId::main(), // placeholder
                        reason,
                        stalled_at: chrono::Utc::now(),
                    };
                    
                    let mut q = queue.write().await;
                    q.push_back(stalled_job);
                }
                Ok(_) => {} // Other events ignored
                Err(_) => {
                    crate::debug_log!("[STALL_SCHEDULER] Event channel closed, exiting");
                    break;
                }
            }
        }
    }
    
    /// Get the next stalled job awaiting resolution.
    pub async fn next_stalled(&self) -> Option<StalledJob> {
        let mut queue = self.stalled_queue.write().await;
        queue.pop_front()
    }
    
    /// Get all stalled jobs without removing them.
    pub async fn list_stalled(&self) -> Vec<StalledJob> {
        let queue = self.stalled_queue.read().await;
        queue.iter().cloned().collect()
    }
    
    /// Apply a resolution strategy to a stalled job.
    pub async fn resolve(
        &self,
        job_id: &JobId,
        resolution: StallResolution,
    ) -> Result<(), StallError> {
        match resolution {
            StallResolution::Condense { summary } => {
                crate::info_log!("[STALL_SCHEDULER] Condensing job {}: {}", job_id, summary);
                // TODO: Implement context condensation
                // For now, wake the worker
                self.commonbox.wake_worker(job_id).await
                    .map_err(|e| StallError::Commonbox(e.to_string()))?;
            }
            StallResolution::Auto => {
                crate::info_log!("[STALL_SCHEDULER] Switching job {} to Auto mode", job_id);
                // TODO: Implement Auto mode switch
                // For now, wake the worker
                self.commonbox.wake_worker(job_id).await
                    .map_err(|e| StallError::Commonbox(e.to_string()))?;
            }
            StallResolution::Archive => {
                crate::info_log!("[STALL_SCHEDULER] Archiving job {}", job_id);
                // Archive the stalled job (Stalled → Completed)
                self.commonbox.archive_job(
                    job_id,
                    JobResult::new("Archived due to stall").unwrap(),
                ).await
                    .map_err(|e| StallError::Commonbox(e.to_string()))?;
            }
            StallResolution::Leave => {
                crate::info_log!("[STALL_SCHEDULER] Leaving job {} stalled", job_id);
                // Do nothing - leave in stalled state
            }
        }
        
        // Remove from queue if present
        let mut queue = self.stalled_queue.write().await;
        queue.retain(|job| job.job_id != *job_id);
        
        Ok(())
    }
    
    /// Archive all currently stalled jobs (simplest resolution).
    pub async fn archive_all(&self) -> usize {
        let stalled = self.list_stalled().await;
        let count = stalled.len();
        
        for job in stalled {
            if let Err(e) = self.resolve(&job.job_id, StallResolution::Archive).await {
                crate::error_log!("[STALL_SCHEDULER] Failed to archive {}: {}", job.job_id, e);
            }
        }
        
        count
    }
}

/// Errors that can occur during stall resolution.
#[derive(Debug, thiserror::Error)]
pub enum StallError {
    #[error("Commonbox error: {0}")]
    Commonbox(String),
    #[error("Job not found")]
    JobNotFound,
    #[error("Invalid resolution for job state")]
    InvalidResolution,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::identity::AgentId;
    
    // Note: WorkerStall was previously named StallScheduler
    
    #[tokio::test]
    async fn test_stall_scheduler_creation() {
        let commonbox = Arc::new(Commonbox::new());
        let scheduler = WorkerStall::new(commonbox);
        
        // Initially empty
        let stalled = scheduler.list_stalled().await;
        assert!(stalled.is_empty());
    }
    
    #[tokio::test]
    async fn test_stall_detection() {
        let commonbox = Arc::new(Commonbox::new());
        let scheduler = WorkerStall::new(commonbox.clone());
        
        // Create and stall a job
        let agent = AgentId::worker("test");
        commonbox.register_agent(agent.clone()).await;
        let job_id = commonbox.create_job(agent, "Test job").await.unwrap();
        
        commonbox.start_job(&job_id).await.unwrap();
        commonbox.stall_job(&job_id, "Test stall").await.unwrap();
        
        // Give event loop time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        
        // Should be in stalled queue
        let stalled = scheduler.list_stalled().await;
        assert_eq!(stalled.len(), 1);
        assert_eq!(stalled[0].job_id, job_id);
    }
    
    #[tokio::test]
    async fn test_archive_resolution() {
        let commonbox = Arc::new(Commonbox::new());
        let scheduler = WorkerStall::new(commonbox.clone());
        
        // Create and stall a job
        let agent = AgentId::worker("test");
        commonbox.register_agent(agent.clone()).await;
        let job_id = commonbox.create_job(agent, "Test job").await.unwrap();
        
        commonbox.start_job(&job_id).await.unwrap();
        commonbox.stall_job(&job_id, "Test stall").await.unwrap();
        
        // Give event loop time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        
        // Archive it
        scheduler.resolve(&job_id, StallResolution::Archive).await.unwrap();
        
        // Job should be completed
        let job = commonbox.get_job(&job_id).await.unwrap();
        assert_eq!(job.status, crate::agent::runtime::orchestrator::commonbox::JobStatus::Completed);
    }
}
