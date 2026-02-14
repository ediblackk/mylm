//! Agent V2 Core Implementation

use crate::llm::{LlmClient, chat::{ChatMessage, MessageRole}, TokenUsage};
use crate::agent_old::tool::{Tool, ToolKind};
use crate::agent_old::toolcall_log;
use crate::agent_old::v2::jobs::{JobRegistry, ActionType};
use crate::agent_old::event_bus::EventBus;
use crate::agent_old::v2::recovery::{RecoveryWorker, RecoveryContext};
use crate::agent_old::v2::protocol::{AgentDecision, AgentRequest, parse_short_key_actions_from_content};
use crate::agent_old::prompt::PromptBuilder;
use crate::agent_old::v2::execution::execute_parallel_tools;
use crate::agent_old::v2::memory::MemoryManager;
use crate::memory::{MemoryCategorizer, scribe::Scribe};
use crate::context::ContextManager;
use crate::config::types::AgentPermissions;
use crate::agent_old::tools::StructuredScratchpad;

use std::error::Error as StdError;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use regex::Regex;
use crate::agent_old::tools::ScratchpadTool;
use crate::agent_old::tools::ConsolidateTool;

// Type alias for the scratchpad tool storage
pub type ScratchpadToolRef = Arc<ScratchpadTool>;

/// Configuration for creating a new AgentV2
#[derive(Clone)]
pub struct AgentV2Config {
    pub client: Arc<LlmClient>,
    pub scribe: Arc<Scribe>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub system_prompt_prefix: String,
    pub max_iterations: usize,
    pub version: crate::config::AgentVersion,
    pub memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    pub categorizer: Option<Arc<MemoryCategorizer>>,
    pub job_registry: Option<JobRegistry>,
    pub capabilities_context: Option<String>,
    pub permissions: Option<AgentPermissions>,
    pub scratchpad: Option<Arc<RwLock<StructuredScratchpad>>>, // Note: tokio::sync::RwLock
    pub disable_memory: bool,
    pub event_bus: Option<Arc<EventBus>>,
    /// When true, tools are executed internally by V2 (parallel mode).
    /// When false, V2 returns AgentDecision::Action and lets the orchestrator execute.
    pub execute_tools_internally: bool,
    /// Maximum actions before worker is considered stalled
    pub max_actions_before_stall: usize,
    /// Maximum consecutive messages without tool use
    pub max_consecutive_messages: u32,
    /// Maximum recovery attempts after errors
    pub max_recovery_attempts: u32,
    /// Maximum tool failures before worker is stalled
    pub max_tool_failures: usize,
}

/// The core AgentV2 that manages the agentic loop.
pub struct AgentV2 {
    pub llm_client: Arc<LlmClient>,
    pub scribe: Arc<Scribe>,
    pub tools: HashMap<String, Arc<dyn Tool>>,
    pub job_registry: Arc<JobRegistry>,
    pub max_iterations: usize,
    pub system_prompt_prefix: String,
    pub memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    pub categorizer: Option<Arc<MemoryCategorizer>>,
    pub session_id: String,
    pub version: crate::config::AgentVersion,
    pub event_bus: Option<Arc<EventBus>>,

    // State maintained between steps (managed by LifecycleManager)
    pub history: Vec<ChatMessage>,
    pub iteration_count: usize,
    pub total_usage: TokenUsage,
    pub pending_decision: Option<AgentDecision>,
    /// Queue of pending actions for batch processing (when not executing internally)
    pub pending_action_queue: Vec<AgentDecision>,

    // Safety tracking
    last_tool_call: Option<(String, String)>,
    repetition_count: usize,
    pending_tool_call_id: Option<String>,

    pub recovery_worker: RecoveryWorker,
    parse_failure_count: usize,

    // Budget and timeout controls
    pub budget: usize,
    pub max_steps: usize,
    pub heartbeat_interval: std::time::Duration,
    pub safety_timeout: std::time::Duration,
    
    // Agent behavior configuration (from config file)
    pub max_actions_before_stall: usize,
    pub max_consecutive_messages: u32,
    pub max_recovery_attempts: u32,
    /// Maximum tool failures before worker is stalled
    pub max_tool_failures: usize,
    /// Current consecutive tool failure count (runtime state)
    pub consecutive_tool_failures: usize,

    /// Optional capabilities context to inject into system prompt
    pub capabilities_context: Option<String>,

    /// Optional permission controls for this agent
    pub permissions: Option<AgentPermissions>,

    /// Scratchpad for short-term working memory
    pub scratchpad: Arc<RwLock<StructuredScratchpad>>, // Note: tokio::sync::RwLock
    
    /// Scratchpad tool for accessing ephemeral items
    scratchpad_tool: Option<ScratchpadToolRef>,

    /// Context manager for token counting, pruning, and condensation
    pub context_manager: ContextManager,
    /// Disable memory recall and hot memory injection (incognito mode)
    pub disable_memory: bool,
    /// When true, tools are executed internally by V2 (parallel mode).
    /// When false, V2 returns AgentDecision::Action and lets the orchestrator execute.
    pub execute_tools_internally: bool,

    // Helper components
    memory_manager: MemoryManager,
    prompt_builder: PromptBuilder,
}

