//! Commonbox - Unified Agent State
//!
//! Single source of truth for all agent state in the multi-agent system.
//! Replaces and subsumes JobRegistry functionality.
//!
//! # Architecture
//!
//! Commonbox uses a **single RwLock** to prevent deadlocks. All state mutations
//! go through `with_state()`, which regenerates the LLM snapshot atomically.
//!
//! ```text
//! Commonbox
//! ├── state: RwLock<CommonboxState>
//! │   ├── entries: HashMap<AgentId, CommonboxEntry>
//! │   ├── jobs: HashMap<JobId, Job>
//! │   ├── agent_to_job: HashMap<AgentId, JobId>
//! │   └── llm_snapshot: String (cached, auto-regenerated)
//! └── event_tx: broadcast::Sender<CommonboxEvent>
//! ```
//!
//! # Security Contract
//!
//! - **Self-update only**: Agents can only update their own CommonboxEntry
//! - **Runtime-enforced**: `update_own_entry()` verifies `caller == target`
//! - **LLM cannot spoof**: AgentId is set by Runtime, not from LLM output
//!
//! # Snapshot Regeneration Contract
//!
//! The LLM snapshot is regenerated on **every state mutation** via `with_state()`.
//! This ensures consistency between machine state and LLM-visible state.

pub mod agent;
pub mod coordination;
pub mod error;
pub mod event;
pub mod id;
pub mod job;
pub mod state;

// Re-exports for convenience
pub use agent::{AgentStatus, CommonboxEntry, EntryUpdate};
pub use coordination::{CoordinationBoard, CoordinationEntry};
pub use error::CommonboxError;
pub use event::CommonboxEvent;
pub use id::JobId;
pub use job::{Job, JobResult, JobStatus, RoutedQuery};
pub use state::CommonboxState;

use crate::agent::identity::AgentId;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// Unified agent state container.
///
/// Commonbox is the single source of truth for:
/// - Agent lifecycle status and metrics
/// - Job tracking and dependencies
/// - Semantic snapshot for LLM consumption
///
/// # Thread Safety
///
/// Uses a single RwLock to prevent deadlocks. All mutations go through
/// `with_state()` which regenerates the LLM snapshot atomically.
#[derive(Debug, Clone)]
pub struct Commonbox {
    /// Protected state - single lock for deadlock prevention
    state: Arc<RwLock<CommonboxState>>,
    /// Event broadcast channel
    event_tx: broadcast::Sender<CommonboxEvent>,
}

