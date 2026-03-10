//! Delegate Tool - Spawn differentiated worker agents for parallel execution
//!
//! This tool enables the main agent to spawn multiple specialized workers
//! that work in parallel with shared scratchpad coordination.

pub mod types;
pub mod filter;
pub mod permissions;
pub mod prompt;
pub mod creator;
pub mod runner;

pub use types::*;
pub use filter::{WorkerEventFilter, FilterDecision};
pub use permissions::build_worker_permissions;
pub use prompt::build_worker_prompt;

use crate::agent::runtime::core::{Capability, ToolCapability};
use crate::agent::runtime::core::RuntimeContext;
use crate::agent::runtime::core::ToolError;
use crate::agent::runtime::orchestrator::commonbox::Commonbox;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use crate::agent::runtime::orchestrator::OutputSender;


use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

use runner::spawn_worker;

/// Delegate tool for spawning worker agents
pub struct DelegateTool {
    /// Commonbox for tracking workers and agent state
    commonbox: Arc<Commonbox>,
    /// Factory for creating worker sessions (single source of truth)
    factory: crate::agent::AgentSessionFactory,
    /// LLM client for workers
    llm_client: Option<Arc<crate::provider::LlmClient>>,
    /// Maximum concurrent workers
    max_workers: usize,
    /// Default max iterations per worker
    default_max_iterations: usize,
    /// Escalation channel for shell commands
    escalation_tx: Option<mpsc::Sender<(crate::agent::tools::worker_shell::EscalationRequest, oneshot::Sender<crate::agent::tools::worker_shell::EscalationResponse>)>>,
    /// Output sender for streaming events (optional)
    /// Uses OutputSender enum to support both broadcast (main) and mpsc (workers)
    output_tx: Option<OutputSender>,
}

impl DelegateTool {
    /// Create a new delegate tool
    pub fn new(
        commonbox: Arc<Commonbox>,
        factory: crate::agent::AgentSessionFactory,
    ) -> Self {
        Self {
            commonbox,
            factory,
            llm_client: None,
            max_workers: 10,
            default_max_iterations: 20,
            escalation_tx: None,
            output_tx: None,
        }
    }
    
    /// Set LLM client for workers
    pub fn with_llm_client(mut self, client: Arc<crate::provider::LlmClient>) -> Self {
        self.llm_client = Some(client);
        self
    }
    
    /// Set maximum workers
    pub fn with_max_workers(mut self, max: usize) -> Self {
        self.max_workers = max;
        self
    }
    
    /// Set default max iterations
    pub fn with_default_iterations(mut self, iterations: usize) -> Self {
        self.default_max_iterations = iterations;
        self
    }
    
    /// Set escalation channel for shell commands
    pub fn with_escalation_channel(
        mut self,
        tx: mpsc::Sender<(crate::agent::tools::worker_shell::EscalationRequest, oneshot::Sender<crate::agent::tools::worker_shell::EscalationResponse>)>,
    ) -> Self {
        self.escalation_tx = Some(tx);
        self
    }
    
    /// Set output sender for streaming events
    pub fn with_output_sender(
        mut self,
        tx: OutputSender,
    ) -> Self {
        self.output_tx = Some(tx);
        self
    }
    
    /// Check if we're within worker limits
    async fn check_worker_limit(&self, count: usize) -> Result<(), String> {
        let active = self.commonbox.active_job_count().await;
        if active + count > self.max_workers {
            return Err(format!(
                "Worker limit exceeded: {} active + {} new > max {}",
                active, count, self.max_workers
            ));
        }
        Ok(())
    }
}

impl Capability for DelegateTool {
    fn name(&self) -> &'static str {
        "delegate"
    }
}

