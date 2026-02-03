use crate::agent::tool::{Tool, ToolOutput, ToolKind};
use crate::agent::v2::core::AgentV2;
use crate::agent::v2::jobs::JobRegistry;
use crate::llm::LlmClient;
use crate::memory::{MemoryCategorizer, scribe::Scribe};
use crate::config::ConfigManager;
use async_trait::async_trait;
use serde::Deserialize;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Deserialize)]
struct DelegateArgs {
    objective: String,
    #[serde(default)]
    context: Option<serde_json::Value>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    max_iterations: Option<usize>,
    #[serde(default)]
    model: Option<String>,
}

/// A tool for delegating tasks to sub-agents (Executor layer).
/// This allows an Orchestrator agent to spawn worker agents for specific subtasks.
pub struct DelegateTool {
    llm_client: Arc<LlmClient>,
    scribe: Arc<Scribe>,
    job_registry: JobRegistry,
    memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    categorizer: Option<Arc<MemoryCategorizer>>,
    event_tx: Option<mpsc::UnboundedSender<crate::agent::event::RuntimeEvent>>,
    config_manager: Option<Arc<ConfigManager>>,
}

impl DelegateTool {
    /// Create a new DelegateTool
    pub fn new(
        llm_client: Arc<LlmClient>,
        scribe: Arc<Scribe>,
        job_registry: JobRegistry,
        memory_store: Option<Arc<crate::memory::store::VectorStore>>,
        categorizer: Option<Arc<MemoryCategorizer>>,
        event_tx: Option<mpsc::UnboundedSender<crate::agent::event::RuntimeEvent>>,
    ) -> Self {
        Self {
            llm_client,
            scribe,
            job_registry,
            memory_store,
            categorizer,
            event_tx,
            config_manager: None,
        }
    }

    /// Set the config manager for worker limit checking
    pub fn with_config_manager(mut self, config_manager: Arc<ConfigManager>) -> Self {
        self.config_manager = Some(config_manager);
        self
    }