impl Commonbox {
    /// Create a new empty Commonbox.
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            state: Arc::new(RwLock::new(CommonboxState::new())),
            event_tx,
        }
    }

    /// Helper: Execute mutation with automatic snapshot regeneration.
    ///
    /// All state modifications MUST use this helper.
    /// The snapshot is regenerated after EVERY mutation.
    async fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut CommonboxState) -> R,
    {
        let mut state = self.state.write().await;
        let result = f(&mut *state);
        state.regenerate_snapshot();
        result
    }

    /// Get the current LLM semantic snapshot.
    ///
    /// This is a cached string regenerated on every state change.
    /// Format per agent: "{name} {s:status,h:health,cm:comment}"
    pub async fn get_llm_snapshot(&self) -> String {
        self.state.read().await.llm_snapshot.clone()
    }

    /// Subscribe to Commonbox events.
    pub fn subscribe(&self) -> broadcast::Receiver<CommonboxEvent> {
        self.event_tx.subscribe()
    }

    // =========================================================================
    // Agent Management
    // =========================================================================

    /// Register a new agent in the Commonbox.
    ///
    /// Creates a new CommonboxEntry for the agent.
    pub async fn register_agent(&self, agent_id: AgentId) {
        self.with_state(|state| {
            if !state.entries.contains_key(&agent_id) {
                let entry = CommonboxEntry::new(agent_id.clone());
                state.entries.insert(agent_id.clone(), entry);
            }
        })
        .await;

        let _ = self
            .event_tx
            .send(CommonboxEvent::AgentRegistered { agent_id });
    }

    /// Runtime-enforced self-update.
    ///
    /// Agents can only update their own entry. The `caller` must match
    /// the `agent_id` in the `updates`.
    ///
    /// # Errors
    ///
    /// Returns `PermissionDenied` if caller tries to update another agent.
    pub async fn update_own_entry(
        &self,
        caller: &AgentId,
        updates: EntryUpdate,
    ) -> Result<(), CommonboxError> {
        // Security check: caller must be updating their own entry
        if caller != &updates.agent_id {
            return Err(CommonboxError::PermissionDenied);
        }

        let agent_id = updates.agent_id.clone();

        self.with_state(|state| {
            if let Some(entry) = state.entries.get_mut(&agent_id) {
                entry.apply(updates);
                entry.last_updated = Utc::now();
                Ok(())
            } else {
                Err(CommonboxError::AgentNotFound)
            }
        })
        .await?;

        let _ = self
            .event_tx
            .send(CommonboxEvent::AgentUpdated { agent_id });

        Ok(())
    }

    /// Get an agent's entry.
    pub async fn get_entry(&self, agent_id: &AgentId) -> Option<CommonboxEntry> {
        self.state.read().await.entries.get(agent_id).cloned()
    }

    /// Get all agent entries.
    pub async fn list_entries(&self) -> Vec<CommonboxEntry> {
        self.state
            .read()
            .await
            .entries
            .values()
            .cloned()
            .collect()
    }

    // =========================================================================
    // Job Management
    // =========================================================================

    /// Create a new job.
    ///
    /// The job is created in `Pending` status. Call `start_job()` to begin.
    pub async fn create_job(
        &self,
        agent_id: AgentId,
        description: impl Into<String>,
    ) -> Result<JobId, CommonboxError> {
        let job_id = JobId::new();
        let description = description.into();

        // Ensure agent is registered
        self.register_agent(agent_id.clone()).await;

        self.with_state(|state| {
            let job = Job {
                id: job_id,
                agent_id: agent_id.clone(),
                description: description.clone(),
                status: JobStatus::Pending,
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
                result: None,
                error: None,
                status_message: Some("Pending".to_string()),
                dependencies: Vec::new(),
                dependents: Vec::new(),
                pending_queries: Vec::new(),
            };

            state.jobs.insert(job_id, job);
            state.agent_to_job.insert(agent_id.clone(), job_id);
        })
        .await;

        let _ = self
            .event_tx
            .send(CommonboxEvent::JobCreated { job_id, agent_id });

        Ok(job_id)
    }

    /// Get a job by ID.
    pub async fn get_job(&self, job_id: &JobId) -> Option<Job> {
        self.state.read().await.jobs.get(job_id).cloned()
    }

    /// Get job for an agent.
    pub async fn get_agent_job(&self, agent_id: &AgentId) -> Option<Job> {
        let state = self.state.read().await;
        state
            .agent_to_job
            .get(agent_id)
            .and_then(|job_id| state.jobs.get(job_id).cloned())
    }

    /// Start a job (transition from Pending to Running).
    pub async fn start_job(&self, job_id: &JobId) -> Result<(), CommonboxError> {
        self.with_state(|state| {
            if let Some(job) = state.jobs.get_mut(job_id) {
                if job.status != JobStatus::Pending {
                    return Err(CommonboxError::InvalidTransition {
                        from: job.status,
                        to: JobStatus::Running,
                    });
                }
                job.status = JobStatus::Running;
                job.started_at = Some(Utc::now());
                Ok(())
            } else {
                Err(CommonboxError::JobNotFound)
            }
        })
        .await?;

        let _ = self.event_tx.send(CommonboxEvent::JobStarted { job_id: *job_id });
        Ok(())
    }

    /// Update job status message (for progress reporting).
    ///
    /// This updates the human-readable status message without changing
    /// the job's actual status.
    pub async fn update_job_status_message(&self, job_id: &JobId, message: impl Into<String>) {
        let message = message.into();
        self.with_state(|state| {
            if let Some(job) = state.jobs.get_mut(job_id) {
                job.status_message = Some(message);
            }
        })
        .await;

        // Note: No event for status message updates to avoid spam
    }

    /// Archive a stalled job (transition from Stalled → Completed).
    pub async fn archive_job(&self, job_id: &JobId, result: JobResult) -> Result<(), CommonboxError> {
        let job = self
            .with_state(|state| {
                if let Some(job) = state.jobs.get_mut(job_id) {
                    if job.status != JobStatus::Stalled {
                        return Err(CommonboxError::InvalidTransition {
                            from: job.status,
                            to: JobStatus::Completed,
                        });
                    }

                    let dependents = job.dependents.clone();
                    let agent_id = job.agent_id.clone();

                    job.status = JobStatus::Completed;
                    job.completed_at = Some(Utc::now());
                    job.result = Some(result.clone());

                    // Notify dependents
                    for dependent_id in &dependents {
                        if let Some(dep) = state.jobs.get_mut(dependent_id) {
                            dep.dependencies.retain(|id| id != job_id);
                        }
                    }

                    Ok((agent_id, dependents))
                } else {
                    Err(CommonboxError::JobNotFound)
                }
            })
            .await?;

        let (agent_id, dependents) = job;

        // Send event
        let _ = self
            .event_tx
            .send(CommonboxEvent::JobCompleted { job_id: *job_id, result });

        for dependent_id in dependents {
            let _ = self.event_tx.send(CommonboxEvent::DependencyCompleted {
                job_id: dependent_id,
                dependency_id: *job_id,
            });
        }

        // Update agent status
        let _ = self
            .update_own_entry(
                &agent_id,
                EntryUpdate::for_agent(agent_id.clone()).with_status(AgentStatus::Completed),
            )
            .await;

        Ok(())
    }

    /// Mark a job as completed.
    pub async fn complete_job(
        &self,
        job_id: &JobId,
        result: JobResult,
    ) -> Result<(), CommonboxError> {
        let job = self
            .with_state(|state| {
                if let Some(job) = state.jobs.get_mut(job_id) {
                    if job.status.is_terminal() {
                        return Err(CommonboxError::InvalidTransition {
                            from: job.status,
                            to: JobStatus::Completed,
                        });
                    }

                    // Get dependents to notify
                    let dependents = job.dependents.clone();
                    let agent_id = job.agent_id.clone();

                    job.status = JobStatus::Completed;
                    job.completed_at = Some(Utc::now());
                    job.result = Some(result.clone());

                    // Notify dependents
                    for dependent_id in &dependents {
                        if let Some(dep) = state.jobs.get_mut(dependent_id) {
                            // Remove completed dependency
                            dep.dependencies.retain(|id| id != job_id);
                        }
                    }

                    Ok((agent_id, dependents))
                } else {
                    Err(CommonboxError::JobNotFound)
                }
            })
            .await?;

        let (agent_id, dependents) = job;

        // Send events
        let _ = self
            .event_tx
            .send(CommonboxEvent::JobCompleted { job_id: *job_id, result });

        for dependent_id in dependents {
            let _ = self.event_tx.send(CommonboxEvent::DependencyCompleted {
                job_id: dependent_id,
                dependency_id: *job_id,
            });
        }

        // Update agent status
        let _ = self
            .update_own_entry(
                &agent_id,
                EntryUpdate::for_agent(agent_id.clone()).with_status(AgentStatus::Completed),
            )
            .await;

        Ok(())
    }

    /// Mark a job as idle (completed initial task, waiting for routed queries).
    pub async fn idle_job(&self, job_id: &JobId) -> Result<(), CommonboxError> {
        let agent_id = self
            .with_state(|state| {
                if let Some(job) = state.jobs.get_mut(job_id) {
                    if job.status != JobStatus::Running {
                        return Err(CommonboxError::InvalidTransition {
                            from: job.status,
                            to: JobStatus::Idle,
                        });
                    }

                    let agent_id = job.agent_id.clone();
                    job.status = JobStatus::Idle;

                    Ok(agent_id)
                } else {
                    Err(CommonboxError::JobNotFound)
                }
            })
            .await?;

        let _ = self.event_tx.send(CommonboxEvent::JobIdle { job_id: *job_id });

        // Update agent status
        let _ = self
            .update_own_entry(
                &agent_id,
                EntryUpdate::for_agent(agent_id.clone()).with_status(AgentStatus::Idle),
            )
            .await;

        Ok(())
    }

    /// Get all idle workers (available for query routing).
    pub async fn list_idle_workers(&self) -> Vec<(JobId, Job)> {
        self.with_state(|state| {
            state
                .jobs
                .iter()
                .filter(|(_, job)| job.status == JobStatus::Idle)
                .map(|(id, job)| (*id, job.clone()))
                .collect()
        })
        .await
    }

    /// Wake an idle worker to process a routed query.
    /// Transitions from Idle → Running.
    pub async fn wake_worker(&self, job_id: &JobId) -> Result<(), CommonboxError> {
        let agent_id = self
            .with_state(|state| {
                if let Some(job) = state.jobs.get_mut(job_id) {
                    if job.status != JobStatus::Idle {
                        return Err(CommonboxError::InvalidTransition {
                            from: job.status,
                            to: JobStatus::Running,
                        });
                    }

                    let agent_id = job.agent_id.clone();
                    job.status = JobStatus::Running;

                    Ok(agent_id)
                } else {
                    Err(CommonboxError::JobNotFound)
                }
            })
            .await?;

        let _ = self.event_tx.send(CommonboxEvent::JobStarted { job_id: *job_id });

        // Update agent status
        let _ = self
            .update_own_entry(
                &agent_id,
                EntryUpdate::for_agent(agent_id.clone()).with_status(AgentStatus::Processing),
            )
            .await;

        Ok(())
    }

    /// Route a query to an idle worker.
    /// Returns the query ID if successful.
    pub async fn route_query(
        &self,
        job_id: &JobId,
        query: impl Into<String>,
        context: Option<String>,
    ) -> Result<String, CommonboxError> {
        let query = RoutedQuery::new(query).with_context(context.unwrap_or_default());
        let query_id = query.id.clone();

        self.with_state(|state| {
            if let Some(job) = state.jobs.get_mut(job_id) {
                // Can only route to idle or running workers
                if job.status != JobStatus::Idle && job.status != JobStatus::Running {
                    return Err(CommonboxError::InvalidState {
                        state: format!("{:?}", job.status),
                        operation: "route_query".to_string(),
                    });
                }

                job.pending_queries.push(query);
                Ok(())
            } else {
                Err(CommonboxError::JobNotFound)
            }
        })
        .await?;

        let _ = self
            .event_tx
            .send(CommonboxEvent::QueryRouted {
                job_id: *job_id,
                query_id: query_id.clone(),
            });

        Ok(query_id)
    }

    /// Get and clear pending queries for a job.
    /// Called by workers to fetch their routed queries.
    pub async fn fetch_pending_queries(&self, job_id: &JobId) -> Vec<RoutedQuery> {
        self.with_state(|state| {
            if let Some(job) = state.jobs.get_mut(job_id) {
                std::mem::take(&mut job.pending_queries)
            } else {
                Vec::new()
            }
        })
        .await
    }

    /// Submit a result for a routed query.
    pub async fn submit_query_result(
        &self,
        job_id: &JobId,
        query_id: String,
        result: JobResult,
    ) -> Result<(), CommonboxError> {
        // TODO: Store query results somewhere accessible to Main
        // For now, just emit an event
        let _ = self
            .event_tx
            .send(CommonboxEvent::QueryResult {
                job_id: *job_id,
                query_id,
                result,
            });
        Ok(())
    }

    /// Mark a job as failed.
    pub async fn fail_job(&self, job_id: &JobId, error: impl Into<String>) -> Result<(), CommonboxError> {
        let error = error.into();
        let agent_id = self
            .with_state(|state| {
                if let Some(job) = state.jobs.get_mut(job_id) {
                    if job.status.is_terminal() {
                        return Err(CommonboxError::InvalidTransition {
                            from: job.status,
                            to: JobStatus::Failed,
                        });
                    }

                    let agent_id = job.agent_id.clone();
                    job.status = JobStatus::Failed;
                    job.completed_at = Some(Utc::now());
                    job.error = Some(error.clone());

                    Ok(agent_id)
                } else {
                    Err(CommonboxError::JobNotFound)
                }
            })
            .await?;

        let _ = self.event_tx.send(CommonboxEvent::JobFailed {
            job_id: *job_id,
            error,
        });

        // Update agent status
        let _ = self
            .update_own_entry(
                &agent_id,
                EntryUpdate::for_agent(agent_id.clone()).with_status(AgentStatus::Failed),
            )
            .await;

        Ok(())
    }

    /// Mark a job as stalled (needs Main resolution).
    pub async fn stall_job(&self, job_id: &JobId, reason: impl Into<String>) -> Result<(), CommonboxError> {
        let reason = reason.into();
        let agent_id = self
            .with_state(|state| {
                if let Some(job) = state.jobs.get_mut(job_id) {
                    if job.status.is_terminal() {
                        return Err(CommonboxError::InvalidTransition {
                            from: job.status,
                            to: JobStatus::Stalled,
                        });
                    }

                    let agent_id = job.agent_id.clone();
                    job.status = JobStatus::Stalled;

                    Ok(agent_id)
                } else {
                    Err(CommonboxError::JobNotFound)
                }
            })
            .await?;

        let _ = self.event_tx.send(CommonboxEvent::JobStalled {
            job_id: *job_id,
            reason: reason.clone(),
        });

        // Update agent entry
        let _ = self
            .update_own_entry(
                &agent_id,
                EntryUpdate::for_agent(agent_id.clone())
                    .with_status(AgentStatus::Stalled)
                    .with_comment(reason),
            )
            .await;

        Ok(())
    }

    /// Add a dependency between jobs.
    pub async fn add_dependency(
        &self,
        job_id: &JobId,
        depends_on: &JobId,
    ) -> Result<(), CommonboxError> {
        if job_id == depends_on {
            return Err(CommonboxError::CircularDependency);
        }

        self.with_state(|state| {
            // Verify both jobs exist
            if !state.jobs.contains_key(job_id) {
                return Err(CommonboxError::JobNotFound);
            }
            if !state.jobs.contains_key(depends_on) {
                return Err(CommonboxError::DependencyNotFound);
            }

            // Add dependency
            if let Some(job) = state.jobs.get_mut(job_id) {
                if !job.dependencies.contains(depends_on) {
                    job.dependencies.push(*depends_on);
                }
            }

            // Add as dependent
            if let Some(dep) = state.jobs.get_mut(depends_on) {
                if !dep.dependents.contains(job_id) {
                    dep.dependents.push(*job_id);
                }
            }

            Ok(())
        })
        .await
    }

    /// Check if a job is ready to run (all dependencies satisfied).
    pub async fn is_job_ready(&self, job_id: &JobId) -> bool {
        let state = self.state.read().await;
        if let Some(job) = state.jobs.get(job_id) {
            if job.status != JobStatus::Pending {
                return false;
            }
            // Check all dependencies are completed
            job.dependencies.iter().all(|dep_id| {
                state
                    .jobs
                    .get(dep_id)
                    .map(|d| d.status == JobStatus::Completed)
                    .unwrap_or(true)
            })
        } else {
            false
        }
    }

    /// List all stalled jobs (for Main resolution).
    pub async fn list_stalled_jobs(&self) -> Vec<(JobId, Job)> {
        self.state
            .read()
            .await
            .jobs
            .iter()
            .filter(|(_, job)| job.status == JobStatus::Stalled)
            .map(|(id, job)| (*id, job.clone()))
            .collect()
    }

    /// List all active jobs.
    pub async fn list_active_jobs(&self) -> Vec<Job> {
        self.state
            .read()
            .await
            .jobs
            .values()
            .filter(|job| job.status.is_active())
            .cloned()
            .collect()
    }

    /// Get count of active jobs.
    pub async fn active_job_count(&self) -> usize {
        self.state
            .read()
            .await
            .jobs
            .values()
            .filter(|job| job.status.is_active())
            .count()
    }

    // =========================================================================
    // Coordination Board (Commonboard)
    // =========================================================================

    /// Add a coordination entry to the commonboard.
    pub async fn add_coordination_entry(
        &self,
        agent_id: AgentId,
        entry_type: impl Into<String>,
        content: impl Into<String>,
        tags: Vec<String>,
    ) {
        let entry = CoordinationEntry {
            id: Uuid::new_v4(),
            agent_id,
            entry_type: entry_type.into(),
            content: content.into(),
            timestamp: Utc::now(),
            tags,
        };

        self.with_state(|state| {
            state.coordination.add(entry);
        })
        .await;
    }

    /// Claim a resource on the commonboard.
    pub async fn claim_resource(
        &self,
        agent_id: AgentId,
        resource: impl Into<String>,
    ) -> Result<(), CommonboxError> {
        let resource = resource.into();

        // Check if already claimed
        let already_claimed = self
            .with_state(|state| {
                !state.coordination.find_claims(&resource).is_empty()
            })
            .await;

        if already_claimed {
            return Err(CommonboxError::ResourceAlreadyClaimed);
        }

        self.add_coordination_entry(agent_id, "claim", resource, vec!["claim".to_string()])
            .await;

        Ok(())
    }

    /// Report progress on the commonboard.
    pub async fn report_progress(&self, agent_id: AgentId, message: impl Into<String>) {
        self.add_coordination_entry(
            agent_id,
            "progress",
            message,
            vec!["progress".to_string()],
        )
        .await;
    }

    /// Mark completion on the commonboard.
    pub async fn mark_complete(&self, agent_id: AgentId, summary: impl Into<String>) {
        self.add_coordination_entry(
            agent_id,
            "complete",
            summary,
            vec!["complete".to_string()],
        )
        .await;
    }

    /// List all coordination entries.
    pub async fn list_coordination(&self) -> Vec<CoordinationEntry> {
        self.state
            .read()
            .await
            .coordination
            .list()
            .to_vec()
    }

    /// Get coordination board formatted for LLM consumption.
    pub async fn get_coordination_snapshot(&self) -> String {
        self.state.read().await.coordination.format_for_llm()
    }

    /// Check if a resource is claimed.
    pub async fn is_resource_claimed(&self, resource: &str) -> Option<AgentId> {
        self.with_state(|state| {
            state
                .coordination
                .find_claims(resource)
                .first()
                .map(|e| e.agent_id.clone())
        })
        .await
    }

    /// Release a resource claim.
    ///
    /// Only the agent that claimed the resource can release it.
    pub async fn release_resource(
        &self,
        agent_id: &AgentId,
        resource: impl Into<String>,
    ) -> Result<(), CommonboxError> {
        let resource = resource.into();

        self.with_state(|state| {
            // Find and remove the claim
            match state.coordination.remove_claim(&resource) {
                Some(entry) => {
                    // Verify this agent owns the claim
                    if &entry.agent_id != agent_id {
                        // Put it back - wrong agent
                        state.coordination.add(entry);
                        Err(CommonboxError::PermissionDenied)
                    } else {
                        Ok(())
                    }
                }
                None => Err(CommonboxError::InvalidState {
                    state: "not_claimed".to_string(),
                    operation: "release_resource".to_string(),
                }),
            }
        })
        .await
    }

    /// List all active resource claims.
    pub async fn list_claims(&self) -> Vec<(String, AgentId)> {
        self.with_state(|state| {
            state
                .coordination
                .list()
                .iter()
                .filter(|e| e.entry_type == "claim")
                .map(|e| (e.content.clone(), e.agent_id.clone()))
                .collect()
        })
        .await
    }

    /// Cleanup old completed entries.
    pub async fn cleanup_coordination(&self, older_than: DateTime<Utc>) {
        self.with_state(|state| {
            state.coordination.cleanup_completed(older_than);
        })
        .await;
    }
}

