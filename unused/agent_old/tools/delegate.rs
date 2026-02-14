//! Worker Delegation Tool
//!
//! Spawns differentiated worker agents with shared scratchpad coordination.
//! Workers inherit the model from agent settings (not selectable via API).

use crate::agent_old::tool::{Tool, ToolOutput, ToolKind};
use crate::agent_old::v2::core::AgentV2;
use crate::agent_old::v2::jobs::{JobRegistry, AgentType, JobStatus};
use crate::agent_old::prompt::PromptBuilder;
use crate::agent_old::tools::worker_shell::{WorkerShellTool, WorkerShellPermissions, EscalationRequest, EscalationResponse};
use crate::agent_old::event_bus::CoreEvent;
use crate::config::types::{WorkerShellConfig, EscalationMode};
use crate::config::llm::ProviderConfig;
use crate::llm::LlmProvider;
use std::str::FromStr;
use crate::executor::CommandExecutor;
use crate::llm::LlmClient;
use crate::memory::{MemoryCategorizer, scribe::Scribe};
use crate::config::ConfigManager;
use crate::rate_limiter::RateLimiter;
use crate::config::types::AgentPermissions;
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock as TokioRwLock};

/// Shared scratchpad for worker coordination
pub type SharedScratchpad = Arc<TokioRwLock<crate::agent_old::tools::scratchpad::StructuredScratchpad>>;

/// Worker configuration - each worker gets a specific task and tool set
#[derive(Deserialize, Clone, Debug)]
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
}

/// Delegate tool arguments - only supports differentiated workers
#[derive(Deserialize)]
pub struct DelegateArgs {
    /// Shared context for all workers
    #[serde(default)]
    pub shared_context: Option<String>,
    /// Worker configurations (1+ required)
    pub workers: Vec<WorkerConfig>,
}

/// Tool for spawning worker agents
pub struct DelegateTool {
    llm_client: Arc<LlmClient>,
    scribe: Arc<Scribe>,
    job_registry: JobRegistry,
    memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    categorizer: Option<Arc<MemoryCategorizer>>,
    event_bus: Option<Arc<crate::agent_old::event_bus::EventBus>>,
    config_manager: Option<Arc<ConfigManager>>,
    tools: HashMap<String, Arc<dyn Tool>>,
    permissions: Option<AgentPermissions>,
    rate_limiter: Option<Arc<RateLimiter>>,
    max_iterations: usize,
    max_actions_before_stall: usize,
    max_consecutive_messages: u32,
    max_recovery_attempts: u32,
    /// Executor for worker shell commands
    executor: Arc<CommandExecutor>,
    /// Channel for shell command escalation requests
    escalation_tx: Option<mpsc::Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
    /// Maximum tool failures before worker is stalled
    max_tool_failures: usize,
    /// Model to use for workers (from config profile's worker_model)
    worker_model: Option<String>,
    /// Providers configuration for looking up worker model endpoints
    providers: HashMap<String, ProviderConfig>,
}

/// Configuration for creating a DelegateTool
pub struct DelegateToolConfig {
    pub llm_client: Arc<LlmClient>,
    pub scribe: Arc<Scribe>,
    pub job_registry: JobRegistry,
    pub memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    pub categorizer: Option<Arc<MemoryCategorizer>>,
    pub event_bus: Option<Arc<crate::agent_old::event_bus::EventBus>>,
    pub tools: HashMap<String, Arc<dyn Tool>>,
    pub permissions: Option<AgentPermissions>,
    pub max_iterations: usize,
    /// Executor for worker shell commands (required)
    pub executor: Arc<CommandExecutor>,
    /// Maximum tool failures before worker is stalled
    /// (from OrchestratorConfig.max_worker_tool_failures)
    pub max_tool_failures: usize,
    /// Model to use for workers (from config profile's worker_model)
    /// If None, inherits from parent LLM client
    pub worker_model: Option<String>,
    /// Providers configuration for looking up worker model endpoints
    pub providers: HashMap<String, ProviderConfig>,
}

