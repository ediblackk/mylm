//! Local Worker Capability
//!
//! Spawns workers as tokio tasks with parent-child communication.

use crate::agent::runtime::{
    capability::{Capability, WorkerCapability, WorkerHandle},
    context::RuntimeContext,
    error::WorkerError,
};
use crate::agent::types::intents::WorkerSpec;
use crate::agent::types::events::WorkerId;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

/// Worker status
#[derive(Debug, Clone)]
pub enum WorkerStatus {
    Running,
    Completed(String),
    Failed(String),
}

/// Worker instance
struct WorkerInstance {
    _handle: JoinHandle<()>,
    #[allow(dead_code)]
    status_tx: Option<mpsc::Sender<WorkerStatus>>,
    #[allow(dead_code)]
    id: WorkerId,
}

/// Local worker capability - spawns tokio tasks
pub struct LocalWorkerCapability {
    workers: Arc<Mutex<HashMap<WorkerId, WorkerInstance>>>,
    results: Arc<Mutex<HashMap<WorkerId, WorkerStatus>>>,
    next_id: AtomicU64,
}

impl LocalWorkerCapability {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(Mutex::new(HashMap::new())),
            results: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
        }
    }
    
    fn generate_id(&self) -> WorkerId {
        WorkerId(self.next_id.fetch_add(1, Ordering::SeqCst))
    }
    
    /// Get worker status
    pub async fn get_status(&self, id: &WorkerId) -> Option<WorkerStatus> {
        self.results.lock().await.get(id).cloned()
    }
    
    /// List all workers
    pub async fn list_workers(&self) -> Vec<WorkerId> {
        self.workers.lock().await.keys().cloned().collect()
    }
    
    /// Spawn a simple worker task
    async fn spawn_worker(
        &self,
        spec: WorkerSpec,
        results: Arc<Mutex<HashMap<WorkerId, WorkerStatus>>>,
    ) -> Result<WorkerHandle, WorkerError> {
        let worker_id = self.generate_id();
        let (status_tx, _status_rx) = mpsc::channel(10);
        
        // Spawn the worker task
        let worker_id_clone = worker_id.clone();
        let results_clone = results.clone();
        
        let handle = tokio::spawn(async move {
            // Worker execution logic
            let result = execute_worker_objective(&spec.objective).await;
            
            // Store result
            let status = match result {
                Ok(output) => WorkerStatus::Completed(output),
                Err(e) => WorkerStatus::Failed(e),
            };
            
            results_clone.lock().await.insert(worker_id_clone.clone(), status.clone());
            
            // Notify parent (if channel still open)
            let _ = status_tx.send(status).await;
        });
        
        // Store worker instance
        let instance = WorkerInstance {
            _handle: handle,
            status_tx: None,
            id: worker_id.clone(),
        };
        
        self.workers.lock().await.insert(worker_id.clone(), instance);
        
        Ok(WorkerHandle { id: worker_id })
    }
}

impl Default for LocalWorkerCapability {
    fn default() -> Self {
        Self::new()
    }
}

impl Capability for LocalWorkerCapability {
    fn name(&self) -> &'static str {
        "local-worker"
    }
}

#[async_trait::async_trait]
impl WorkerCapability for LocalWorkerCapability {
    async fn spawn(
        &self,
        _ctx: &RuntimeContext,
        spec: WorkerSpec,
    ) -> Result<WorkerHandle, WorkerError> {
        self.spawn_worker(spec, self.results.clone()).await
    }
}

/// Execute a worker's objective
async fn execute_worker_objective(objective: &str) -> Result<String, String> {
    // For now, workers are simple task executors
    // In production, this would create a nested Session with its own engine
    
    // Simulate work
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    Ok(format!("Worker completed: {}", objective))
}

/// Worker session - runs a nested cognitive session
pub struct WorkerSession {
    worker_id: WorkerId,
    objective: String,
    result_tx: mpsc::Sender<WorkerResult>,
}

#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub worker_id: WorkerId,
    pub output: String,
    pub success: bool,
}

impl WorkerSession {
    pub fn new(
        worker_id: WorkerId,
        objective: String,
        result_tx: mpsc::Sender<WorkerResult>,
    ) -> Self {
        Self {
            worker_id,
            objective,
            result_tx,
        }
    }
    
    /// Run the worker session
    pub async fn run(self) {
        // This would create a full nested Session with:
        // - Its own CognitiveEngine
        // - Its own RuntimeContext
        // - Access to same capabilities (or subset)
        
        // For now, simple execution
        let result = execute_worker_objective(&self.objective).await;
        
        let worker_result = match result {
            Ok(output) => WorkerResult {
                worker_id: self.worker_id,
                output,
                success: true,
            },
            Err(e) => WorkerResult {
                worker_id: self.worker_id,
                output: e,
                success: false,
            },
        };
        
        let _ = self.result_tx.send(worker_result).await;
    }
}

/// Worker pool for managing multiple workers
pub struct WorkerPool {
    workers: Arc<Mutex<HashMap<WorkerId, WorkerSession>>>,
    max_workers: usize,
}

impl WorkerPool {
    pub fn new(max_workers: usize) -> Self {
        Self {
            workers: Arc::new(Mutex::new(HashMap::new())),
            max_workers,
        }
    }
    
    pub async fn spawn(&self, _objective: String) -> Result<WorkerId, String> {
        let workers = self.workers.lock().await;
        if workers.len() >= self.max_workers {
            return Err("Max workers reached".to_string());
        }
        
        // Would store and spawn here
        // For now, just return a new ID
        Ok(WorkerId(0))
    }
}
