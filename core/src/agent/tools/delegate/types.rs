//! Types for the delegate tool - Worker configurations and arguments

use crate::agent::runtime::orchestrator::commonbox::JobId;
use crate::agent::types::events::WorkerId;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

/// Worker configuration - each worker gets a specific task and tool set
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct WorkerConfig {
    /// Unique identifier (e.g., "models", "handlers")
    pub id: String,
    /// Specific task for this worker
    pub objective: String,
    /// Additional system prompt instructions
    #[serde(default)]
    pub instructions: Option<String>,
    /// Allowed tools (subset of parent's tools). If empty/none, all tools allowed.
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    /// Auto-approved command patterns (e.g., ["cargo check *"])
    #[serde(default)]
    pub allowed_commands: Option<Vec<String>>,
    /// Forbidden command patterns (e.g., ["rm -rf *"])
    #[serde(default)]
    pub forbidden_commands: Option<Vec<String>>,
    /// Tags for scratchpad coordination entries
    #[serde(default)]
    pub tags: Vec<String>,
    /// Worker IDs that must complete before this worker starts
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Optional context specific to this worker
    #[serde(default)]
    pub context: Option<serde_json::Value>,
    /// Maximum iterations for this worker
    #[serde(default)]
    pub max_iterations: Option<usize>,
    /// Timeout for initial session execution (seconds)
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Delegate tool arguments
#[derive(Deserialize, Serialize)]
pub struct DelegateArgs {
    /// Shared context for all workers
    #[serde(default)]
    pub shared_context: Option<String>,
    /// Worker configurations (1+ required)
    pub workers: Vec<WorkerConfig>,
}

/// Information about a spawned worker
#[derive(Debug)]
pub struct SpawnedWorker {
    pub config: WorkerConfig,
    pub job_id: JobId,
    pub worker_id: WorkerId,
    pub handle: JoinHandle<()>,
}