impl DelegateTool {
    pub fn new(config: DelegateToolConfig) -> Self {
        Self {
            llm_client: config.llm_client,
            scribe: config.scribe,
            job_registry: config.job_registry,
            memory_store: config.memory_store,
            categorizer: config.categorizer,
            event_bus: config.event_bus,
            config_manager: None,
            tools: config.tools,
            permissions: config.permissions,
            rate_limiter: None,
            max_iterations: config.max_iterations,
            max_actions_before_stall: 15,
            max_consecutive_messages: 3,
            max_recovery_attempts: 3,
            executor: config.executor,
            escalation_tx: None,
            max_tool_failures: config.max_tool_failures,
            worker_model: config.worker_model,
            providers: config.providers,
        }
    }

    pub fn with_config_manager(mut self, cm: Arc<ConfigManager>) -> Self {
        self.config_manager = Some(cm);
        self
    }

    pub fn with_rate_limiter(mut self, rl: Arc<RateLimiter>) -> Self {
        self.rate_limiter = Some(rl);
        self
    }

    /// Set the escalation channel for shell command approval
    pub fn with_escalation_channel(
        mut self,
        tx: mpsc::Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>,
    ) -> Self {
        self.escalation_tx = Some(tx);
        self
    }

    async fn check_worker_limit(&self, count: usize) -> Result<(), String> {
        if let Some(cm) = &self.config_manager {
            let limit = cm.get_worker_limit().await;
            let active = self.job_registry.list_active_jobs().len();
            if active + count > limit {
                return Err(format!(
                    "Worker limit exceeded: {} active + {} new > limit {}",
                    active, count, limit
                ));
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
        "Spawn differentiated worker agents for parallel execution with shared scratchpad coordination. \
        Each worker must have a unique ID. Workers with duplicate IDs will be rejected. \
        Similar objectives to currently running jobs will trigger warnings but still spawn."
    }

    fn usage(&self) -> &str {
        r#"Spawn workers with specific tasks and tool restrictions:
{
  "shared_context": "Refactoring auth module",
  "workers": [
    {
      "id": "models",
      "objective": "Update User and Session models",
      "tools": ["read_file", "write_file"],
      "allowed_commands": ["cargo check --lib"],
      "tags": ["models"]
    },
    {
      "id": "handlers",
      "objective": "Update login handlers",
      "tools": ["read_file", "write_file"],
      "tags": ["handlers"],
      "depends_on": ["models"]
    }
  ]
}"#
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "shared_context": {
                    "type": "string",
                    "description": "Context shared with all workers"
                },
                "workers": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Unique worker ID" },
                            "objective": { "type": "string", "description": "Worker's specific task" },
                            "instructions": { "type": "string", "description": "Additional instructions" },
                            "tools": { 
                                "type": "array", 
                                "items": { "type": "string" },
                                "description": "Allowed tools (subset of parent's)"
                            },
                            "allowed_commands": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Auto-approved command patterns"
                            },
                            "forbidden_commands": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Forbidden command patterns"
                            },
                            "tags": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Tags for coordination entries"
                            },
                            "depends_on": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Worker IDs to wait for"
                            },
                            "context": {
                                "type": "object",
                                "description": "Worker-specific context"
                            }
                        },
                        "required": ["id", "objective"]
                    }
                }
            },
            "required": ["workers"]
        })
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let args: DelegateArgs = match serde_json::from_str(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolOutput::Immediate(serde_json::json!({
                "error": format!("Invalid arguments: {}", e)
            }))),
        };

        if args.workers.is_empty() {
            return Ok(ToolOutput::Immediate(serde_json::json!({
                "error": "No workers specified"
            })));
        }

        if let Err(e) = self.check_worker_limit(args.workers.len()).await {
            return Ok(ToolOutput::Immediate(serde_json::json!({ "error": e })));
        }

        // Check for duplicate worker IDs within this call
        let mut seen_ids = std::collections::HashSet::new();
        let mut duplicates = Vec::new();
        for worker in &args.workers {
            if !seen_ids.insert(&worker.id) {
                duplicates.push(&worker.id);
            }
        }
        if !duplicates.is_empty() {
            return Ok(ToolOutput::Immediate(serde_json::json!({
                "error": format!(
                    "Duplicate worker IDs detected: {:?}. Each worker must have a unique ID.",
                    duplicates
                )
            })));
        }

        // Warn about potentially redundant workers (same objective, different IDs)
        let active_jobs = self.job_registry.list_active_jobs();
        let mut warnings = Vec::new();
        for worker in &args.workers {
            for job in &active_jobs {
                if job.status == crate::agent_old::v2::jobs::JobStatus::Running {
                    // Check for very similar objectives (case-insensitive contains)
                    let worker_obj = worker.objective.to_lowercase();
                    let job_obj = job.description.to_lowercase();
                    if worker_obj == job_obj || 
                       (worker_obj.len() > 20 && job_obj.contains(&worker_obj)) ||
                       (job_obj.len() > 20 && worker_obj.contains(&job_obj)) {
                        warnings.push(format!(
                            "Worker '{}' has similar objective to running job '{}' ({}). \
                            If these are processing different data, this is fine. \
                            If not, consider waiting for the existing job instead.",
                            worker.id,
                            job.description,
                            &job.id[..8.min(job.id.len())]
                        ));
                    }
                }
            }
        }

        // Create shared scratchpad for coordination
        let scratchpad = Arc::new(TokioRwLock::new(
            crate::agent_old::tools::scratchpad::StructuredScratchpad::new()
        ));

        // Initialize with shared context
        {
            let mut sp = scratchpad.write().await;
            let ctx = args.shared_context.clone().unwrap_or_default();
            let worker_list: Vec<_> = args.workers.iter().map(|w| &w.id).collect();
            sp.append(
                format!("COORDINATION STARTED\nContext: {}\nWorkers: {:?}", ctx, worker_list),
                None,
                vec!["coordination".to_string()],
                true,
            );
        }

        // Spawn all workers
        let mut job_ids = Vec::new();
        let mut worker_infos = Vec::new();

        for config in &args.workers {
            let job_id = self.spawn_worker(config, &args.shared_context, scratchpad.clone()).await;
            job_ids.push(job_id.clone());
            worker_infos.push(serde_json::json!({
                "id": config.id,
                "job_id": job_id,
                "objective": config.objective,
            }));
        }

        let mut result = serde_json::json!({
            "message": format!("Spawned {} workers. Use list_jobs tool to check status.", args.workers.len()),
            "job_ids": job_ids,
            "workers": worker_infos,
        });
        
        // Add warnings if any potentially redundant workers were detected
        if !warnings.is_empty() {
            result["warnings"] = serde_json::Value::Array(
                warnings.into_iter().map(serde_json::Value::String).collect()
            );
        }
        
        Ok(ToolOutput::Immediate(result))
    }
}

