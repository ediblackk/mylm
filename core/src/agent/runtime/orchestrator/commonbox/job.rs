//! Job types and status

use crate::agent::identity::AgentId;
use crate::agent::runtime::orchestrator::commonbox::id::JobId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Typed job result for type safety.
///
/// Wraps serde_json::Value with a named boundary for future tightening.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult(pub serde_json::Value);

impl JobResult {
    /// Create from any serializable type.
    pub fn new<T: Serialize>(value: T) -> Result<Self, serde_json::Error> {
        Ok(Self(serde_json::to_value(value)?))
    }

    /// Get as string if it's a string value.
    pub fn as_str(&self) -> Option<&str> {
        self.0.as_str()
    }

    /// Extract to a typed value.
    pub fn extract<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.0.clone())
    }

    /// Get inner Value.
    pub fn inner(&self) -> &serde_json::Value {
        &self.0
    }
}

impl From<serde_json::Value> for JobResult {
    fn from(value: serde_json::Value) -> Self {
        Self(value)
    }
}

/// Job status in lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    /// Worker completed initial task, waiting for routed queries
    Idle,
    Completed,
    Failed,
    Stalled,
    Cancelled,
}

impl JobStatus {
    /// Check if terminal (completed, failed, stalled, or cancelled).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Stalled | JobStatus::Cancelled
        )
    }

    /// Check if active (not terminal).
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }
}

/// A query routed from Main to an idle worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutedQuery {
    /// Unique ID for this query
    pub id: String,
    /// The query content
    pub query: String,
    /// Context/context for the query
    pub context: Option<String>,
    /// When the query was sent
    pub sent_at: DateTime<Utc>,
}

impl RoutedQuery {
    /// Create a new routed query.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            query: query.into(),
            context: None,
            sent_at: Utc::now(),
        }
    }

    /// Add context to the query.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

/// A job tracked in the Commonbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique job ID
    pub id: JobId,
    /// Agent executing this job
    pub agent_id: AgentId,
    /// Human-readable description
    pub description: String,
    /// Current status
    pub status: JobStatus,
    /// When created
    pub created_at: DateTime<Utc>,
    /// When started (if started)
    pub started_at: Option<DateTime<Utc>>,
    /// When completed (if completed)
    pub completed_at: Option<DateTime<Utc>>,
    /// Job result (if completed)
    pub result: Option<JobResult>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Status message for progress reporting
    pub status_message: Option<String>,
    /// Dependencies that must complete first
    pub dependencies: Vec<JobId>,
    /// Jobs waiting on this one
    pub dependents: Vec<JobId>,
    /// Pending routed queries (for idle workers)
    pub pending_queries: Vec<RoutedQuery>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_job_result_typing() {
        // Test new() with serializable
        #[derive(Serialize)]
        struct MyResult {
            value: i32,
            message: String,
        }

        let result = JobResult::new(MyResult {
            value: 42,
            message: "Hello".to_string(),
        })
        .unwrap();

        // Test extraction
        #[derive(Deserialize, Debug, PartialEq)]
        struct Extracted {
            value: i32,
            message: String,
        }

        let extracted: Extracted = result.extract().unwrap();
        assert_eq!(extracted.value, 42);
        assert_eq!(extracted.message, "Hello");
    }
}