#[async_trait::async_trait]
impl ToolCapability for DelegateTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse arguments with detailed error handling
        let args: DelegateArgs = match serde_json::from_value(call.arguments.clone()) {
            Ok(a) => a,
            Err(e) => {
                let error_msg = format!(
                    r#"Invalid delegate arguments: {}.

Expected format:
{{
  "shared_context": "Optional context for all workers",
  "workers": [
    {{
      "id": "worker1",
      "objective": "Task description",
      "tools": ["read_file", "shell"],
      "allowed_commands": ["ls -la", "cat *", "cargo check"],
      "forbidden_commands": ["rm -rf *", "sudo *"],
      "tags": ["tag1"]
    }}
  ]
}}

Field descriptions:
  - tools: List of tools the worker can use (shell, read_file, etc.)
  - allowed_commands: Shell command patterns worker can execute without approval (* wildcard supported)
  - forbidden_commands: Shell command patterns that are always blocked

Received: {}"#,
                    e, call.arguments
                );
                return Ok(ToolResult::Error {
                    message: error_msg,
                    code: Some("INVALID_ARGS".to_string()),
                    retryable: false,
                });
            }
        };
        
        // Validate workers list
        if args.workers.is_empty() {
            return Ok(ToolResult::Error {
                message: "No workers specified. At least one worker is required.\n\nExample: {\"workers\": [{\"id\": \"worker1\", \"objective\": \"Read and summarize debug.log\"}]}".to_string(),
                code: Some("NO_WORKERS".to_string()),
                retryable: false,
            });
        }
        
        // Check worker limit
        if let Err(e) = self.check_worker_limit(args.workers.len()).await {
            return Ok(ToolResult::Error {
                message: e,
                code: Some("WORKER_LIMIT".to_string()),
                retryable: true,
            });
        }
        
        // Check for duplicate worker IDs
        let mut seen_ids = std::collections::HashSet::new();
        let mut duplicates = Vec::new();
        for worker in &args.workers {
            if !seen_ids.insert(&worker.id) {
                duplicates.push(&worker.id);
            }
        }
        if !duplicates.is_empty() {
            return Ok(ToolResult::Error {
                message: format!(
                    "Duplicate worker IDs detected: {:?}. Each worker must have a unique ID.",
                    duplicates
                ),
                code: Some("DUPLICATE_IDS".to_string()),
                retryable: false,
            });
        }
        
        // Validate dependencies exist
        let valid_ids: std::collections::HashSet<_> = args.workers.iter()
            .map(|w| w.id.clone())
            .collect();
        
        for worker in &args.workers {
            for dep in &worker.depends_on {
                if !valid_ids.contains(dep) {
                    return Ok(ToolResult::Error {
                        message: format!(
                            "Worker '{}' depends on unknown worker '{}'",
                            worker.id, dep
                        ),
                        code: Some("INVALID_DEPENDENCY".to_string()),
                        retryable: false,
                    });
                }
            }
        }
        
        // Shared job ID mapping
        let id_to_job: Arc<RwLock<HashMap<String, crate::agent::runtime::orchestrator::commonbox::JobId>>> = Arc::new(RwLock::new(HashMap::new()));
        
        // Spawn all workers
        let mut spawned = Vec::new();
        let mut errors = Vec::new();
        
        crate::info_log!("[DELEGATE] Spawning {} workers...", args.workers.len());
        
        for (index, config) in args.workers.iter().enumerate() {
            crate::info_log!("[DELEGATE] Spawning worker {}/{}: {}", index + 1, args.workers.len(), config.id);
            
            match spawn_worker(
                config,
                &args.shared_context,
                index,
                id_to_job.clone(),
                self.commonbox.clone(),
                self.factory.clone(),
                self.output_tx.clone(),
            ).await {
                Ok(worker) => {
                    crate::info_log!("[DELEGATE] Worker {} spawned successfully (job_id: {})", config.id, worker.job_id);
                    spawned.push(worker);
                }
                Err(e) => {
                    crate::error_log!("[DELEGATE] Failed to spawn worker '{}': {}", config.id, e);
                    errors.push(format!("Failed to spawn worker '{}': {}", config.id, e));
                }
            }
        }
        
        crate::info_log!("[DELEGATE] Spawned {}/{} workers successfully", spawned.len(), args.workers.len());
        
        // Build result
        let worker_infos: Vec<_> = spawned.iter()
            .map(|w| serde_json::json!({
                "id": w.config.id,
                "job_id": w.job_id.to_string(),
                "objective": w.config.objective,
                "depends_on": w.config.depends_on,
                "tags": w.config.tags,
            }))
            .collect();
        
        let result = serde_json::json!({
            "message": format!("Spawned {} workers", spawned.len()),
            "workers": worker_infos,
            "commonboard_ready": true,
        });
        
        if !errors.is_empty() {
            let mut result_obj = result.as_object().unwrap().clone();
            result_obj.insert("errors".to_string(), serde_json::json!(errors));
            return Ok(ToolResult::Success {
                output: format!("Spawned {} workers with {} errors", spawned.len(), errors.len()),
                structured: Some(serde_json::Value::Object(result_obj)),
            });
        }
        
        Ok(ToolResult::Success {
            output: format!("Successfully spawned {} workers", spawned.len()),
            structured: Some(result),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::ToolRegistry;
    use crate::agent::runtime::orchestrator::commonbox::Commonbox;

    #[tokio::test]
    async fn test_delegate_tool_creation() {
        let commonbox = Arc::new(Commonbox::new());
        let _tools = Arc::new(ToolRegistry::new()); // Kept for test context, not passed to constructor
        let config = crate::config::Config::default();
        let factory = crate::agent::AgentSessionFactory::new(config);
        let tool = DelegateTool::new(commonbox, factory);
        assert_eq!(tool.name(), "delegate");
    }

    #[tokio::test]
    async fn test_worker_config_parsing() {
        let json = r#"{
            "shared_context": "Test context",
            "workers": [
                {
                    "id": "worker1",
                    "objective": "Do something",
                    "tools": ["read_file"],
                    "tags": ["test"],
                    "depends_on": []
                }
            ]
        }"#;
        
        let args: DelegateArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.workers.len(), 1);
        assert_eq!(args.workers[0].id, "worker1");
    }

    #[test]
    fn test_worker_prompt_building() {
        let config = WorkerConfig {
            id: "test-worker".to_string(),
            objective: "Test objective".to_string(),
            instructions: Some("Be careful".to_string()),
            tools: Some(vec!["read_file".to_string()]),
            allowed_commands: None,
            forbidden_commands: None,
            tags: vec!["test".to_string()],
            depends_on: vec![],
            context: None,
            max_iterations: None,
            timeout_secs: None,
        };
        
        let prompt = prompt::build_worker_prompt(&config, &Some("Shared".to_string()));
        assert!(prompt.contains("test-worker"));
        assert!(prompt.contains("Test objective"));
        assert!(prompt.contains("Be careful"));
        assert!(prompt.contains("Coordination Protocol"));
    }
}