impl DelegateTool {
    async fn spawn_worker(
        &self,
        config: &WorkerConfig,
        shared_context: &Option<String>,
        scratchpad: SharedScratchpad,
    ) -> String {
        // Use configured worker_model for this worker
        let worker_model = self.worker_model.clone();
        
        let job_id = self.job_registry.create_job_with_options(
            "delegate",
            &format!("{}: {}", config.id, &config.objective[..config.objective.len().min(40)]),
            true,
            None, // No parent job ID for top-level workers
            worker_model.clone(), // Pass worker_model to job registry
            AgentType::Worker(config.id.clone()),
        );

        let job_id_clone = job_id.clone();
        let job_registry = self.job_registry.clone();
        let llm_client = self.llm_client.clone();
        let scribe = self.scribe.clone();
        let memory_store = self.memory_store.clone();
        let categorizer = self.categorizer.clone();
        let event_bus = self.event_bus.clone();
        let parent_tools = self.tools.clone();
        let parent_permissions = self.permissions.clone();
        let rate_limiter = self.rate_limiter.clone();
        let config_clone = config.clone();
        let shared_ctx = shared_context.clone();

        let cancel_token = job_registry.get_cancellation_token(&job_id_clone);
        let max_iterations = self.max_iterations;
        let max_stall = self.max_actions_before_stall;
        let max_msg = self.max_consecutive_messages;
        let max_recovery = self.max_recovery_attempts;
        let max_tool_failures = self.max_tool_failures;

        // Clone values for the async block
        let escalation_tx = self.escalation_tx.clone();
        let executor = self.executor.clone();
        let providers = self.providers.clone();
        
        tokio::spawn(async move {
            crate::info_log!("Worker [{}] starting: {}", config_clone.id, config_clone.objective);
            job_registry.update_status_message(&job_id_clone, "Initializing worker...");

            // Get worker shell config from agent permissions
            let worker_shell_config = parent_permissions
                .as_ref()
                .and_then(|p| p.worker_shell.as_ref());

            // Build worker tools - replace ShellTool with WorkerShellTool
            let tools = build_worker_tools(
                &parent_tools,
                config_clone.tools.as_ref(),
                executor.clone(),
                config_clone.id.clone(),
                job_id_clone.clone(),
                escalation_tx.clone(),
                worker_shell_config,
            );
            let capabilities = PromptBuilder::format_capabilities_for_tools(&tools);

            // Build permissions
            let permissions = build_permissions(&parent_permissions, &config_clone);

            // Build system prompt
            let system_prompt = build_system_prompt(&config_clone, &capabilities, shared_ctx);

            // Create LLM client (uses worker_model from config if available, with correct provider)
            let client = create_worker_client(&llm_client, &rate_limiter, worker_model.as_ref(), &providers);
            client.set_job_id(Some(job_id_clone.clone()));
            if let Some(token) = cancel_token {
                client.set_cancel_token(token);
            }

            // Create agent
            let agent_config = crate::agent_old::v2::AgentV2Config {
                client,
                scribe,
                tools,
                system_prompt_prefix: system_prompt,
                max_iterations,
                version: crate::config::AgentVersion::V2,
                memory_store,
                categorizer,
                job_registry: Some(job_registry.clone()),
                capabilities_context: Some(capabilities),
                permissions,
                scratchpad: None,
                disable_memory: false,
                event_bus: event_bus.clone(),
                execute_tools_internally: true,
                max_actions_before_stall: max_stall,
                max_consecutive_messages: max_msg,
                max_recovery_attempts: max_recovery,
                max_tool_failures: max_tool_failures,
            };
            let mut agent = AgentV2::new_with_config(agent_config);

            // Setup channels
            let (_tx, interrupt_rx) = mpsc::channel(1);
            let (approval_tx, approval_rx) = mpsc::channel(5);
            for _ in 0..5 {
                let _ = approval_tx.send(true).await;
            }

            // Build task message
            let ctx_str = config_clone.context.as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "None".to_string());
            let task = format!(
                "Objective: {}\nContext: {}\n\nUse scratchpad to coordinate: CLAIM files before work, REPORT progress, SIGNAL when done.",
                config_clone.objective, ctx_str
            );

            let history = vec![crate::llm::chat::ChatMessage::user(task)];
            let bus = event_bus.unwrap_or_else(|| Arc::new(crate::agent_old::event_bus::EventBus::new()));

            job_registry.update_status_message(&job_id_clone, &format!("Running: {}", config_clone.objective));
            
            // Run agent
            let result = agent.run_event_driven(history, bus.clone(), interrupt_rx, approval_rx).await;

            // Handle result
            match result {
                Ok((output, usage)) => {
                    // Check if job was stalled (not actually completed)
                    let job = job_registry.get_job(&job_id_clone);
                    let is_stalled = job.as_ref().map(|j| j.status == JobStatus::Stalled).unwrap_or(false);
                    
                    if is_stalled {
                        // Job is stalled - don't mark as complete, keep stalled status
                        crate::info_log!("Worker [{}] STALLED (not completed)", config_clone.id);
                        
                        let mut sp = scratchpad.write().await;
                        sp.append(
                            format!("STALLED [{}]: {}", config_clone.id, &output[..output.len().min(200)]),
                            None,
                            config_clone.tags.clone(),
                            true,
                        );
                        drop(sp);
                        
                        // Publish WorkerStalled event so UI updates immediately
                        bus.publish(CoreEvent::WorkerStalled {
                            job_id: job_id_clone.clone(),
                            reason: output.clone(),
                        });
                        
                        bus.publish(CoreEvent::StatusUpdate {
                            message: format!("⚠️ Worker [{}] stalled - needs intervention", config_clone.id),
                        });
                    } else {
                        // Job actually completed successfully
                        crate::info_log!("Worker [{}] completed", config_clone.id);
                        job_registry.update_status_message(&job_id_clone, "Completed successfully");

                        // Write to shared scratchpad and capture coordination log
                        let mut sp = scratchpad.write().await;
                        sp.append(
                            format!("COMPLETE [{}]: {}", config_clone.id, &output[..output.len().min(200)]),
                            None,
                            config_clone.tags.clone(),
                            true,
                        );
                        let scratchpad_content = sp.to_string();
                        drop(sp);

                         job_registry.complete_job(
                             &job_id_clone,
                             serde_json::json!({
                                 "worker_id": config_clone.id,
                                 "result": output,
                                 "scratchpad": scratchpad_content,
                                 "usage": {
                                     "prompt_tokens": usage.prompt_tokens,
                                     "completion_tokens": usage.completion_tokens,
                                     "total_tokens": usage.total_tokens
                                 }
                             }),
                         );

                         bus.publish(CoreEvent::StatusUpdate {
                             message: format!("✅ Worker [{}] completed", config_clone.id),
                         });
                    }
                }
                Err(e) => {
                    let msg = format!("Worker [{}] failed: {}", config_clone.id, e);
                    crate::error_log!("{}", msg);
                    job_registry.update_status_message(&job_id_clone, &format!("Failed: {}", e));

                    let mut sp = scratchpad.write().await;
                    sp.append(
                        format!("FAILED [{}]: {}", config_clone.id, &msg[..msg.len().min(200)]),
                        None,
                        vec!["error".to_string()],
                        true,
                    );
                    let _scratchpad_content = sp.to_string();
                    drop(sp);

                     job_registry.fail_job(&job_id_clone, &msg);

                     bus.publish(CoreEvent::StatusUpdate {
                         message: format!("❌ {}", msg),
                     });
                }
            }
        });