    /// Check if we can spawn a new worker based on worker_limit
    async fn check_worker_limit(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if let Some(cm) = &self.config_manager {
            let worker_limit = cm.get_worker_limit().await;
            let active_jobs = self.job_registry.list_active_jobs().len();
            if active_jobs >= worker_limit {
                return Err(format!(
                    "Worker limit exceeded: {}/{}. Cannot spawn new sub-agent.",
                    active_jobs, worker_limit
                ).into());
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        "Delegate a specific task to a sub-agent. Use this to spawn worker agents for parallel execution of subtasks."
    }

    fn usage(&self) -> &str {
        r#"Provide objective and optional parameters:
{
  "objective": "task description",
  "context": {...},
  "system_prompt": "custom system prompt (optional)",
  "tools": ["tool1", "tool2"],
  "max_iterations": 50,
  "model": "model-name"
}"#
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "objective": {
                    "type": "string",
                    "description": "Task to delegate to a sub-agent"
                },
                "context": {
                    "description": "Optional structured context passed to the sub-agent",
                    "nullable": true
                },
                "system_prompt": {
                    "type": "string",
                    "description": "Optional system prompt override for the sub-agent"
                },
                "tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional allowlist of tool names for the sub-agent"
                },
                "max_iterations": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum iterations for the sub-agent"
                },
                "model": {
                    "type": "string",
                    "description": "Optional model name override for the sub-agent"
                }
            },
            "required": ["objective"]
        })
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        crate::info_log!("DelegateTool::call execution started: {}", args);

        // Check worker limit before spawning
        if let Err(e) = self.check_worker_limit().await {
            return Ok(ToolOutput::Immediate(serde_json::json!({
                "error": format!("Delegation failed: {}", e)
            })));
        }

        // Parse arguments
        let delegate_args = if let Ok(parsed) = serde_json::from_str::<DelegateArgs>(args) {
            parsed
        } else {
            // Try to parse as a simple string (objective only)
            DelegateArgs {
                objective: args.trim().trim_matches('"').to_string(),
                context: None,
                system_prompt: None,
                tools: None,
                max_iterations: None::<usize>,
                model: None,
            }
        };

        let objective = delegate_args.objective;
        let context = delegate_args.context.unwrap_or(serde_json::Value::Null);

        crate::info_log!("Delegating task: {} with context: {}", objective, context);

        // Create a job ID for tracking this delegation
        let job_id = self.job_registry.create_job("delegate", &format!("Delegating: {}", objective));
        let job_id_clone = job_id.clone();
        let job_registry = self.job_registry.clone();
        
        // Clone resources for the background task
        let llm_client = self.llm_client.clone();
        let scribe = self.scribe.clone();
        let memory_store = self.memory_store.clone();
        let categorizer = self.categorizer.clone();
        let event_tx = self.event_tx.clone();
        let objective_clone = objective.clone();

        // Spawn the sub-agent in a background task
        tokio::spawn(async move {
            crate::info_log!("Starting background sub-agent for task: {}", objective_clone);
            
            // Prepare sub-agent configuration with custom or default values
            let system_prompt = delegate_args.system_prompt.unwrap_or_else(|| {
                format!("You are a specialized worker agent. Your objective is: {}", objective_clone)
            });
            let max_iterations = delegate_args.max_iterations.unwrap_or(50);
            let requested_model = delegate_args.model;

            // Handle worker model configuration
            let client = if let Some(model_name) = requested_model {
                let mut config = llm_client.config().clone();
                config.model = model_name.clone();
                match LlmClient::new(config) {
                    Ok(c) => Arc::new(c),
                    Err(e) => {
                        crate::error_log!("Failed to create worker client with model {}: {}", model_name, e);
                        // Fallback to parent client
                        llm_client.clone()
                    }
                }
            } else {
                llm_client.clone()
            };

            // TODO: Filter tools based on delegate_args.tools if provided
            // For now, sub-agent starts with empty tool set (inherits context via prompt)
            let _requested_tools = delegate_args.tools;

            // Create a new AgentV2 instance for the subtask
            let mut sub_agent = AgentV2::new_with_iterations(
                client,
                scribe,
                vec![], // Sub-agent will inherit tools from parent or use minimal set
                system_prompt,
                max_iterations,
                crate::config::AgentVersion::V2,
                memory_store,
                categorizer,
                Some(job_registry.clone()), // Share parent's job registry
                None, // capabilities_context
                None, // scratchpad
            );

            // Set up event channels for the sub-agent
            let (sub_event_tx, _sub_event_rx) = mpsc::unbounded_channel();
            let (_interrupt_tx, interrupt_rx) = mpsc::channel(1);
            let (approval_tx, approval_rx) = mpsc::channel(1);
            
            // Auto-approve for sub-agents (they're trusted workers)
            let _ = approval_tx.send(true).await;
            
            // Prepare the task history
            let history = vec![
                crate::llm::chat::ChatMessage::user(format!(
                    "Objective: {}\nContext: {}\n\nPlease complete this task and provide a final answer.",
                    objective_clone,
                    if context.is_null() { "No additional context provided".to_string() } else { context.to_string() }
                ))
            ];

            // Run the sub-agent event-driven loop with panic catching
            // Spawn the sub-agent in a separate task so we can catch panics via JoinHandle
            let sub_task = tokio::spawn(async move {
                sub_agent.run_event_driven(
                    history,
                    sub_event_tx,
                    interrupt_rx,
                    approval_rx,
                ).await
            });

            match sub_task.await {
                Ok(Ok((result, usage))) => {
                    crate::info_log!("Sub-agent completed task: {} with result: {}", objective_clone, result);

                    // Update job with success
                    job_registry.complete_job(
                        &job_id_clone,
                        serde_json::json!({
                            "objective": objective_clone,
                            "result": result,
                            "usage": {
                                "prompt_tokens": usage.prompt_tokens,
                                "completion_tokens": usage.completion_tokens,
                                "total_tokens": usage.total_tokens
                            }
                        })
                    );

                    // Send completion event if we have an event channel
                    if let Some(tx) = &event_tx {
                        let _ = tx.send(crate::agent::event::RuntimeEvent::StatusUpdate {
                            message: format!("✅ Sub-agent completed: {}", objective_clone),
                        });
                    }
                }
                Ok(Err(e)) => {
                    let error_msg = format!("Sub-agent failed: {}", e);
                    crate::error_log!("{}", error_msg);

                    // Update job with failure
                    job_registry.fail_job(&job_id_clone, &error_msg);

                    // Send failure event if we have an event channel
                    if let Some(tx) = &event_tx {
                        let _ = tx.send(crate::agent::event::RuntimeEvent::StatusUpdate {
                            message: format!("❌ Sub-agent failed: {}", error_msg),
                        });
                    }
                }
                Err(e) if e.is_panic() => {
                    // Panic occurred - extract panic message
                    let panic_msg = if let Ok(panic) = e.try_into_panic() {
                        if let Some(s) = panic.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = panic.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "Unknown panic".to_string()
                        }
                    } else {
                        "Unknown panic (could not extract)".to_string()
                    };
                    let error_msg = format!("Sub-agent panic: {}", panic_msg);
                    crate::error_log!("{}", error_msg);

                    // Update job with failure
                    job_registry.fail_job(&job_id_clone, &error_msg);

                    // Send failure event if we have an event channel
                    if let Some(tx) = &event_tx {
                        let _ = tx.send(crate::agent::event::RuntimeEvent::StatusUpdate {
                            message: format!("❌ Sub-agent panic: {}", error_msg),
                        });
                    }
                }
                Err(e) => {
                    // Task was cancelled or other error
                    let error_msg = format!("Sub-agent task error: {}", e);
                    crate::error_log!("{}", error_msg);

                    // Update job with failure
                    job_registry.fail_job(&job_id_clone, &error_msg);

                    // Send failure event if we have an event channel
                    if let Some(tx) = &event_tx {
                        let _ = tx.send(crate::agent::event::RuntimeEvent::StatusUpdate {
                            message: format!("❌ Sub-agent task error: {}", error_msg),
                        });
                    }
                }
            }
        });

        Ok(ToolOutput::Background {
            job_id,
            description: format!("Delegating task to sub-agent: {}", objective),
        })
    }
}
