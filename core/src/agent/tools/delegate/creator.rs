//! Worker Creation - Session instantiation for delegated workers
//!
//! This module handles the CREATION of worker sessions:
//! - Job registration in Commonbox
//! - Session instantiation via Factory
//! - Channel setup (isolated mpsc for worker)
//!
//! NOTE: This is a candidate for moving to session/delegate later.
//! The creation logic was extracted from runner.rs to clarify the boundary
//! between "creating" (this module) and "coordinating" (runner.rs).

use super::types::WorkerConfig;
use crate::agent::runtime::orchestrator::commonbox::{Commonbox, JobId};
use crate::agent::runtime::orchestrator::{AgencySession, OutputEvent};
use crate::agent::cognition::Planner;
use crate::agent::runtime::orchestrator::ContractRuntime;
use crate::agent::runtime::capabilities::InMemoryTransport;
use crate::agent::types::events::WorkerId;

use std::sync::Arc;
use tokio::sync::mpsc;


/// Context needed to create a worker
pub struct WorkerCreationContext {
    pub commonbox: Arc<Commonbox>,
    pub factory: crate::agent::AgentSessionFactory,
    pub worker_index: usize,
}

/// Result of worker creation - contains everything needed to run the worker
pub struct CreatedWorker {
    pub config: WorkerConfig,
    pub job_id: JobId,
    pub worker_id: WorkerId,
    pub session: AgencySession<
        Planner,
        ContractRuntime,
        InMemoryTransport,
    >,
    /// Worker's isolated output channel receiver
    pub worker_output_rx: mpsc::Receiver<OutputEvent>,
}



/// Create a worker session WITHOUT starting it
/// 
/// This function handles:
/// 1. Job registration in Commonbox
/// 2. Session creation via Factory (the actual instantiation)
/// 3. Channel setup (isolated mpsc)
/// 
/// It does NOT:
/// - Start the session (that's coordination/runner's job)
/// - Handle dependencies (that's coordination/runner's job)
/// - Manage the lifecycle (that's coordination/runner's job)
pub async fn create_worker_session(
    config: &WorkerConfig,
    _shared_context: &Option<String>,
    ctx: &WorkerCreationContext,
) -> Result<CreatedWorker, String> {
    // 1. Create job in registry (identity creation)
    let desc = if config.objective.len() > 40 {
        format!("{}: {}...", config.id, &config.objective[..40])
    } else {
        format!("{}: {}", config.id, config.objective)
    };
    
    let agent_id = crate::agent::identity::AgentId::worker(config.id.clone());
    let job_id = ctx.commonbox.create_job(
        agent_id,
        &desc,
    ).await.map_err(|e| e.to_string())?;
    
    let worker_id = WorkerId((ctx.worker_index + 1000) as u64);
    
    crate::info_log!("[CREATOR] Created job {} for worker [{}]", job_id.0, config.id);
    
    // 2. Create isolated channel (mpsc, not broadcast, to prevent race conditions)
    let (worker_output_tx, worker_output_rx) = mpsc::channel::<OutputEvent>(100);
    
    // 3. Build worker session configuration
    let pre_approved_tools = config.tools.clone().unwrap_or_default();
    let allowed_commands = config.allowed_commands.clone().unwrap_or_default();
    let forbidden_commands = config.forbidden_commands.clone().unwrap_or_default();
    
    crate::info_log!(
        "[CREATOR] Worker [{}] command restrictions: allowed={:?}, forbidden={:?}",
        config.id, allowed_commands, forbidden_commands
    );
    
    let worker_config = crate::agent::factory::WorkerSessionConfig {
        allowed_tools: pre_approved_tools,
        allowed_commands,
        forbidden_commands,
        scratchpad: None, // Workers have their own agent-local scratchpad
        output_tx: Some(worker_output_tx),
        objective: config.objective.clone(),
        instructions: config.instructions.clone(),
        tags: Some(config.tags.clone()),
        commonbox: Some(ctx.commonbox.clone()), // Share commonbox for coordination (commonboard tool)
    };
    
    // 4. Create session via Factory (the actual CREATION)
    crate::info_log!("[CREATOR] Creating session for worker [{}] via Factory...", config.id);
    let session = match ctx.factory.create_configured_worker_session(&config.id, worker_config).await {
        Ok(s) => {
            crate::info_log!("[CREATOR] Session created for worker [{}] at {:p}", config.id, &s);
            s
        }
        Err(e) => {
            crate::error_log!("[CREATOR] Failed to create session for worker [{}]: {}", config.id, e);
            let _ = ctx.commonbox.fail_job(&job_id, &format!("Factory error: {}", e)).await;
            return Err(format!("Factory error: {}", e));
        }
    };
    
    crate::info_log!("[CREATOR] Returning CreatedWorker with session at {:p}, transport instance_id: {}", &session, session.transport_instance_id());
    Ok(CreatedWorker {
        config: config.clone(),
        job_id,
        worker_id,
        session,
        worker_output_rx,
    })
}

/// Emit WorkerSpawned event (helper for runner)
pub fn emit_worker_spawned_event(
    output_tx: &Option<crate::agent::runtime::orchestrator::OutputSender>,
    worker_id: WorkerId,
    job_id: crate::agent::runtime::orchestrator::commonbox::JobId,
    objective: String,
    agent_id: String,
) {
    if let Some(ref tx) = output_tx {
        let _ = tx.send(OutputEvent::WorkerSpawned {
            worker_id,
            job_id,
            objective,
            agent_id,
        });
    }
}