impl AgentV2 {
    /// Create a new AgentV2 from a configuration struct
    pub fn new_with_config(config: AgentV2Config) -> Self {
        let mut tool_map = HashMap::new();
        for tool in config.tools {
            tool_map.insert(tool.name().to_string(), tool);
        }
        // Only log total count to reduce verbosity
        crate::info_log!("AgentV2 initialized with {} tools", tool_map.len());

        let scratchpad = config.scratchpad.unwrap_or_else(|| Arc::new(RwLock::new(StructuredScratchpad::new())));
        let scratchpad_tool = Arc::new(ScratchpadTool::new(scratchpad.clone()));
        let scratchpad_tool_for_storage = scratchpad_tool.clone();
        tool_map.insert(scratchpad_tool.name().to_string(), scratchpad_tool);

        if let Some(store) = &config.memory_store {
            let consolidate_tool = Arc::new(ConsolidateTool::new(scratchpad.clone(), store.clone()));
            tool_map.insert(consolidate_tool.name().to_string(), consolidate_tool);
        }

        let session_id = chrono::Utc::now().timestamp_millis().to_string();

        // Pass job_registry to LLM client so metrics can be tracked
        let job_registry = Arc::new(config.job_registry.unwrap_or_default());
        config.client.set_job_registry(job_registry.clone());

        // Initialize context manager from LLM client config
        let context_manager = ContextManager::from_llm_client(&config.client);

        // Create helper components
        let memory_manager = MemoryManager::new(
            config.scribe.clone(),
            config.memory_store.clone(),
            config.categorizer.clone(),
            config.disable_memory,
        );
        let prompt_builder = PromptBuilder::new(
            config.system_prompt_prefix.clone(),
            tool_map.clone(),
            config.capabilities_context.clone(),
        );

        Self {
            llm_client: config.client.clone(),
            scribe: config.scribe,
            tools: tool_map,
            job_registry,
            max_iterations: config.max_iterations,
            system_prompt_prefix: config.system_prompt_prefix,
            categorizer: config.categorizer,
            memory_store: config.memory_store,
            session_id,
            history: Vec::new(),
            iteration_count: 0,
            total_usage: TokenUsage::default(),
            pending_decision: None,
            pending_action_queue: Vec::new(),
            last_tool_call: None,
            repetition_count: 0,
            pending_tool_call_id: None,
            version: config.version,
            event_bus: config.event_bus,
            recovery_worker: RecoveryWorker::new(config.client.clone()),
            parse_failure_count: 0,
            budget: config.max_iterations,
            max_steps: config.max_iterations,
            heartbeat_interval: std::time::Duration::from_secs(5),
            safety_timeout: std::time::Duration::from_secs(300),
            capabilities_context: config.capabilities_context,
            permissions: config.permissions,
            scratchpad,
            scratchpad_tool: Some(scratchpad_tool_for_storage),
            context_manager,
            disable_memory: config.disable_memory,
            execute_tools_internally: config.execute_tools_internally,
            memory_manager,
            prompt_builder,
            max_actions_before_stall: config.max_actions_before_stall,
            max_consecutive_messages: config.max_consecutive_messages,
            max_recovery_attempts: config.max_recovery_attempts,
            max_tool_failures: config.max_tool_failures,
            consecutive_tool_failures: 0,
        }
    }

    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Set the budget (maximum steps) for autonomous execution.
    pub fn with_budget(mut self, budget: usize) -> Self {
        self.budget = budget;
        self.max_steps = budget;
        self
    }

    /// Set the heartbeat interval for polling background jobs.
    pub fn with_heartbeat_interval(mut self, interval: std::time::Duration) -> Self {
        self.heartbeat_interval = interval;
        self
    }