        job_id
    }
}

/// Filter tools based on allowed list
/// Build tools for workers, replacing ShellTool with WorkerShellTool
fn build_worker_tools(
    parent_tools: &HashMap<String, Arc<dyn Tool>>,
    allowed: Option<&Vec<String>>,
    executor: Arc<CommandExecutor>,
    worker_id: String,
    job_id: String,
    escalation_tx: Option<mpsc::Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
    worker_shell_config: Option<&WorkerShellConfig>,
) -> Vec<Arc<dyn Tool>> {
    // Get the list of tool names to include
    let tool_names: Vec<&str> = match allowed {
        Some(list) if !list.is_empty() => list.iter().map(|s| s.as_str()).collect(),
        _ => parent_tools.keys().map(|s| s.as_str()).collect(),
    };
    
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
     
    for name in tool_names {
        if name == "execute_command" {
            // Build permissions with config or defaults
            let permissions = WorkerShellPermissions {
                allowed_patterns: worker_shell_config
                    .and_then(|c| c.allowed_patterns.clone())
                    .unwrap_or_else(WorkerShellConfig::default_allowed),
                restricted_patterns: worker_shell_config
                    .and_then(|c| c.restricted_patterns.clone())
                    .unwrap_or_else(WorkerShellConfig::default_restricted),
                forbidden_patterns: worker_shell_config
                    .and_then(|c| c.forbidden_patterns.clone())
                    .unwrap_or_default(),
                escalation_mode: worker_shell_config
                    .and_then(|c| c.escalation_mode.clone())
                    .unwrap_or(EscalationMode::EscalateToMain),
            };

            // Replace ShellTool with WorkerShellTool
            let worker_shell = WorkerShellTool::new(
                executor.clone(),
                permissions,
                worker_id.clone(),
                job_id.clone(),
                escalation_tx.clone(),
            );
            tools.push(Arc::new(worker_shell));
        } else if let Some(tool) = parent_tools.get(name) {
            tools.push(tool.clone());
        }
    }
    
    tools
}

