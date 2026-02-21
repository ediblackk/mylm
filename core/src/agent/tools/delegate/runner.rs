//! Worker runner - manages the lifecycle (coordination) of spawned worker sessions
//!
//! This module handles COORDINATION:
//! - Dependency waiting
//! - Output forwarding with filtering
//! - Session execution (run loop)
//! - Idle loop and query processing
//! - Cleanup and completion
//!
//! It does NOT handle creation - that's in creator.rs
//!
//! NOTE:
//! Worker query processing is currently a stub.
//! Queries are acknowledged but not executed through a cognitive loop.
//! The worker session is consumed during initial objective execution.
//! Proper implementation requires re-entrant session or dedicated query session.

use super::types::{SpawnedWorker, WorkerConfig};
use super::filter::WorkerEventFilter;
use super::creator::{create_worker_session, WorkerCreationContext, emit_worker_spawned_event};
use crate::agent::runtime::orchestrator::commonbox::{Commonbox, JobId, CommonboxEvent};
use crate::agent::runtime::orchestrator::{Session, UserInput, OutputEvent};

use crate::agent::types::events::WorkerId;
use crate::agent::runtime::orchestrator::OutputSender;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Spawn a single worker - COORDINATION entry point
/// 
/// This function ORCHESTRATES worker spawning:
/// 1. Calls creator to CREATE the worker session
/// 2. Sets up coordination state (id mapping)
/// 3. Spawns the coordination task (run_worker_session)
pub async fn spawn_worker(
    config: &WorkerConfig,
    shared_context: &Option<String>,
    worker_index: usize,
    id_to_job: Arc<RwLock<HashMap<String, JobId>>>,
    commonbox: Arc<Commonbox>,
    factory: crate::agent::AgentSessionFactory,
    output_tx: Option<OutputSender>,
) -> Result<SpawnedWorker, String> {
    // Step 1: CREATE worker session (delegated to creator module)
    let creation_ctx = WorkerCreationContext {
        commonbox: commonbox.clone(),
        factory: factory.clone(),
        worker_index,
    };
    
    // Destructure created worker to extract all parts
    let super::creator::CreatedWorker {
        config: _,
        job_id,
        worker_id,
        session,
        worker_output_rx,
    } = create_worker_session(config, shared_context, &creation_ctx).await?;
    
    // Step 2: COORDINATION - Set up tracking and state
    emit_worker_spawned_event(&output_tx, worker_id, job_id, config.objective.clone(), config.id.clone());
    
    // Add to id mapping for dependency resolution
    {
        let mut map = id_to_job.write().await;
        map.insert(config.id.clone(), job_id);
    }
    
    // Update job status
    commonbox.update_job_status_message(&job_id, "Waiting for dependencies...").await;
    
    // Step 3: COORDINATION - Clone values for async block
    let config_clone = config.clone();
    let shared_ctx = shared_context.clone();
    let commonbox_clone = commonbox.clone();
    let id_to_job_clone = id_to_job.clone();
    let parent_output_tx = output_tx.clone();
    
    crate::info_log!("[RUNNER] Spawning worker [{}] with session at {:p}", config.id, &session);
    
    // Step 4: COORDINATION - Spawn the runner task
    // The runner task handles: dependency wait, session run, idle loop, cleanup
    let handle = tokio::spawn(async move {
        run_worker_session(
            config_clone,
            shared_ctx,
            job_id,
            worker_id,
            commonbox_clone,
            session,
            id_to_job_clone,
            worker_output_rx,
            parent_output_tx,
        ).await;
    });
    
    Ok(SpawnedWorker {
        config: config.clone(),
        job_id,
        worker_id,
        handle,
    })
}