    /// Set the safety timeout for autonomous execution.
    pub fn with_safety_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.safety_timeout = timeout;
        self
    }

    /// Increase the step budget by a specified amount.
    pub fn increase_budget(&mut self, additional_steps: usize) {
        self.max_steps += additional_steps;
    }

    /// Get current budget usage statistics.
    pub fn budget_stats(&self) -> (usize, usize, usize) {
        (self.iteration_count, self.max_steps, self.budget)
    }

    /// Check if the agent has a pending decision to be returned.
    pub fn has_pending_decision(&self) -> bool {
        self.pending_decision.is_some()
    }

    /// Get the full scratchpad content including ephemeral items.
    async fn get_scratchpad_content(&self) -> String {
        if let Some(tool) = &self.scratchpad_tool {
            tool.get_full_content().await
        } else {
            // Use try_read to avoid blocking
            match self.scratchpad.try_read() {
                Ok(guard) => guard.to_string(),
                Err(_) => {
                    crate::error_log!("Scratchpad lock busy in get_scratchpad_content, returning empty string");
                    String::new()
                }
            }
        }
    }

    /// Reset the agent's state for a new task.
    pub async fn reset(&mut self, history: Vec<ChatMessage>) {
        // Debug: Log each message in the incoming history
        crate::info_log!("AgentV2::reset() called with {} history messages", history.len());
        for (i, msg) in history.iter().enumerate() {
            let preview: String = msg.content.chars().take(100).collect();
            crate::info_log!("  History[{}]: role={:?}, content_len={} chars, content_preview={:?}", 
                i, msg.role, msg.content.len(), preview);
        }
        
        self.history = history;
        self.iteration_count = 0;
        self.total_usage = TokenUsage::default();
        self.pending_decision = None;
        self.pending_action_queue.clear();
        self.last_tool_call = None;
        self.repetition_count = 0;
        self.parse_failure_count = 0;
        self.pending_tool_call_id = None;
        self.max_steps = self.budget;
        self.consecutive_tool_failures = 0;

        // Ensure system prompt is present with capability awareness
        if self.history.is_empty() || self.history[0].role != MessageRole::System {
            let scratchpad_content = self.get_scratchpad_content().await;
            let system_prompt = self.prompt_builder.build(&scratchpad_content);
            crate::info_log!("AgentV2 system prompt size: {} chars (scratchpad: {} chars)", system_prompt.len(), scratchpad_content.len());
            // Disabled: too verbose - crate::info_log!("AgentV2 system prompt preview: {}...", &system_prompt[..500.min(system_prompt.len())]);
            self.history.insert(0, ChatMessage::system(system_prompt));
        }
        
        // Simplified: removed reset complete log
    }

    /// Get the current system prompt for debugging purposes.
    pub async fn get_system_prompt(&self) -> String {
        let scratchpad_content = self.get_scratchpad_content().await;
        self.prompt_builder.get_full_prompt(&scratchpad_content)
    }

    /// Get the tools description for debugging.
    pub fn get_tools_description(&self) -> String {
        self.prompt_builder.get_tools_description()
    }

    /// Inject hot memory (recent journal entries) into the conversation context.
    pub async fn inject_hot_memory(&mut self, limit: usize) {
        self.memory_manager.inject_hot_memory(&mut self.history, limit).await;
    }

    /// Reset the agent and immediately inject hot memory context.
    pub async fn reset_with_memory(&mut self, history: Vec<ChatMessage>, limit: usize) {
        self.reset(history).await;
        if !self.disable_memory {
            self.inject_hot_memory(limit).await;
        }
    }
    
    /// Reset iteration counter for a fresh "turn" in chat mode.
    /// This allows the agent to continue chatting without hitting iteration limits.
    pub fn reset_iteration_counter(&mut self) {
        self.iteration_count = 0;
    }
    
    /// Set a new iteration limit dynamically (for extending budget in chat sessions)
    pub fn set_iteration_limit(&mut self, limit: usize) {
        self.max_iterations = limit;
        self.max_steps = limit;
    }

    /// Perform a single step in the agentic loop.
    pub async fn step(&mut self, observation: Option<String>) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
        // Check scratchpad size and inject warning if needed
        let scratchpad_size = self.scratchpad.read().await.get_size();
        if scratchpad_size > crate::agent_old::tools::scratchpad::SCRATCHPAD_WARNING_SIZE {
            let warning = format!(
                "⚠️ SCRATCHPAD SIZE: {}/{} chars (limit: {}).\n\
                 Action required: Use `consolidate_memory` tool to save important facts to long-term memory,\n\
                 then `scratchpad` with action 'clear' to reset.", 
                scratchpad_size,
                crate::agent_old::tools::scratchpad::SCRATCHPAD_WARNING_SIZE,
                crate::agent_old::tools::scratchpad::SCRATCHPAD_CRITICAL_SIZE
            );

            // Avoid spamming if the last message is already the warning
            let last_msg_is_warning = self.history.last()
                .map(|m| m.content.contains("SCRATCHPAD SIZE"))
                .unwrap_or(false);

            if !last_msg_is_warning {
                self.history.push(ChatMessage::system(warning));
            }
        }
        
        // Critical size - force mention
        if scratchpad_size > crate::agent_old::tools::scratchpad::SCRATCHPAD_CRITICAL_SIZE {
            let critical_warning = format!(
                "🚨 CRITICAL: Scratchpad is overfull ({}/{}). Context condensation imminent!\n\
                 IMMEDIATE ACTION: Use consolidate_memory NOW or you will lose information.",
                scratchpad_size,
                crate::agent_old::tools::scratchpad::SCRATCHPAD_CRITICAL_SIZE
            );
            self.history.push(ChatMessage::system(critical_warning));
        }

        // 1. Hard Iteration Limit Check
        if self.iteration_count >= self.max_iterations {
            self.pending_decision = None;
            self.pending_tool_call_id = None;

            return Ok(AgentDecision::Message(
                format!("⚠️ Maximum iteration limit ({}) reached.", self.max_iterations),
                self.total_usage.clone(),
            ));
        }

        // 2. Return pending decision if we have one
        if let Some(decision) = self.pending_decision.take() {
            return Ok(decision);
        }

        // 2b. Check action queue for batch processing (non-internal execution mode)
        if !self.pending_action_queue.is_empty() {
            let next_action = self.pending_action_queue.remove(0);
            return Ok(next_action);
        }

        if let Some(obs) = observation {
            // DISABLED: Scribe redesign pending - see scribe.rs header
            // Log the observation
            // if let Err(e) = self.scribe.observe(crate::memory::journal::InteractionType::Output, &obs).await {
            //     crate::error_log!("Failed to log observation to memory: {}", e);
            //     if let Some(event_bus) = &self.event_bus {
            //         event_bus.publish(crate::agent::event_bus::CoreEvent::StatusUpdate { message: format!("Memory logging error: {}", e) });
            //     }
            // }

            // Track tool failures from observation (for external execution mode)
            // Detect if this is an error observation
            let is_error_observation = obs.starts_with("Error:") ||
                obs.contains("Tool Error:") ||
                obs.contains("failed:") ||
                obs.contains("Error executing command:");
            
            if is_error_observation {
                self.consecutive_tool_failures += 1;
                crate::info_log!(
                    "Tool failure detected from observation. Consecutive failures: {}/{}",
                    self.consecutive_tool_failures,
                    self.max_tool_failures
                );
                
                // Check if we've exceeded the max tool failures limit
                if self.consecutive_tool_failures >= self.max_tool_failures {
                    crate::error_log!(
                        "Worker stalled: {} consecutive tool failures (max: {})",
                        self.consecutive_tool_failures,
                        self.max_tool_failures
                    );
                    return Ok(AgentDecision::Stall {
                        reason: format!(
                            "Worker stalled after {} consecutive tool failures",
                            self.consecutive_tool_failures
                        ),
                        tool_failures: self.consecutive_tool_failures,
                    });
                }
            } else if !obs.starts_with("--- TERMINAL CONTEXT ---") && !obs.contains("Worker") && !obs.contains("completed") {
                // Reset counter on success (but not for status messages or worker completions)
                // Only reset if this looks like an actual tool result
                if self.last_tool_call.is_some() {
                    self.consecutive_tool_failures = 0;
                }
            }

            // Truncate observation to prevent context explosion
            const MAX_OBS_CHARS: usize = 8000;
            let obs_len = obs.len();
            let truncated_obs = if obs_len > MAX_OBS_CHARS {
                crate::warn_log!("Observation too large ({} chars), truncating to {}", obs_len, MAX_OBS_CHARS);
                format!("{}...[truncated {} chars]", &obs[..MAX_OBS_CHARS], obs_len - MAX_OBS_CHARS)
            } else {
                obs
            };
            if let Some(tool_id) = self.pending_tool_call_id.take() {
                let tool_name = self.last_tool_call.as_ref().map(|(n, _)| n.clone()).unwrap_or_else(|| "unknown".to_string());
                self.history.push(ChatMessage::tool(tool_id, tool_name, truncated_obs));
            } else {
                self.history.push(ChatMessage::user(format!("Observation: {}", truncated_obs)));
            }
        }

        // --- Context Management (Pruning & Condensation) ---
        self.context_manager.set_history(&self.history);

        // Pre-flight check for warnings
        if let Some(warning) = self.context_manager.preflight_check(None) {
            crate::info_log!("Context warning: {}", warning);
            if let Some(event_bus) = &self.event_bus {
                event_bus.publish(crate::agent_old::event_bus::CoreEvent::StatusUpdate {
                    message: format!("⚠️ {}", warning)
                });
            }
        }

        // Prepare context (condense if needed, then prune)
        let before_tokens = self.context_manager.total_tokens();
        match self.context_manager.prepare_context(Some(&self.llm_client)).await {
            Ok(optimized_history) => {
                let after_tokens = optimized_history.iter()
                    .map(|m| crate::context::TokenCounter::estimate(&m.content))
                    .sum();
                
                // Add condensation stamp if tokens were reduced significantly
                if after_tokens < before_tokens.saturating_sub(100) {
                    self.context_manager.add_stamp(
                        crate::context::stamps::context_condensed(before_tokens, after_tokens)
                    );
                }
                
                self.history = optimized_history;
            }
            Err(e) => {
                crate::info_log!("Context preparation failed: {}. Using original history.", e);
            }
        }

        // --- Memory Recall ---
        let mut request_history = self.history.clone();
        if !self.disable_memory {
            let query = self.history.iter().rev()
                .find(|m| m.role == MessageRole::User)
                .map(|m| m.content.as_str())
                .unwrap_or("");

            if let Ok(recalled_context) = self.scribe.recall(query, 5).await {
                if !recalled_context.is_empty() {
                    request_history.push(ChatMessage::system(format!(
                        "## Recalled Context (Long-term & Recent Memory)\n{}\nUse this context to inform your decisions.",
                        recalled_context
                    )));
                }
            }
        }

        let mut request = crate::llm::chat::ChatRequest::new(self.llm_client.model().to_string(), request_history);

        // Provide tool definitions for Modern API fallback
        let mut chat_tools = Vec::new();
        for tool in self.tools.values() {
            chat_tools.push(crate::llm::chat::ChatTool {
                type_: "function".to_string(),
                function: crate::llm::chat::ChatFunction {
                    name: tool.name().to_string(),
                    description: Some(tool.description().to_string()),
                    parameters: Some(tool.parameters()),
                },
            });
        }
        if !chat_tools.is_empty() {
            request = request.with_tools(chat_tools);
        }

        let response = self.llm_client.chat(&request).await?;
        let content = response.content();
        
        let _step_profiler = std::time::Instant::now();
        // Disabled: DEBUG_PERF logging - crate::info_log!("DEBUG_PERF: LLM response received. Starting post-processing.");

        // Structured debug log (JSONL)
        let tool_calls_json = response.choices
            .first()
            .and_then(|c| c.message.tool_calls.as_ref())
            .map(|tcs| serde_json::to_value(tcs).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null);
            
        let _t0 = std::time::Instant::now();
        toolcall_log::append_jsonl_owned(serde_json::json!({
            "kind": "llm_response",
            "session_id": self.session_id,
            "iteration": self.iteration_count,
            "provider": format!("{}", self.llm_client.provider()),
            "model": self.llm_client.model(),
            "finish_reason": response.choices.first().and_then(|c| c.finish_reason.clone()),
            "content": truncate_for_log(&content, 4000),
            "tool_calls": tool_calls_json,
        }));
        // Disabled: DEBUG_PERF logging - crate::info_log!("DEBUG_PERF: toolcall_log took {:?}", t0.elapsed());

        // DISABLED: Scribe redesign pending - see scribe.rs header
        // Log the thought/response
        let _t1 = std::time::Instant::now();
        // if let Err(e) = self.scribe.observe(crate::memory::journal::InteractionType::Thought, &content).await {
        //     crate::error_log!("Failed to log thought to memory: {}", e);
        //     if let Some(event_bus) = &self.event_bus {
        //         event_bus.publish(crate::agent::event_bus::CoreEvent::StatusUpdate { message: format!("Memory logging error: {}", e) });
        //     }
        // }
        // Disabled: scribe DEBUG_PERF logging - crate::info_log!("DEBUG_PERF: scribe.observe took {:?}", t1.elapsed());

        if let Some(usage) = &response.usage {
            self.total_usage.prompt_tokens += usage.prompt_tokens;
            self.total_usage.completion_tokens += usage.completion_tokens;
            self.total_usage.total_tokens += usage.total_tokens;
        }

        self.iteration_count += 1;

        // --- Periodic Scratchpad Management ---
        if self.iteration_count % 5 == 0 {
            let t2 = std::time::Instant::now();
            let _ = self.manage_scratchpad().await;
            crate::info_log!("DEBUG_PERF: manage_scratchpad took {:?}", t2.elapsed());
        }

        // --- Process Decision (Short-Key JSON Protocol) ---
        let _t3 = std::time::Instant::now();
        let mut short_key_actions = None;
        let is_short_key_likely = content.contains("\"t\":") || content.contains("\"a\":");

        match parse_short_key_actions_from_content(&content) {
            Ok(actions) => {
                self.parse_failure_count = 0;
                short_key_actions = Some(actions);
            }
            Err(e) if is_short_key_likely => {
                toolcall_log::append_jsonl_owned(serde_json::json!({
                    "kind": "parse_error",
                    "session_id": self.session_id,
                    "iteration": self.iteration_count,
                    "parser": "short_key",
                    "message": e.message,
                }));
                self.parse_failure_count += 1;
                if self.parse_failure_count > 1 {
                    crate::info_log!("Repeated Short-Key failure (attempt {}), triggering RecoveryWorker...", self.parse_failure_count);

                    let task = self.history.iter()
                        .find(|m| m.role == MessageRole::User)
                        .map(|m| m.content.clone())
                        .unwrap_or_else(|| "No task found".to_string());

                    let mut tools_desc = String::new();
                    for tool in self.tools.values() {
                        tools_desc.push_str(&format!("- {}: {}\n  Usage: {}\n", tool.name(), tool.description(), tool.usage()));
                    }

                    let context = RecoveryContext {
                        task,
                        available_tools: tools_desc,
                        failed_content: content.clone(),
                        error_message: e.message.clone(),
                    };

                    match self.recovery_worker.recover(context, None).await {
                        Ok(recovered_actions) => {
                            crate::info_log!("Recovery successful!");
                            self.parse_failure_count = 0;
                            short_key_actions = Some(recovered_actions);
                        }
                        Err(rec_err) => {
                            toolcall_log::append_jsonl_owned(serde_json::json!({
                                "kind": "recovery_failed",
                                "session_id": self.session_id,
                                "iteration": self.iteration_count,
                                "error": format!("{rec_err}"),
                            }));
                            crate::error_log!("Recovery failed: {:?}", rec_err);
                            if let Some(event_bus) = &self.event_bus {
                                event_bus.publish(crate::agent_old::event_bus::CoreEvent::StatusUpdate {
                                    message: format!("❌ Recovery failed: {}", rec_err),
                                });
                            }
                            return Ok(AgentDecision::Error(format!("Recovery failed: {}", rec_err)));
                        }
                    }
                } else {
                    crate::error_log!("Short-Key Parsing Error: {:?}", e);
                    self.history.push(ChatMessage::assistant(content.clone()));
                    return Ok(AgentDecision::MalformedAction(e.message));
                }
            }
            Err(_) => {}
        }

        // Disabled: DEBUG_PERF logging - crate::info_log!("DEBUG_PERF: parsing took {:?}", t3.elapsed());
        // Disabled: DEBUG_PERF logging - crate::info_log!("DEBUG_PERF: Total post-processing took {:?}", step_profiler.elapsed());

        if let Some(actions) = short_key_actions {
            // Simplified: removed verbose actions log

            // Log thinking action to job registry with actual thought content
            if let Some(job_id) = self.llm_client.get_job_id() {
                let thought = actions.first()
                    .map(|a| a.thought.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "Processing...".to_string());
                self.job_registry.add_action(&job_id, ActionType::Thought, &thought);
            }

            // Check for Final Answer
            if let Some(final_answer) = actions.iter().find_map(|a| a.final_answer.clone()) {
                let thought = actions.first().map(|a| a.thought.clone()).unwrap_or_default();
                
                // Log final answer to job registry
                if let Some(job_id) = self.llm_client.get_job_id() {
                    self.job_registry.add_action(&job_id, ActionType::FinalAnswer, "Task complete");
                }
                
                self.history.push(ChatMessage::assistant(content.clone()));
                return Ok(AgentDecision::Message(
                    format!("Thought: {}\nFinal Answer: {}", thought, final_answer),
                    self.total_usage.clone(),
                ));
            }

            // Filter for actual tool actions
            let tool_actions: Vec<_> = actions.into_iter().filter(|a| a.action.is_some()).collect();
            if tool_actions.is_empty() {
                self.history.push(ChatMessage::assistant(content.clone()));
                toolcall_log::append_jsonl_owned(serde_json::json!({
                    "kind": "agent_decision",
                    "session_id": self.session_id,
                    "iteration": self.iteration_count,
                    "decision": "message",
                }));
                return Ok(AgentDecision::Message(content, self.total_usage.clone()));
            }

            // Check for confirm flag - chat first, act after approval (ReAct-style)
            if let Some(first_action) = tool_actions.first() {
                if first_action.confirm {
                    let tool_name = first_action.action.clone().unwrap();
                    let args = first_action.input.as_ref()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    let kind = self.tools.get(&tool_name).map(|t| t.kind()).unwrap_or(ToolKind::Internal);
                    
                    // Store the action as pending for later execution
                    let pending = AgentDecision::Action { 
                        tool: tool_name.clone(), 
                        args: args.clone(), 
                        kind 
                    };
                    self.pending_decision = Some(pending);
                    self.last_tool_call = Some((tool_name.clone(), args));
                    
                    // Return thought as message to chat with user first
                    let thought = if !first_action.thought.is_empty() {
                        first_action.thought.clone()
                    } else {
                        format!("I'll execute `{}` for you. Proceed?", tool_name)
                    };
                    
                    self.history.push(ChatMessage::assistant(content.clone()));
                    toolcall_log::append_jsonl_owned(serde_json::json!({
                        "kind": "agent_decision",
                        "session_id": self.session_id,
                        "iteration": self.iteration_count,
                        "decision": "confirm_request",
                        "tool": tool_name,
                    }));
                    
                    return Ok(AgentDecision::Message(thought, self.total_usage.clone()));
                }
            }

            // Convert ShortKeyAction to AgentRequest
            let mut agent_requests = Vec::new();
            for (idx, sk) in tool_actions.into_iter().enumerate() {
                // We filtered for actions where `action.is_some()`, but use safe pattern anyway
                let Some(raw_tool_name) = sk.action else {
                    crate::warn_log!("Skipping action with missing tool name at index {}", idx);
                    continue;
                };
                let tool_name = crate::agent_old::tool_registry::normalize_tool_name(&raw_tool_name).to_string();

                let input = sk.input.unwrap_or(serde_json::Value::Null);
                agent_requests.push(AgentRequest {
                    id: Some(format!("call_{}_{}", self.iteration_count, idx)),
                    thought: sk.thought,
                    action: tool_name,
                    input,
                });
            }

            // If not executing tools internally, queue all actions and return first one
            // This allows the orchestrator to execute them sequentially while maintaining
            // the chat session flow
            if !self.execute_tools_internally {
                if !agent_requests.is_empty() {
                    // Convert all requests to AgentDecision::Action and queue them
                    let mut decisions: Vec<AgentDecision> = agent_requests.into_iter()
                        .map(|req| {
                            let tool_name = req.action;
                            let args = req.input.to_string();
                            let kind = self.tools.get(&tool_name).map(|t| t.kind()).unwrap_or(ToolKind::Internal);
                            AgentDecision::Action { tool: tool_name, args, kind }
                        })
                        .collect();
                    
                    // Take the first one to return now
                    let first = decisions.remove(0);
                    // Store the rest in the queue for subsequent steps
                    self.pending_action_queue = decisions;
                    
                    if let AgentDecision::Action { ref tool, ref args, .. } = first {
                        self.last_tool_call = Some((tool.clone(), args.clone()));
                    }
                    self.history.push(ChatMessage::assistant(content.clone()));
                    
                    return Ok(first);
                }
            }

            // Build tool_calls for the assistant message (required by API when tool results follow)
            let tool_calls: Vec<crate::llm::chat::ToolCall> = agent_requests.iter()
                .filter_map(|req| {
                    req.id.as_ref().map(|id| crate::llm::chat::ToolCall {
                        id: id.clone(),
                        type_: "function".to_string(),
                        function: crate::llm::chat::ToolCallFunction {
                            name: req.action.clone(),
                            arguments: req.input.to_string(),
                        },
                    })
                })
                .collect();

            // Execute Actions in Parallel (internal mode)
            let results = execute_parallel_tools(
                agent_requests,
                &self.tools,
                self.scribe.clone(),
                &self.permissions,
                &None, // No internal event channel, using EventBus
                &Some(self.job_registry.clone()),
                self.llm_client.get_job_id().as_deref(),
            ).await?;

            // Add Assistant content to history WITH tool_calls
            let assistant_msg = if tool_calls.is_empty() {
                ChatMessage::assistant(content.clone())
            } else {
                ChatMessage {
                    role: MessageRole::Assistant,
                    content: content.clone(),
                    name: None,
                    tool_call_id: None,
                    tool_calls: Some(tool_calls),
                    reasoning_content: None,
                }
            };
            self.history.push(assistant_msg);

            // Track consecutive tool failures (before consuming results)
            let all_failed = results.iter().all(|r| r.error.is_some());
            let any_succeeded = results.iter().any(|r| r.result.is_some());

            // Add Tool results to history
            let mut observation_summary = String::new();
            for res in results {
                let tool_id = res.result.as_ref()
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                // Maximum tokens per tool output (roughly 4000 chars = ~1000 tokens)
                const MAX_OUTPUT_CHARS: usize = 8000;
                
                let (output, is_error) = if let Some(r) = &res.result {
                    // Extract the actual tool output, not the JSON wrapper
                    let actual_output = r.get("output")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| r.to_string());
                    // Truncate if too long to prevent context explosion
                    let truncated = if actual_output.len() > MAX_OUTPUT_CHARS {
                        format!("{}...[truncated {} chars]", 
                            &actual_output[..MAX_OUTPUT_CHARS], 
                            actual_output.len() - MAX_OUTPUT_CHARS)
                    } else {
                        actual_output
                    };
                    (truncated, false)
                } else if let Some(e) = &res.error {
                    (format!("Error: {}", e.message), true)
                } else {
                    ("No output".to_string(), false)
                };

                // Add action stamp for tool execution
                if is_error {
                    self.context_manager.add_stamp(
                        crate::context::stamps::tool_failed(&tool_id, &output)
                    );
                } else {
                    // Extract tool name from the result if possible
                    let tool_name = res.result.as_ref()
                        .and_then(|v| v.get("status"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool");
                    self.context_manager.add_stamp(
                        crate::context::stamps::tool_success(tool_name, Some(&output[..output.len().min(50)]))
                    );
                }

                observation_summary.push_str(&format!("\n- {}", output));
                self.history.push(ChatMessage::tool(tool_id, "batch".to_string(), output));
            }

            // Update consecutive tool failure tracking
            
            if any_succeeded {
                self.consecutive_tool_failures = 0;
            } else if all_failed {
                self.consecutive_tool_failures += 1;
                
                // Check if we've exceeded the max tool failures limit
                if self.consecutive_tool_failures >= self.max_tool_failures {
                    crate::error_log!(
                        "Worker stalled: {} consecutive tool failures (max: {})",
                        self.consecutive_tool_failures,
                        self.max_tool_failures
                    );
                    return Ok(AgentDecision::Stall {
                        reason: format!(
                            "Worker stalled after {} consecutive tool failures",
                            self.consecutive_tool_failures
                        ),
                        tool_failures: self.consecutive_tool_failures,
                    });
                }
            }

            return Ok(AgentDecision::Message(
                format!("Executed parallel tools:{}", observation_summary),
                self.total_usage.clone(),
            ));
        }

        // 2. Handle Tool Calls (Modern API)
        if let Some(tool_calls) = response.choices[0].message.tool_calls.as_ref() {
            if !tool_calls.is_empty() {
                let mut message = response.choices[0].message.clone();
                if tool_calls.len() > 1 {
                    if let Some(tc) = message.tool_calls.as_mut() { tc.truncate(1); }
                }
                let (tool_name, args, tool_id) = {
                    // Safe access: we just checked tool_calls is not empty, and if truncation happened it still has at least 1
                    let tool_call = message.tool_calls.as_ref().and_then(|calls| calls.first())
                        .ok_or_else(|| Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "No tool calls available after truncation")) as Box<dyn StdError + Send + Sync>)?;
                    let normalized_name = crate::agent_old::tool_registry::normalize_tool_name(&tool_call.function.name).to_string();
                    (normalized_name, tool_call.function.arguments.to_string(), tool_call.id.clone())
                };

                // Recoverable Error for Unknown Tools
                if !self.tools.contains_key(&tool_name) {
                    let available_tools: Vec<_> = self.tools.keys().cloned().collect();
                    let observation = format!(
                        "TOOL_ERROR(name_not_found, requested='{}', available={:?})",
                        tool_name, available_tools
                    );

                    self.history.push(message);
                    self.history.push(ChatMessage::tool(tool_id, tool_name.clone(), observation.clone()));

                    toolcall_log::append_jsonl_owned(serde_json::json!({
                        "kind": "tool_error",
                        "session_id": self.session_id,
                        "iteration": self.iteration_count,
                        "error": "name_not_found",
                        "tool": tool_name
                    }));

                    return Ok(AgentDecision::Message(
                        format!("I attempted to call tool '{}' but it was not found. I will try again with a valid tool.", tool_name),
                        self.total_usage.clone()
                    ));
                }

                self.pending_tool_call_id = Some(tool_id);
                self.last_tool_call = Some((tool_name.clone(), args.clone()));
                let kind = self.tools.get(&tool_name).map(|t| t.kind()).unwrap_or(ToolKind::Internal);
                self.history.push(message);
                let action = AgentDecision::Action { tool: tool_name, args, kind };
                toolcall_log::append_jsonl_owned(serde_json::json!({
                    "kind": "agent_decision",
                    "session_id": self.session_id,
                    "iteration": self.iteration_count,
                    "decision": "tool_call_native",
                    "tool": self.last_tool_call.as_ref().map(|(n, _)| n.clone()),
                    "args": self.last_tool_call.as_ref().map(|(_, a)| truncate_for_log(a, 4000)),
                }));
                if !content.trim().is_empty() {
                    self.pending_decision = Some(action);
                    return Ok(AgentDecision::Message(content, self.total_usage.clone()));
                }
                return Ok(action);
            }
        }

        // 3. Handle ReAct format
        let action_re = Regex::new(r"(?m)^Action:\s*(.*)")?;
        let action_input_re = Regex::new(r"(?ms)^Action Input:\s*(.*?)(?:\nThought:|\nObservation:|\nFinal Answer:|\z)")?;
        let action_match = action_re.captures(&content);
        let action_input_match = action_input_re.captures(&content);

        if let (Some(raw_tool_name), Some(args)) = (action_match.as_ref().map(|c| c[1].trim().to_string()), action_input_match.as_ref().map(|c| c[1].trim().to_string())) {
            let tool_name = crate::agent_old::tool_registry::normalize_tool_name(&raw_tool_name).to_string();
            self.last_tool_call = Some((tool_name.clone(), args.clone()));
            if content.contains("Final Answer:") {
                self.history.push(ChatMessage::assistant(content.clone()));
                return Ok(AgentDecision::Message(content, self.total_usage.clone()));
            }
            let kind = self.tools.get(&tool_name).map(|t| t.kind()).unwrap_or(ToolKind::Internal);
            self.history.push(ChatMessage::assistant(content.clone()));
            let action_decision = AgentDecision::Action { tool: tool_name, args, kind };
            if let Some(pos) = content.find("Action:") {
                let thought = content[..pos].trim().to_string();
                if !thought.is_empty() {
                    self.pending_decision = Some(action_decision);
                    return Ok(AgentDecision::Message(thought, self.total_usage.clone()));
                }
            }
            return Ok(action_decision);
        }

        // 4. Final Answer or just a message
        self.history.push(ChatMessage::assistant(content.clone()));
        toolcall_log::append_jsonl_owned(serde_json::json!({
            "kind": "agent_decision",
            "session_id": self.session_id,
            "iteration": self.iteration_count,
            "decision": "message",
        }));
        Ok(AgentDecision::Message(content, self.total_usage.clone()))
    }

    /// Manage scratchpad: periodic cleanup of old and expired entries
    async fn manage_scratchpad(&mut self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        // Use try_write to avoid blocking
        let mut scratchpad = match self.scratchpad.try_write() {
            Ok(guard) => guard,
            Err(_) => {
                crate::error_log!("Scratchpad write lock busy in manage_scratchpad, skipping cleanup");
                return Ok(());
            }
        };
        let removed = scratchpad.cleanup();
        if removed > 0 {
            crate::info_log!("Scratchpad cleanup: removed {} old/expired entries, current size: {} chars, entries: {}",
                removed,
                scratchpad.get_size(),
                scratchpad.len()
            );
        }
        Ok(())
    }

    /// Inject relevant memories into the conversation history based on the last user message.
    pub async fn inject_memory_context(&mut self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        self.memory_manager.inject_memory_context(
            &mut self.history,
            self.llm_client.config().memory.auto_context,
        ).await
    }

    /// Build context from memories.
    pub fn build_context_from_memories(&self, memories: &[crate::memory::store::Memory]) -> String {
        self.memory_manager.build_context_from_memories(memories)
    }

    /// Automatically categorize a newly added memory.
    pub async fn auto_categorize(&self, memory_id: i64, content: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        self.memory_manager.auto_categorize(memory_id, content).await
    }

    /// Condense the conversation history by summarizing older messages.
    pub async fn condense_history(&self, _history: &[ChatMessage]) -> Result<Vec<ChatMessage>, Box<dyn StdError + Send + Sync>> {
        let manager = self.context_manager.clone();
        match manager.condense_history(&self.llm_client).await {
            Ok(result) => Ok(result),
            Err(e) => Err(format!("Condensation failed: {}", e).into()),
        }
    }

    /// Legacy run method for backward compatibility.
    pub async fn run(
        &mut self,
        history: Vec<ChatMessage>,
        event_bus: Arc<EventBus>,
        interrupt_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        auto_approve: bool,
        max_driver_loops: usize,
        approval_rx: Option<tokio::sync::mpsc::Receiver<bool>>,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        use crate::agent_old::v2::driver::run_legacy;
        run_legacy(self, history, event_bus, interrupt_flag, auto_approve, max_driver_loops, approval_rx).await
    }

    /// Event-driven run method with heartbeat loop and budget management.
    pub async fn run_event_driven(
        &mut self,
        history: Vec<ChatMessage>,
        event_bus: Arc<EventBus>,
        interrupt_rx: tokio::sync::mpsc::Receiver<()>,
        approval_rx: tokio::sync::mpsc::Receiver<bool>,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        use crate::agent_old::v2::driver::run_event_driven;
        run_event_driven(self, history, event_bus, interrupt_rx, approval_rx).await
    }
}

fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let mut out = s[..max_len].to_string();
    out.push('…');
    out
}
