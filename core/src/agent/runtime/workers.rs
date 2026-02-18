//! Worker job runtime

use crate::agent::types::ids::JobId;
use std::collections::HashMap;
use tokio::sync::RwLock;

/// Worker job status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Job handle for tracking background jobs
#[derive(Debug, Clone)]
pub struct JobHandle {
    pub id: JobId,
    pub status: WorkerStatus,
}

/// Worker runtime - manages background jobs
#[derive(Debug, Default)]
pub struct WorkerRuntime {
    workers: RwLock<HashMap<JobId, JobHandle>>,
}

impl WorkerRuntime {
    pub fn new() -> Self {
        Self {
            workers: RwLock::new(HashMap::new()),
        }
    }
    
    pub async fn spawn(&self, id: JobId) -> JobHandle {
        let handle = JobHandle {
            id: id.clone(),
            status: WorkerStatus::Running,
        };
        self.workers.write().await.insert(id, handle.clone());
        handle
    }
    
    pub async fn complete(&self, id: &JobId, _result: String) {
        if let Some(worker) = self.workers.write().await.get_mut(id) {
            worker.status = WorkerStatus::Completed;
        }
    }
    
    pub async fn list_active(&self) -> Vec<JobHandle> {
        self.workers
            .read()
            .await
            .values()
            .filter(|w| w.status == WorkerStatus::Running)
            .cloned()
            .collect()
    }
}
