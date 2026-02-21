//! Commonbox event types

use crate::agent::identity::AgentId;
use crate::agent::runtime::orchestrator::commonbox::id::JobId;
use crate::agent::runtime::orchestrator::commonbox::job::JobResult;
use serde::{Deserialize, Serialize};

/// Events broadcast by Commonbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommonboxEvent {
    /// Agent entry was created
    AgentRegistered { agent_id: AgentId },
    /// Agent entry was updated
    AgentUpdated { agent_id: AgentId },
    /// Job was created
    JobCreated { job_id: JobId, agent_id: AgentId },
    /// Job started running
    JobStarted { job_id: JobId },
    /// Job completed initial task, now idle (waiting for routed queries)
    JobIdle { job_id: JobId },
    /// Job completed successfully
    JobCompleted { job_id: JobId, result: JobResult },
    /// Job failed
    JobFailed { job_id: JobId, error: String },
    /// Job stalled (needs Main resolution)
    JobStalled { job_id: JobId, reason: String },
    /// Job was cancelled
    JobCancelled { job_id: JobId },
    /// Dependency completed (for waiting jobs)
    DependencyCompleted { job_id: JobId, dependency_id: JobId },
    /// Query routed to a worker
    QueryRouted { job_id: JobId, query_id: String },
    /// Query result from a worker
    QueryResult { job_id: JobId, query_id: String, result: JobResult },
}