/// Build worker permissions
fn build_permissions(
    parent: &Option<AgentPermissions>,
    config: &WorkerConfig,
) -> Option<AgentPermissions> {
    if config.allowed_commands.is_some() || config.forbidden_commands.is_some() {
        Some(AgentPermissions {
            allowed_tools: None,
            auto_approve_commands: config.allowed_commands.clone(),
            forbidden_commands: config.forbidden_commands.clone(),
            worker_shell: None,
        })
    } else {
        parent.clone()
    }
}

/// Create worker LLM client (uses worker_model from config if provided, otherwise inherits from parent)
/// Worker model can be in format "provider/model" to use a different provider than the main agent
fn create_worker_client(
    parent: &Arc<LlmClient>,
    rate_limiter: &Option<Arc<RateLimiter>>,
    worker_model: Option<&String>,
    providers: &HashMap<String, ProviderConfig>,
) -> Arc<LlmClient> {
    let mut config = parent.config().clone();
    
    // Override with worker_model if specified in config
    if let Some(model_str) = worker_model {
        // Check if worker_model is in "provider/model" format
        if let Some((provider_name, model_name)) = model_str.split_once('/') {
            // Try to look up the provider configuration
            if let Some(provider_cfg) = providers.get(provider_name) {
                // Use the provider's base_url and api_key
                config.base_url = provider_cfg.base_url.clone();
                config.api_key = provider_cfg.api_key.clone();
                config.provider = LlmProvider::from_str(&format!("{:?}", provider_cfg.provider_type)).unwrap_or(LlmProvider::OpenAiCompatible);
                config.model = model_name.to_string();
                crate::info_log!(
                    "Worker using provider '{}' with model '{}' (base_url: {})",
                    provider_name, model_name, config.base_url
                );
            } else {
                // Provider not found, use as-is (might be a model with '/' in name)
                config.model = model_str.clone();
                crate::warn_log!(
                    "Worker provider '{}' not found, using model name as-is: {}",
                    provider_name, model_str
                );
            }
        } else {
            // No '/' in model name, use same provider as main agent
            config.model = model_str.clone();
            crate::info_log!("Worker using configured worker_model: {} (same provider as main)", model_str);
        }
    }
    
    let client = LlmClient::new(config)
        .expect("Failed to create worker client")
        .set_worker(true);

    let client = match rate_limiter {
        Some(rl) => client.with_rate_limiter(rl.clone()),
        None => client,
    };

    Arc::new(client)
}