impl Default for Commonbox {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_agent() {
        let commonbox = Commonbox::new();
        let agent = AgentId::main();

        commonbox.register_agent(agent.clone()).await;

        let entry = commonbox.get_entry(&agent).await;
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().agent_id, agent);
    }

    #[tokio::test]
    async fn test_update_own_entry_success() {
        let commonbox = Commonbox::new();
        let agent = AgentId::main();

        commonbox.register_agent(agent.clone()).await;

        let updates = EntryUpdate::for_agent(agent.clone())
            .with_status(AgentStatus::Processing)
            .with_step_count(5)
            .with_comment("Working");

        let result = commonbox.update_own_entry(&agent, updates).await;
        assert!(result.is_ok());

        let entry = commonbox.get_entry(&agent).await.unwrap();
        assert_eq!(entry.status, AgentStatus::Processing);
        assert_eq!(entry.step_count, 5);
        assert_eq!(entry.comment, "Working");
    }

    #[tokio::test]
    async fn test_update_own_entry_permission_denied() {
        let commonbox = Commonbox::new();
        let main = AgentId::main();
        let worker = AgentId::worker("test");

        commonbox.register_agent(main.clone()).await;
        commonbox.register_agent(worker.clone()).await;

        // Worker tries to update Main's entry
        let updates = EntryUpdate::for_agent(main.clone()).with_status(AgentStatus::Processing);

        let result = commonbox.update_own_entry(&worker, updates).await;
        assert_eq!(result, Err(CommonboxError::PermissionDenied));
    }

    #[tokio::test]
    async fn test_job_lifecycle() {
        let commonbox = Commonbox::new();
        let agent = AgentId::worker("test");

        // Create job
        let job_id = commonbox.create_job(agent.clone(), "Test job").await.unwrap();

        let job = commonbox.get_job(&job_id).await.unwrap();
        assert_eq!(job.status, JobStatus::Pending);

        // Start job
        commonbox.start_job(&job_id).await.unwrap();
        let job = commonbox.get_job(&job_id).await.unwrap();
        assert_eq!(job.status, JobStatus::Running);
        assert!(job.started_at.is_some());

        // Complete job
        let result = JobResult::new("success").unwrap();
        commonbox.complete_job(&job_id, result.clone()).await.unwrap();

        let job = commonbox.get_job(&job_id).await.unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert!(job.completed_at.is_some());
        assert_eq!(job.result.unwrap().as_str(), Some("success"));
    }

    #[tokio::test]
    async fn test_stall_job() {
        let commonbox = Commonbox::new();
        let agent = AgentId::worker("test");

        let job_id = commonbox.create_job(agent.clone(), "Test").await.unwrap();
        commonbox.start_job(&job_id).await.unwrap();

        commonbox.stall_job(&job_id, "Context limit").await.unwrap();

        let job = commonbox.get_job(&job_id).await.unwrap();
        assert_eq!(job.status, JobStatus::Stalled);

        let entry = commonbox.get_entry(&agent).await.unwrap();
        assert_eq!(entry.status, AgentStatus::Stalled);
        assert_eq!(entry.comment, "Context limit");
    }

    #[tokio::test]
    async fn test_job_dependencies() {
        let commonbox = Commonbox::new();
        let agent = AgentId::main();

        let parent = commonbox.create_job(agent.clone(), "Parent").await.unwrap();
        let child = commonbox.create_job(agent.clone(), "Child").await.unwrap();

        // Add dependency
        commonbox.add_dependency(&child, &parent).await.unwrap();

        // Child should not be ready yet
        assert!(!commonbox.is_job_ready(&child).await);

        // Complete parent
        commonbox.start_job(&parent).await.unwrap();
        commonbox
            .complete_job(&parent, JobResult::new("done").unwrap())
            .await
            .unwrap();

        // Child should now be ready
        assert!(commonbox.is_job_ready(&child).await);
    }

    #[tokio::test]
    async fn test_circular_dependency() {
        let commonbox = Commonbox::new();
        let agent = AgentId::main();

        let job = commonbox.create_job(agent.clone(), "Job").await.unwrap();

        let result = commonbox.add_dependency(&job, &job).await;
        assert_eq!(result, Err(CommonboxError::CircularDependency));
    }

    #[tokio::test]
    async fn test_llm_snapshot_generation() {
        let commonbox = Commonbox::new();

        // Register agents
        let main = AgentId::main();
        let worker = AgentId::worker("refactor");

        commonbox.register_agent(main.clone()).await;
        commonbox
            .update_own_entry(
                &main,
                EntryUpdate::for_agent(main.clone())
                    .with_status(AgentStatus::Processing)
                    .with_comment("Coordinating"),
            )
            .await
            .unwrap();

        commonbox.register_agent(worker.clone()).await;
        commonbox
            .update_own_entry(
                &worker,
                EntryUpdate::for_agent(worker.clone())
                    .with_status(AgentStatus::Processing)
                    .with_step_count(25) // Exceeds max_steps to trigger "stalled"
                    .with_comment("Working"),
            )
            .await
            .unwrap();

        let snapshot = commonbox.get_llm_snapshot().await;

        // Should contain both agents
        assert!(snapshot.contains("MAIN"));
        assert!(snapshot.contains("WORKER-refactor"));

        // Should have status abbreviations
        assert!(snapshot.contains("s:Processing"));

        // Should have health classifications
        assert!(
            snapshot.contains("h:good")
                || snapshot.contains("h:stalled")
                || snapshot.contains("h:heavy")
        );

        // Should have comments
        assert!(snapshot.contains("cm:Coordinating"));
    }

    #[tokio::test]
    async fn test_events_broadcast() {
        let commonbox = Commonbox::new();
        let mut rx = commonbox.subscribe();

        let agent = AgentId::worker("test");
        let job_id = commonbox.create_job(agent.clone(), "Test").await.unwrap();

        // First event is AgentRegistered (from register_agent)
        let event = rx.try_recv();
        assert!(event.is_ok());
        assert!(matches!(event.unwrap(), CommonboxEvent::AgentRegistered { .. }));

        // Second event is JobCreated
        let event = rx.try_recv();
        assert!(event.is_ok());
        match event.unwrap() {
            CommonboxEvent::JobCreated { job_id: id, .. } => {
                assert_eq!(id, job_id);
            }
            _ => panic!("Expected JobCreated event, got different event"),
        }
    }

    #[tokio::test]
    async fn test_list_stalled_jobs() {
        let commonbox = Commonbox::new();
        let agent1 = AgentId::worker("w1");
        let agent2 = AgentId::worker("w2");

        let job1 = commonbox.create_job(agent1.clone(), "Job 1").await.unwrap();
        let job2 = commonbox.create_job(agent2.clone(), "Job 2").await.unwrap();

        commonbox.start_job(&job1).await.unwrap();
        commonbox.start_job(&job2).await.unwrap();

        commonbox.stall_job(&job1, "Stalled 1").await.unwrap();
        commonbox.stall_job(&job2, "Stalled 2").await.unwrap();

        let stalled = commonbox.list_stalled_jobs().await;
        assert_eq!(stalled.len(), 2);
    }

    #[tokio::test]
    async fn test_active_job_count() {
        let commonbox = Commonbox::new();
        let agent = AgentId::main();

        assert_eq!(commonbox.active_job_count().await, 0);

        let job1 = commonbox.create_job(agent.clone(), "Job 1").await.unwrap();
        let job2 = commonbox.create_job(agent.clone(), "Job 2").await.unwrap();

        // Pending is active (not terminal)
        assert_eq!(commonbox.active_job_count().await, 2);

        commonbox.start_job(&job1).await.unwrap();
        commonbox.start_job(&job2).await.unwrap();

        assert_eq!(commonbox.active_job_count().await, 2);

        commonbox
            .complete_job(&job1, JobResult::new("done").unwrap())
            .await
            .unwrap();

        assert_eq!(commonbox.active_job_count().await, 1);
    }

    #[tokio::test]
    async fn test_job_idle_lifecycle() {
        let commonbox = Commonbox::new();
        let agent = AgentId::worker("test-worker");

        let job = commonbox.create_job(agent.clone(), "Test job").await.unwrap();
        commonbox.start_job(&job).await.unwrap();

        // Transition to Idle
        commonbox.idle_job(&job).await.unwrap();

        let job_state = commonbox.get_job(&job).await.unwrap();
        assert_eq!(job_state.status, JobStatus::Idle);

        // Should be in idle workers list
        let idle = commonbox.list_idle_workers().await;
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].0, job);

        // Wake worker
        commonbox.wake_worker(&job).await.unwrap();

        let job_state = commonbox.get_job(&job).await.unwrap();
        assert_eq!(job_state.status, JobStatus::Running);

        // Should no longer be idle
        let idle = commonbox.list_idle_workers().await;
        assert!(idle.is_empty());
    }

    #[tokio::test]
    async fn test_idle_invalid_transitions() {
        let commonbox = Commonbox::new();
        let agent = AgentId::worker("test-worker");

        let job = commonbox.create_job(agent.clone(), "Test job").await.unwrap();

        // Can't idle a Pending job
        let result = commonbox.idle_job(&job).await;
        assert!(matches!(result, Err(CommonboxError::InvalidTransition { .. })));

        commonbox.start_job(&job).await.unwrap();
        commonbox.idle_job(&job).await.unwrap();

        // Can't idle an already Idle job
        let result = commonbox.idle_job(&job).await;
        assert!(matches!(result, Err(CommonboxError::InvalidTransition { .. })));

        // Can't wake a Running job (must be Idle)
        commonbox.wake_worker(&job).await.unwrap();
        let result = commonbox.wake_worker(&job).await;
        assert!(matches!(result, Err(CommonboxError::InvalidTransition { .. })));
    }

    #[tokio::test]
    async fn test_query_routing() {
        let commonbox = Commonbox::new();
        let agent = AgentId::worker("test-worker");

        let job = commonbox.create_job(agent.clone(), "Test job").await.unwrap();
        commonbox.start_job(&job).await.unwrap();
        commonbox.idle_job(&job).await.unwrap();

        // Route a query to the idle worker
        let query_id = commonbox
            .route_query(&job, "What is the status?", Some("Context".to_string()))
            .await
            .unwrap();
        assert!(!query_id.is_empty());

        // Worker fetches pending queries
        let pending = commonbox.fetch_pending_queries(&job).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].query, "What is the status?");
        assert_eq!(pending[0].context, Some("Context".to_string()));

        // After fetch, pending should be empty
        let pending = commonbox.fetch_pending_queries(&job).await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_archive_job() {
        let commonbox = Commonbox::new();
        let agent = AgentId::worker("test-worker");

        let job = commonbox.create_job(agent.clone(), "Test job").await.unwrap();
        commonbox.start_job(&job).await.unwrap();
        commonbox.stall_job(&job, "Test stall").await.unwrap();

        // Verify job is stalled
        let job_state = commonbox.get_job(&job).await.unwrap();
        assert_eq!(job_state.status, JobStatus::Stalled);

        // Archive the job (Stalled → Completed)
        commonbox
            .archive_job(&job, JobResult::new("archived").unwrap())
            .await
            .unwrap();

        // Verify job is completed
        let job_state = commonbox.get_job(&job).await.unwrap();
        assert_eq!(job_state.status, JobStatus::Completed);
        assert!(job_state.result.is_some());
    }

    #[tokio::test]
    async fn test_query_routing_to_non_idle_worker() {
        let commonbox = Commonbox::new();
        let agent = AgentId::worker("test-worker");

        // Can't route to Pending worker
        let job = commonbox.create_job(agent.clone(), "Test job").await.unwrap();
        let result = commonbox.route_query(&job, "Query", None).await;
        assert!(matches!(result, Err(CommonboxError::InvalidState { .. })));

        // Can route to Running worker
        commonbox.start_job(&job).await.unwrap();
        let result = commonbox.route_query(&job, "Query", None).await;
        assert!(result.is_ok());

        // Can route to Idle worker
        commonbox.idle_job(&job).await.unwrap();
        let result = commonbox.route_query(&job, "Query 2", None).await;
        assert!(result.is_ok());

        // Can't route to Completed worker
        commonbox.wake_worker(&job).await.unwrap();
        commonbox
            .complete_job(&job, JobResult::new("done").unwrap())
            .await
            .unwrap();
        let result = commonbox.route_query(&job, "Query 3", None).await;
        assert!(matches!(result, Err(CommonboxError::InvalidState { .. })));
    }
}