/// Run a worker session lifecycle - COORDINATION
/// 
/// This function handles COORDINATION:
/// - Dependency waiting
/// - Session execution (run loop)
/// - Output forwarding with filtering
/// - Idle loop and query processing  
/// - Job state transitions (start, idle, complete, fail)
/// 
/// The session is pre-created by creator.rs (creation is separate from coordination).
pub async fn run_worker_session(
    config: WorkerConfig,
    shared_context: Option<String>,
    job_id: JobId,
    worker_id: WorkerId,
    commonbox: Arc<Commonbox>,
    mut session: crate::agent::runtime::orchestrator::AgencySession<
        crate::agent::cognition::Planner,
        crate::agent::runtime::orchestrator::ContractRuntime,
        crate::agent::runtime::capabilities::InMemoryTransport,
    >,
    id_to_job: Arc<RwLock<HashMap<String, JobId>>>,
    // Worker channel: mpsc receiver - buffers events until forwarder starts
    mut worker_output_rx: tokio::sync::mpsc::Receiver<OutputEvent>,
    // Parent channel: forwarder sends filtered events here
    parent_output_tx: Option<OutputSender>,
) {
    crate::info_log!("[WORKER] Worker [{}] (job={}) STARTING with objective: {}", config.id, job_id.0, config.objective);
    crate::info_log!("[WORKER] Worker [{}] has isolated mpsc channel, parent forwarding: {}", config.id, parent_output_tx.is_some());
    
    // Wait for dependencies before starting
    if !config.depends_on.is_empty() {
        let id_map = id_to_job.read().await.clone();
        
        // Subscribe to job events
        let mut subscriber = commonbox.subscribe();
        let mut pending: std::collections::HashSet<String> = config.depends_on.iter().cloned().collect();
        
        crate::info_log!("Worker [{}] waiting for: {:?}", config.id, pending);
        commonbox.update_job_status_message(&job_id, 
            &format!("Waiting for dependencies: {:?}", pending)).await;
        
        while !pending.is_empty() {
            match subscriber.recv().await {
                Ok(CommonboxEvent::JobCompleted { job_id: completed_id, .. }) |
                Ok(CommonboxEvent::JobFailed { job_id: completed_id, .. }) |
                Ok(CommonboxEvent::JobStalled { job_id: completed_id, .. }) => {
                    if let Some(dep_str) = config.depends_on.iter()
                        .find(|dep| id_map.get(*dep) == Some(&completed_id)) {
                        pending.remove(dep_str);
                        crate::info_log!(
                            "Worker [{}] dependency {} completed. Remaining: {:?}",
                            config.id, dep_str, pending
                        );
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    crate::error_log!("Worker [{}] job event channel closed", config.id);
                    let _ = commonbox.fail_job(&job_id, "Event channel closed").await;
                    return;
                }
            }
        }
        
        crate::info_log!("Worker [{}] all dependencies satisfied", config.id);
    }
    
    // Mark as running
    if let Err(e) = commonbox.start_job(&job_id).await {
        crate::error_log!("Worker [{}] failed to start: {}", config.id, e);
        return;
    }
    
    // Get input sender and send objective
    let input_tx = session.input_sender();
    
    let objective_msg = format!(
        "Your objective: {}\n\nShared context: {}\n\nBegin working on your task. Remember to use the commonboard tool for coordination.",
        config.objective,
        shared_context.unwrap_or_default()
    );
    
    crate::info_log!("Worker [{}] sending objective message ({} bytes)...", config.id, objective_msg.len());
    if let Err(e) = input_tx.send(UserInput::Message(objective_msg)).await {
        crate::error_log!("Worker [{}] failed to send objective: {}", config.id, e);
        let _ = commonbox.fail_job(&job_id, "Failed to initialize").await;
        return;
    }
    crate::info_log!("Worker [{}] objective message sent successfully", config.id);
    
    // CRITICAL: Use numeric worker_id (e.g., 1000) not config.id (e.g., "log_reader")
    // so TUI can match forwarded events to the correct job
    let worker_id_numeric = worker_id.0.to_string();
    let worker_name = config.id.clone(); // Clone for use in forwarding task
    
    // Spawn output forwarding task with SELECTION BEFORE AMPLIFICATION
    // This filter prevents the 1.2M event flood by:
    // 1. Deduplicating Status events within 100ms window
    // 2. Preserving semantic event types (not flattening everything to Status)
    // 3. Dropping diagnostic/noise events
    // 
    // CRITICAL: mpsc channel buffers events until this forwarder starts
    // No race condition - events sent before forwarder starts are preserved
    // CRITICAL: Clone parent_output_tx for the forwarder task
    // The original is kept for direct WorkerCompleted/Error emission below
    let parent_output_tx_clone = parent_output_tx.clone();
    let forward_handle = tokio::spawn(async move {
        crate::info_log!("[WORKER FORWARD] Starting FILTERED forwarding for worker {} (numeric ID: {})", worker_name, worker_id_numeric);
        
        let mut filter = WorkerEventFilter::new(worker_id);
        let mut total_received = 0u64;
        let mut total_forwarded = 0u64;
        let mut total_dropped = 0u64;
        
        // mpsc recv() returns Option, not Result like broadcast
        while let Some(event) = worker_output_rx.recv().await {
            total_received += 1;
            
            // Log event types we're receiving (for debugging)
            match &event {
                OutputEvent::ToolExecuting { tool, .. } => {
                    crate::info_log!("[WORKER FORWARD] Received ToolExecuting: tool={}", tool);
                }
                OutputEvent::ToolCompleted { .. } => {
                    crate::info_log!("[WORKER FORWARD] Received ToolCompleted");
                }
                _ => {}
            }
            
            // Apply selection filter
            let decision = filter.filter(event);
            
            match decision {
                super::filter::FilterDecision::Forward(event) | super::filter::FilterDecision::Transform(event) => {
                    total_forwarded += 1;
                    
                    if let Some(ref tx) = parent_output_tx_clone {
                        // Transform events to include worker context for job tracking
                        let forwarded_event = match event {
                            // Transform ToolExecuting to WorkerToolExecuting with job_id
                            OutputEvent::ToolExecuting { intent_id: _, tool, args } => {
                                crate::info_log!("[WORKER FORWARD] Transforming ToolExecuting -> WorkerToolExecuting: tool={}", tool);
                                OutputEvent::WorkerToolExecuting {
                                    worker_id,
                                    job_id,
                                    tool,
                                    args,
                                }
                            }
                            // Transform ToolCompleted to WorkerToolCompleted with job_id
                            OutputEvent::ToolCompleted { intent_id: _, result } => {
                                crate::info_log!("[WORKER FORWARD] Transforming ToolCompleted -> WorkerToolCompleted: result_len={}", result.len());
                                OutputEvent::WorkerToolCompleted {
                                    worker_id,
                                    job_id,
                                    result,
                                }
                            }
                            // ResponseChunk: pass through unchanged (worker context is implicit)
                            // Adding prefix here causes spam: "[Worker 1000]" appears throughout text
                            chunk @ OutputEvent::ResponseChunk { .. } => chunk,
                            // Prefix Error with worker ID for context
                            OutputEvent::Error { message } => {
                                OutputEvent::Error {
                                    message: format!("[Worker {}] {}", worker_id_numeric, message),
                                }
                            }
                            // Pass through all other events unchanged (semantics preserved)
                            other => other,
                        };
                        
                        match &forwarded_event {
                            OutputEvent::WorkerToolExecuting { tool, .. } => {
                                crate::info_log!("[WORKER FORWARD] Sending WorkerToolExecuting: tool={}", tool);
                            }
                            OutputEvent::WorkerToolCompleted { .. } => {
                                crate::info_log!("[WORKER FORWARD] Sending WorkerToolCompleted");
                            }
                            _ => {}
                        }
                        
                        if let Err(e) = tx.send(forwarded_event) {
                            crate::error_log!("[WORKER FORWARD] Failed to forward event: {}", e);
                        }
                    }
                }
                super::filter::FilterDecision::Drop(reason) => {
                    total_dropped += 1;
                    // Log dropped events at trace level for debugging
                    crate::trace_log!("[WORKER FORWARD] Dropped event: {}", reason);
                }
            }
            
            // Log stats every 50 received events (not forwarded - this shows filtering working)
            if total_received % 50 == 0 {
                crate::debug_log!(
                    "[WORKER FORWARD] Stats: received={}, forwarded={}, dropped={} ({}% filtered)",
                    total_received, total_forwarded, total_dropped,
                    (total_dropped * 100) / total_received.max(1)
                );
            }
        }
        
        crate::info_log!(
            "[WORKER FORWARD] Filtered forwarding ended: received={}, forwarded={}, dropped={} ({}% filtered)",
            total_received, total_forwarded, total_dropped,
            if total_received > 0 { (total_dropped * 100) / total_received } else { 0 }
        );
    });
    
    // Run session using the Session trait with timeout
    crate::info_log!("Worker [{}] about to call session.run(), session at {:p}, transport instance_id: {}", config.id, &session, session.transport_instance_id());
    commonbox.update_job_status_message(&job_id, "Running cognitive loop...").await;
    
    // DEBUG: Check if session has proper setup
    crate::info_log!("Worker [{}] session input sender obtained, objective sent", config.id);
    
    const DEFAULT_WORKER_TIMEOUT_SECS: u64 = 300; // 5 minutes
    let timeout_duration = Duration::from_secs(
        config.timeout_secs.unwrap_or(DEFAULT_WORKER_TIMEOUT_SECS)
    );
    
    match timeout(timeout_duration, session.run()).await {
        Ok(result) => {
            crate::info_log!("Worker [{}] completed initial task", config.id);
            
            // Transition to Idle state (waiting for routed queries)
            if let Err(e) = commonbox.idle_job(&job_id).await {
                crate::error_log!("Worker [{}] failed to transition to idle: {}", config.id, e);
                let _ = commonbox.fail_job(&job_id, &format!("Idle transition failed: {}", e)).await;
                return;
            }
            
            crate::info_log!("Worker [{}] now idle, waiting for routed queries", config.id);
            
            // Idle loop: process routed queries
            let mut query_count = 0;
            let mut action_count = 0;
            const MAX_ACTIONS: usize = 100; // Stall after 100 actions to prevent runaway
            
            loop {
                // Stall detection: check action limit
                action_count += 1;
                if action_count > MAX_ACTIONS {
                    crate::warn_log!("Worker [{}] hit action limit, stalling", config.id);
                    
                    // Transition to Stalled state
                    if let Err(e) = commonbox.stall_job(&job_id, "Action limit exceeded").await {
                        crate::error_log!("Worker [{}] failed to stall: {}", config.id, e);
                        let _ = commonbox.fail_job(&job_id, &format!("Stall failed: {}", e)).await;
                    }
                    return; // Exit worker task, Main will handle via StallScheduler
                }
                
                // Check for pending queries
                let queries = commonbox.fetch_pending_queries(&job_id).await;
                
                if queries.is_empty() {
                    // No queries - wait a bit and check again
                    // In a real implementation, this would use a notification mechanism
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    
                    // Check if we should terminate (e.g., after some timeout or signal)
                    // For now, terminate after being idle with no queries
                    query_count += 1;
                    if query_count > 50 { // 5 seconds of idle
                        crate::info_log!("Worker [{}] idle timeout, completing", config.id);
                        break;
                    }
                    continue;
                }
                
                // Reset idle counter when we get queries
                query_count = 0;
                
                // Process each query
                for query in queries {
                    crate::info_log!("Worker [{}] processing query: {}", config.id, query.id);
                    
                    // Wake worker to process query
                    if let Err(e) = commonbox.wake_worker(&job_id).await {
                        crate::error_log!("Worker [{}] failed to wake: {}", config.id, e);
                        continue;
                    }
                    
                    // TODO: Proper query routing not yet implemented.
                    // Current behavior: acknowledge receipt but do not execute cognitive processing.
                    // The worker session was consumed during initial objective execution.
                    // Implementing query routing requires either:
                    //   1. Re-entrant session architecture, or
                    //   2. Dedicated per-query session spawn
                    let response = format!(
                        "Query received but not processed (routing not implemented): {}",
                        query.query
                    );
                    
                    // Submit result
                    let _ = commonbox.submit_query_result(
                        &job_id,
                        query.id,
                        crate::agent::runtime::orchestrator::commonbox::JobResult(serde_json::json!(response))
                    ).await;
                    
                    // Transition back to idle
                    if let Err(e) = commonbox.idle_job(&job_id).await {
                        crate::error_log!("Worker [{}] failed to return to idle: {}", config.id, e);
                    }
                }
            }
            
            // Complete the job
            let result_json = serde_json::json!({
                "worker_id": config.id,
                "status": "completed",
                "output": format!("{:?}", result),
            });
            
            if let Err(e) = commonbox.complete_job(&job_id, crate::agent::runtime::orchestrator::commonbox::JobResult(result_json)).await {
                crate::error_log!("Worker [{}] failed to complete job in commonbox: {}", config.id, e);
            } else {
                crate::info_log!("Worker [{}] completed successfully and job marked complete in commonbox", config.id);
            }
            
            // Emit WorkerCompleted event for TUI
            if let Some(ref tx) = parent_output_tx {
                crate::info_log!("[WORKER] Emitting WorkerCompleted event for worker {}", worker_id.0);
                if let Err(e) = tx.send(OutputEvent::WorkerCompleted {
                    worker_id,
                    job_id,
                }) {
                    crate::error_log!("[WORKER] Failed to emit WorkerCompleted: {}", e);
                }
            } else {
                crate::warn_log!("[WORKER] No parent_output_tx to emit WorkerCompleted");
            }
            
            // Graceful shutdown sequence
            crate::info_log!("[WORKER] Initiating graceful shutdown for worker {}", config.id);
            
            // Signal session to stop
            session.interrupt().await;
            
            // Wait for forwarder to complete (with timeout)
            match tokio::time::timeout(tokio::time::Duration::from_secs(5), forward_handle).await {
                Ok(Ok(_)) => {
                    crate::info_log!("[WORKER] Forwarder completed cleanly for worker {}", config.id);
                }
                Ok(Err(e)) => {
                    crate::warn_log!("[WORKER] Forwarder panicked for worker {}: {}", config.id, e);
                }
                Err(_) => {
                    crate::warn_log!("[WORKER] Forwarder timeout for worker {}, abandoning", config.id);
                }
            }
            
            // Drop session to release resources
            drop(session);
            
            crate::info_log!("[WORKER] Worker {} shutdown complete", config.id);
        }
        Ok(Err(e)) => {
            // Determine if this is a stall (recoverable) or fatal error
            let error_str = e.to_string();
            let is_stall = error_str.contains("Action limit exceeded") 
                || error_str.contains("Stall");
            
            // Emit WorkerFailed event for TUI
            if let Some(ref tx) = parent_output_tx {
                let _ = tx.send(OutputEvent::WorkerFailed {
                    worker_id,
                    job_id,
                    error: format!("Worker {} failed: {}", config.id, e),
                    is_stall,
                });
            }
            crate::error_log!("Worker [{}] session error: {}", config.id, e);
            
            let _ = commonbox.fail_job(&job_id, &format!("Session error: {}", e)).await;
        }
        Err(_) => {
            // Timeout - session.run() took longer than timeout_duration
            let error_msg = format!("Worker {} timed out after {}s", config.id, timeout_duration.as_secs());
            crate::error_log!("[WORKER] {}", error_msg);
            
            // Emit WorkerFailed with is_stall=true for potential retry
            if let Some(ref tx) = parent_output_tx {
                let _ = tx.send(OutputEvent::WorkerFailed {
                    worker_id,
                    job_id,
                    error: error_msg.clone(),
                    is_stall: true, // Timeout is potentially recoverable
                });
            }
            
            // Mark job as stalled (not failed) for potential retry
            let _ = commonbox.stall_job(&job_id, &error_msg).await;
            
            // Force terminate session
            drop(session);
        }
    }
}