/// Build worker system prompt
fn build_system_prompt(
    config: &WorkerConfig,
    capabilities: &str,
    shared_context: Option<String>,
) -> String {
    let instructions = config.instructions.as_ref()
        .map(|i| format!("\n## Additional Instructions\n{}\n", i))
        .unwrap_or_default();

    let coord = format!(r#"

## Coordination Protocol

You share a scratchpad with other workers. REQUIRED actions:

1. **CLAIM** before working: `{{"a":"scratchpad","i":{{"action":"append","text":"CLAIM: <file>","tags":{:?}}}}}`
2. **REPORT** after milestones: `{{"a":"scratchpad","i":{{"action":"append","text":"PROGRESS: <what you did>","tags":{:?}}}}}`
3. **COMPLETE** when done: `{{"a":"scratchpad","i":{{"action":"append","text":"COMPLETE: <summary>","tags":{:?}}}}}`

Check scratchpad before working: `{{"a":"scratchpad","i":{{"action":"list"}}}}`

Respect CLAIMs from other workers.
"#, config.tags, config.tags, config.tags);

    let shared = shared_context
        .map(|c| format!("\n## Shared Context\n{}\n", c))
        .unwrap_or_default();

    format!(
        r#"You are Worker [{}] - a specialized sub-agent.

## Your Assignment
{}{}{}

## Critical Rules
1. Use tools to complete tasks - don't just describe
2. Respond with Short-Key JSON: `{{"t":"thought","a":"tool","i":{{args}}}}`
3. When complete: `{{"f":"result"}}`
4. No clarifying questions - just execute
{}

## Available Tools
{}
"#,
        config.id,
        config.objective,
        shared,
        instructions,
        coord,
        capabilities
    )
}
