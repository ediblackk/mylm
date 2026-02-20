//! Worker System
//!
//! Workers are independent cognitive agents that run in separate tokio tasks.
//! Each worker has its own AgencySession with Planner kernel.

use crate::agent::{
    WorkerId,
    factory::{AgentSessionFactory, WorkerSessionConfig},
    cognition::Planner,
};
use crate::agent::runtime::session::{Session, UserInput, SessionResult, OutputEvent};
use crate::agent::runtime::session::session::AgencySession;
use crate::agent::runtime::ContractRuntime;
use crate::agent::runtime::capabilities::InMemoryTransport;
use tokio::sync::{oneshot, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use crate::config::Config;

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
    config: Config,
    next_id: AtomicU64,
}

impl WorkerManager {
    /// Create a new worker manager with the given config
    pub fn new(config: Config) -> Self {
        Self { 
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
        
        // Build worker configuration
        let worker_config = WorkerSessionConfig {
            allowed_tools: vec![], // Default: no pre-approved tools
            scratchpad: None,
            output_tx: None,
            objective: spec.objective.clone(),
            instructions: None,
            tags: None,
        };
        
        // Clone config for the async task
        let config = self.config.clone();
        let worker_id_clone = worker_id.clone();
        
        // Spawn worker task
        tokio::spawn(async move {
            // Create factory and session
            let factory = AgentSessionFactory::new(config);
            let session: Arc<RwLock<AgencySession<Planner, ContractRuntime, InMemoryTransport>>> = match factory.create_configured_worker_session(
                &spec.id,
                worker_config
            ).await {
                Ok(s) => Arc::new(RwLock::new(s)),
                Err(e) => {
                    let _ = result_tx.send(WorkerResult {
                        worker_id: worker_id_clone,
                        output: format!("Failed to create worker session: {}", e),
                        success: false,
                    });
                    return;
                }
            };
            
            // Subscribe to output events BEFORE starting run
            let mut output_rx = {
                let sess = session.read().await;
                sess.subscribe_output()
            };
            
            // Clone for the run task
            let session_for_run = Arc::clone(&session);
            
            // Spawn the session run loop
            let mut run_handle = tokio::spawn(async move {
                let mut sess = session_for_run.write().await;
                sess.run().await
            });
            
            // Submit the objective as user input
            let objective_input = UserInput::Message(spec.objective);
            let submit_result = {
                let sess = session.read().await;
                sess.submit_input(objective_input).await
            };
            
            if let Err(e) = submit_result {
                crate::error_log!("[WORKER] Failed to submit objective: {}", e);
            }
            
            // Collect output from output channel while waiting for completion
            let mut output_buffer = Vec::new();
            let mut success = false;
            
            // Process output events while session runs
            loop {
                tokio::select! {
                    // Check for output events
                    Ok(event) = output_rx.recv() => {
                        match event {
                            OutputEvent::ResponseChunk { content } => {
                                output_buffer.push(content);
                            }
                            OutputEvent::ResponseComplete => {
                                // Response is complete
                            }
                            OutputEvent::Halted { .. } => {
                                success = true;
                                break;
                            }
                            OutputEvent::Error { message } => {
                                output_buffer.push(format!("Error: {}", message));
                                break;
                            }
                            _ => {}
                        }
                    }
                    // Wait for session to complete
                    run_result = &mut run_handle => {
                        match run_result {
                            Ok(Ok(SessionResult { completed_successfully, halt_reason, .. })) => {
                                success = completed_successfully;
                                if let Some(reason) = halt_reason {
                                    output_buffer.push(format!("Halted: {}", reason));
                                }
                            }
                            Ok(Err(e)) => {
                                output_buffer.push(format!("Session error: {}", e));
                            }
                            Err(e) => {
                                output_buffer.push(format!("Worker task panicked: {}", e));
                            }
                        }
                        break;
                    }
                }
            }
            
            // Send result back to parent
            let worker_result = WorkerResult {
                worker_id: worker_id_clone,
                output: output_buffer.join(""),
                success,
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
When done, respond with a summary of what you accomplished.

Be concise and focused."#, objective)
}

/// Worker capability implementation
pub struct WorkerCapabilityImpl {
    config: Config,
}

impl WorkerCapabilityImpl {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl crate::agent::runtime::core::Capability for WorkerCapabilityImpl {
    fn name(&self) -> &'static str {
        "worker-manager"
    }
}

#[async_trait::async_trait]
impl crate::agent::runtime::core::WorkerCapability for WorkerCapabilityImpl {
    async fn spawn(
        &self,
        _ctx: &crate::agent::runtime::core::RuntimeContext,
        spec: crate::agent::types::intents::WorkerSpec,
    ) -> Result<crate::agent::runtime::core::WorkerSpawnHandle, crate::agent::runtime::core::WorkerError> {
        // Generate an ID for the worker
        let worker_id = WorkerId(self.config.active_profile.len() as u64 + 1); // Temporary ID generation
        
        let manager = WorkerManager::new(self.config.clone());
        let worker_spec = WorkerSpawnParams {
            id: worker_id.0.to_string(),
            objective: spec.objective,
            parent_trace_id: "parent".to_string(),
        };
        
        manager.spawn(worker_spec).await
            .map(|handle| crate::agent::runtime::core::WorkerSpawnHandle {
                id: handle.id,
            })
            .map_err(|e| crate::agent::runtime::core::WorkerError::new(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    
    #[tokio::test]
    async fn test_worker_spawn() {
        let config = Config::load_or_default();
        let manager = WorkerManager::new(config);
        
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
        let config = Config::load_or_default();
        let manager = WorkerManager::new(config);
        
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
