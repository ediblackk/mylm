//! Worker System
//!
//! Workers are independent cognitive agents that run in separate tokio tasks.
//! Each worker has its own Session with engine and runtime.

use crate::agent::{
    Session, SessionConfig, SessionInput,
    WorkerId,
    runtime::AgentRuntime,
    cognition::{LLMBasedEngine},
};
use tokio::sync::{mpsc, oneshot};
use std::sync::atomic::{AtomicU64, Ordering};

/// Worker handle - returned when spawning a worker
pub struct WorkerHandle {
    pub id: WorkerId,
    result_rx: Option<oneshot::Receiver<WorkerResult>>,
}

/// Worker execution result
#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub worker_id: WorkerId,
    pub output: String,
    pub success: bool,
}

/// Internal worker spawn parameters
/// 
/// This is used internally by WorkerManager. For the contract type
/// used in Intent::SpawnWorker, see `crate::agent::types::intents::WorkerSpec`.
#[derive(Debug, Clone)]
pub struct WorkerSpawnParams {
    pub id: String,
    pub objective: String,
    pub parent_trace_id: String,
}

/// Worker manager - handles spawning and monitoring workers
pub struct WorkerManager {
    runtime: AgentRuntime,
    config: SessionConfig,
    next_id: AtomicU64,
}

impl WorkerManager {
    pub fn new(runtime: AgentRuntime, config: SessionConfig) -> Self {
        Self { 
            runtime, 
            config,
            next_id: AtomicU64::new(1),
        }
    }
    
    fn generate_id(&self) -> WorkerId {
        WorkerId(self.next_id.fetch_add(1, Ordering::SeqCst))
    }
    
    /// Spawn a new worker with its own session
    pub async fn spawn(
        &self,
        spec: WorkerSpawnParams,
    ) -> Result<WorkerHandle, String> {
        let worker_id = self.generate_id();
        
        // Create result channel
        let (result_tx, result_rx) = oneshot::channel();
        
        // Create input channel for worker
        let (input_tx, input_rx) = mpsc::channel(100);
        
        // Send initial objective as chat message
        let objective_msg = SessionInput::Chat(spec.objective.clone());
        input_tx.send(objective_msg).await
            .map_err(|e| format!("Failed to send objective: {}", e))?;
        
        // Clone runtime for worker
        let worker_runtime = self.runtime.clone();
        let worker_config = self.config.clone();
        
        // Spawn worker task
        let worker_id_clone = worker_id.clone();
        tokio::spawn(async move {
            // Create engine for worker
            let engine = LLMBasedEngine::new()
                .with_system_prompt(build_worker_prompt(&spec.objective));
            
            // Create session
            let mut session = Session::new(engine, worker_runtime, worker_config);
            
            // Run session
            let result = session.run(input_rx).await;
            
            // Send result back to parent
            let worker_result = match result {
                Ok(output) => WorkerResult {
                    worker_id: worker_id_clone,
                    output,
                    success: true,
                },
                Err(e) => WorkerResult {
                    worker_id: worker_id_clone,
                    output: format!("Worker failed: {}", e),
                    success: false,
                },
            };
            
            let _ = result_tx.send(worker_result);
        });
        
        Ok(WorkerHandle {
            id: worker_id,
            result_rx: Some(result_rx),
        })
    }
    
    /// Spawn a simple worker that executes a task without full cognitive loop
    pub async fn spawn_simple<F, Fut>(
        &self,
        _id: String,
        task: F,
    ) -> Result<WorkerHandle, String>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send,
    {
        let worker_id = self.generate_id();
        let (result_tx, result_rx) = oneshot::channel();
        
        let worker_id_clone = worker_id.clone();
        tokio::spawn(async move {
            let result = task().await;
            
            let worker_result = match result {
                Ok(output) => WorkerResult {
                    worker_id: worker_id_clone,
                    output,
                    success: true,
                },
                Err(e) => WorkerResult {
                    worker_id: worker_id_clone,
                    output: e,
                    success: false,
                },
            };
            
            let _ = result_tx.send(worker_result);
        });
        
        Ok(WorkerHandle {
            id: worker_id,
            result_rx: Some(result_rx),
        })
    }
}

