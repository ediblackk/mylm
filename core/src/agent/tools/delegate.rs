use crate::agent::tool::{Tool, ToolOutput, ToolKind};
use crate::agent::v2::core::AgentV2;
use crate::agent::v2::jobs::JobRegistry;
use crate::llm::LlmClient;
use crate::memory::{MemoryCategorizer, scribe::Scribe};
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
        }
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
        "Provide objective and optional context: {\"objective\": \"task description\", \"context\": {...}}"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        crate::info_log!("DelegateTool::call execution started: {}", args);

        // Parse arguments
        let delegate_args = if let Ok(parsed) = serde_json::from_str::<DelegateArgs>(args) {
            parsed
        } else {
            // Try to parse as a simple string (objective only)
            DelegateArgs {
                objective: args.trim().trim_matches('"').to_string(),
                context: None,
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
            
            // Create a new AgentV2 instance for the subtask
            let mut sub_agent = AgentV2::new_with_iterations(
                llm_client,
                scribe,
                vec![], // Sub-agent will inherit tools from parent or use minimal set
                format!("You are a specialized worker agent. Your objective is: {}", objective_clone),
                50, // Reasonable iteration limit for sub-agents
                crate::config::AgentVersion::V2,
                memory_store,
                categorizer,
                None, // Sub-agent gets its own JobRegistry for now
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

            // Run the sub-agent event-driven loop
            match sub_agent.run_event_driven(
                history,
                sub_event_tx,
                interrupt_rx,
                approval_rx,
            ).await {
                Ok((result, usage)) => {
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
                Err(e) => {
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
            }
        });

        Ok(ToolOutput::Background {
            job_id,
            description: format!("Delegating task to sub-agent: {}", objective),
        })
    }
}