impl WorkerHandle {
    /// Wait for worker to complete and return result
    pub async fn wait(mut self) -> Result<WorkerResult, String> {
        if let Some(rx) = self.result_rx.take() {
            rx.await.map_err(|e| format!("Worker channel closed: {}", e))
        } else {
            Err("Result receiver not available".to_string())
        }
    }
    
    /// Check if worker is still running (non-blocking)
    pub fn is_running(&self) -> bool {
        self.result_rx.is_some()
    }
    
    /// Get worker ID
    pub fn id(&self) -> &WorkerId {
        &self.id
    }
}

/// Build system prompt for worker
fn build_worker_prompt(objective: &str) -> String {
    format!(r#"You are a worker agent focused on completing a specific task.

Your objective: {}

You have access to the same tools as the main agent (shell, read_file, write_file, etc.).
Focus on completing your assigned task efficiently.
When done, respond with <exit/> or provide a summary of what you accomplished.

Be concise and focused."#, objective)
}

/// Worker capability implementation
pub struct WorkerCapabilityImpl {
    manager: WorkerManager,
}

impl WorkerCapabilityImpl {
    pub fn new(runtime: AgentRuntime, config: SessionConfig) -> Self {
        Self {
            manager: WorkerManager::new(runtime, config),
        }
    }
}

impl crate::agent::runtime::capability::Capability for WorkerCapabilityImpl {
    fn name(&self) -> &'static str {
        "worker-manager"
    }
}

#[async_trait::async_trait]
impl crate::agent::runtime::capability::WorkerCapability for WorkerCapabilityImpl {
    async fn spawn(
        &self,
        _ctx: &crate::agent::runtime::context::RuntimeContext,
        spec: crate::agent::types::intents::WorkerSpec,
    ) -> Result<crate::agent::runtime::capability::WorkerHandle, crate::agent::runtime::error::WorkerError> {
        // Generate an ID for the worker
        let worker_id = self.manager.generate_id();
        let worker_spec = WorkerSpawnParams {
            id: worker_id.0.to_string(),
            objective: spec.objective,
            parent_trace_id: "parent".to_string(),
        };
        
        self.manager.spawn(worker_spec).await
            .map(|handle| crate::agent::runtime::capability::WorkerHandle {
                id: handle.id,
            })
            .map_err(|e| crate::agent::runtime::error::WorkerError::new(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentBuilder;
    
    #[tokio::test]
    async fn test_worker_spawn() {
        let runtime = AgentBuilder::new()
            .with_auto_approve()
            .build_runtime();
        
        let manager = WorkerManager::new(runtime, SessionConfig { max_steps: 10 });
        
        let handle = manager.spawn(WorkerSpawnParams {
            id: "test-worker".to_string(),
            objective: "List current directory".to_string(),
            parent_trace_id: "test".to_string(),
        }).await.expect("Failed to spawn worker");
        
        // Wait for result with timeout
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(30),
            handle.wait()
        ).await;
        
        match result {
            Ok(Ok(worker_result)) => {
                println!("Worker result: {}", worker_result.output);
            }
            Ok(Err(e)) => {
                println!("Worker error: {}", e);
            }
            Err(_) => {
                println!("Worker timed out");
            }
        }
    }
    
    #[tokio::test]
    async fn test_simple_worker() {
        let runtime = AgentBuilder::new()
            .with_auto_approve()
            .build_runtime();
        
        let manager = WorkerManager::new(runtime, SessionConfig { max_steps: 10 });
        
        let handle = manager.spawn_simple(
            "simple-test".to_string(),
            || async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok("Simple task completed".to_string())
            },
        ).await.expect("Failed to spawn simple worker");
        
        let result = handle.wait().await.expect("Failed to get result");
        assert!(result.success);
        assert_eq!(result.output, "Simple task completed");
    }
}
