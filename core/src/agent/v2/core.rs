//! Agent V2 Core Implementation
use crate::llm::{LlmClient, chat::{ChatMessage, MessageRole}, TokenUsage};
use crate::agent::tool::{Tool, ToolKind};
use crate::agent::toolcall_log;
use crate::agent::v2::jobs::JobRegistry;
use crate::agent::event::RuntimeEvent;
use crate::agent::v2::recovery::{RecoveryWorker, RecoveryContext};
use crate::agent::v2::protocol::{AgentDecision, AgentRequest, parse_short_key_actions_from_content};
use crate::agent::v2::prompt::PromptBuilder;
use crate::agent::v2::execution::execute_parallel_tools;
use crate::agent::v2::memory::MemoryManager;
use crate::memory::{MemoryCategorizer, scribe::Scribe};
use crate::context::ContextManager;
use crate::config::v2::types::AgentPermissions;

use std::error::Error as StdError;
use std::sync::Arc;
use std::sync::RwLock;
use std::collections::HashMap;
use regex::Regex;
use crate::agent::tools::ScratchpadTool;
use crate::agent::tools::ConsolidateTool;

/// The core AgentV2 that manages the agentic loop.
pub struct AgentV2 {
    pub llm_client: Arc<LlmClient>,
    pub scribe: Arc<Scribe>,
    pub tools: HashMap<String, Arc<dyn Tool>>,
    pub job_registry: JobRegistry,
    pub max_iterations: usize,
    pub system_prompt_prefix: String,
    pub memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    pub categorizer: Option<Arc<MemoryCategorizer>>,
    pub session_id: String,
    pub version: crate::config::AgentVersion,
    pub event_tx: Option<tokio::sync::mpsc::UnboundedSender<RuntimeEvent>>,

    // State maintained between steps (managed by LifecycleManager)
    pub history: Vec<ChatMessage>,
    pub iteration_count: usize,
    pub total_usage: TokenUsage,
    pub pending_decision: Option<AgentDecision>,

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

    /// Optional capabilities context to inject into system prompt
    pub capabilities_context: Option<String>,

    /// Optional permission controls for this agent
    pub permissions: Option<AgentPermissions>,

    /// Scratchpad for short-term working memory
    pub scratchpad: Arc<RwLock<String>>,

    /// Context manager for token counting, pruning, and condensation
    pub context_manager: ContextManager,
    /// Disable memory recall and hot memory injection (incognito mode)
    pub disable_memory: bool,

    // Helper components
    memory_manager: MemoryManager,
    prompt_builder: PromptBuilder,
}

impl AgentV2 {
    pub fn new_with_iterations(
        client: Arc<LlmClient>,
        scribe: Arc<Scribe>,
        tools: Vec<Arc<dyn Tool>>,
        system_prompt_prefix: String,
        max_iterations: usize,
        version: crate::config::AgentVersion,
        memory_store: Option<Arc<crate::memory::store::VectorStore>>,
        categorizer: Option<Arc<MemoryCategorizer>>,
        job_registry: Option<JobRegistry>,
        capabilities_context: Option<String>,
        permissions: Option<AgentPermissions>,
        scratchpad: Option<Arc<RwLock<String>>>,
        disable_memory: bool,
    ) -> Self {
        let mut tool_map = HashMap::new();
        for tool in tools {
            tool_map.insert(tool.name().to_string(), tool);
        }

        let scratchpad = scratchpad.unwrap_or_else(|| Arc::new(RwLock::new(String::new())));
        let scratchpad_tool = Arc::new(ScratchpadTool::new(scratchpad.clone()));
        tool_map.insert(scratchpad_tool.name().to_string(), scratchpad_tool);

        if let Some(store) = &memory_store {
            let consolidate_tool = Arc::new(ConsolidateTool::new(scratchpad.clone(), store.clone()));
            tool_map.insert(consolidate_tool.name().to_string(), consolidate_tool);
        }

        let session_id = chrono::Utc::now().timestamp_millis().to_string();

        // Initialize context manager from LLM client config
        let context_manager = ContextManager::from_llm_client(&client);

        // Create helper components
        let memory_manager = MemoryManager::new(
            scribe.clone(),
            memory_store.clone(),
            categorizer.clone(),
            disable_memory,
        );
        let prompt_builder = PromptBuilder::new(
            system_prompt_prefix.clone(),
            tool_map.clone(),
            capabilities_context.clone(),
        );

        Self {
            llm_client: client.clone(),
            scribe,
            tools: tool_map,
            job_registry: job_registry.unwrap_or_else(JobRegistry::new),
            max_iterations,
            system_prompt_prefix,
            categorizer,
            memory_store,
            session_id,
            history: Vec::new(),
            iteration_count: 0,
            total_usage: TokenUsage::default(),
            pending_decision: None,
            last_tool_call: None,
            repetition_count: 0,
            pending_tool_call_id: None,
            version,
            event_tx: None,
            recovery_worker: RecoveryWorker::new(client.clone()),
            parse_failure_count: 0,
            budget: max_iterations,
            max_steps: max_iterations,
            heartbeat_interval: std::time::Duration::from_secs(5),
            safety_timeout: std::time::Duration::from_secs(300),
            capabilities_context,
            permissions,
            scratchpad,
            context_manager,
            disable_memory,
            memory_manager,
            prompt_builder,
        }
    }

    pub fn with_event_tx(mut self, tx: tokio::sync::mpsc::UnboundedSender<RuntimeEvent>) -> Self {
        self.event_tx = Some(tx);
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

    /// Reset the agent's state for a new task.
    pub async fn reset(&mut self, history: Vec<ChatMessage>) {
        self.history = history;
        self.iteration_count = 0;
        self.total_usage = TokenUsage::default();
        self.pending_decision = None;
        self.last_tool_call = None;
        self.repetition_count = 0;
        self.parse_failure_count = 0;
        self.pending_tool_call_id = None;
        self.max_steps = self.budget;

        // Ensure system prompt is present with capability awareness
        if self.history.is_empty() || self.history[0].role != MessageRole::System {
            let scratchpad_content = self.scratchpad.read().unwrap_or_else(|e| e.into_inner());
            self.history.insert(0, ChatMessage::system(self.prompt_builder.build(&scratchpad_content)));
        }
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

    /// Perform a single step in the agentic loop.
    pub async fn step(&mut self, observation: Option<String>) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
        // Check scratchpad size and inject warning if needed
        if let Ok(guard) = self.scratchpad.read() {
            if guard.len() > 4000 {
                let warning = format!("SYSTEM ALERT: Scratchpad size is {}/4000. Please use `consolidate_memory` to save important facts to long-term memory and condense the scratchpad.", guard.len());

                // Avoid spamming if the last message is already the warning
                let last_msg_is_warning = self.history.last()
                    .map(|m| m.content == warning)
                    .unwrap_or(false);

                if !last_msg_is_warning {
                    self.history.push(ChatMessage::system(warning));
                }
            }
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

        if let Some(obs) = observation {
            // Log the observation
            if let Err(e) = self.scribe.observe(crate::memory::journal::InteractionType::Output, &obs).await {
                crate::error_log!("Failed to log observation to memory: {}", e);
                if let Some(tx) = &self.event_tx {
                    let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", e) });
                }
            }

            if let Some(tool_id) = self.pending_tool_call_id.take() {
                let tool_name = self.last_tool_call.as_ref().map(|(n, _)| n.clone()).unwrap_or_else(|| "unknown".to_string());
                self.history.push(ChatMessage::tool(tool_id, tool_name, obs));
            } else {
                self.history.push(ChatMessage::user(format!("Observation: {}", obs)));
            }
        }

        // --- Context Management (Pruning & Condensation) ---
        self.context_manager.set_history(&self.history);

        // Prepare context (condense if needed, then prune)
        match self.context_manager.prepare_context(Some(&self.llm_client)).await {
            Ok(optimized_history) => {
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

        // Structured debug log (JSONL)
        let tool_calls_json = response.choices
            .first()
            .and_then(|c| c.message.tool_calls.as_ref())
            .map(|tcs| serde_json::to_value(tcs).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null);
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

        // Log the thought/response
        if let Err(e) = self.scribe.observe(crate::memory::journal::InteractionType::Thought, &content).await {
            crate::error_log!("Failed to log thought to memory: {}", e);
            if let Some(tx) = &self.event_tx {
                let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", e) });
            }
        }

        if let Some(usage) = &response.usage {
            self.total_usage.prompt_tokens += usage.prompt_tokens;
            self.total_usage.completion_tokens += usage.completion_tokens;
            self.total_usage.total_tokens += usage.total_tokens;
        }

        self.iteration_count += 1;

        // --- Process Decision (Short-Key JSON Protocol) ---
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
                            if let Some(tx) = &self.event_tx {
                                let _ = tx.send(RuntimeEvent::StatusUpdate {
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

        if let Some(actions) = short_key_actions {
            crate::info_log!("Processing Actions (Short-Key): {:?}", actions);

            // Check for Final Answer
            if let Some(final_answer) = actions.iter().find_map(|a| a.final_answer.clone()) {
                let thought = actions.first().map(|a| a.thought.clone()).unwrap_or_default();
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

            // Convert ShortKeyAction to AgentRequest
            let mut agent_requests = Vec::new();
            for (idx, sk) in tool_actions.into_iter().enumerate() {
                let raw_tool_name = sk.action.unwrap();
                let tool_name = crate::agent::tool_registry::normalize_tool_name(&raw_tool_name).to_string();

                let input = sk.input.unwrap_or(serde_json::Value::Null);
                agent_requests.push(AgentRequest {
                    id: Some(format!("call_{}_{}", self.iteration_count, idx)),
                    thought: sk.thought,
                    action: tool_name,
                    input,
                });
            }

            // Execute Actions in Parallel
            let results = execute_parallel_tools(
                agent_requests,
                &self.tools,
                self.scribe.clone(),
                &self.permissions,
                &self.event_tx,
            ).await?;

            // Add Assistant content to history
            self.history.push(ChatMessage::assistant(content.clone()));

            // Add Tool results to history
            let mut observation_summary = String::new();
            for res in results {
                let tool_id = res.result.as_ref()
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                let output = if let Some(r) = &res.result {
                    r.to_string()
                } else if let Some(e) = &res.error {
                    format!("Error: {}", e.message)
                } else {
                    "No output".to_string()
                };

                observation_summary.push_str(&format!("\n- {}", output));
                self.history.push(ChatMessage::tool(tool_id, "batch".to_string(), output));
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
                    let tool_call = &message.tool_calls.as_ref().unwrap()[0];
                    let normalized_name = crate::agent::tool_registry::normalize_tool_name(&tool_call.function.name).to_string();
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
            let tool_name = crate::agent::tool_registry::normalize_tool_name(&raw_tool_name).to_string();
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
        event_tx: tokio::sync::mpsc::UnboundedSender<crate::terminal::app::TuiEvent>,
        interrupt_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        auto_approve: bool,
        max_driver_loops: usize,
        approval_rx: Option<tokio::sync::mpsc::Receiver<bool>>,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        use crate::agent::v2::driver::run_legacy;
        run_legacy(self, history, event_tx, interrupt_flag, auto_approve, max_driver_loops, approval_rx).await
    }

    /// Event-driven run method with heartbeat loop and budget management.
    pub async fn run_event_driven(
        &mut self,
        history: Vec<ChatMessage>,
        event_tx: tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
        interrupt_rx: tokio::sync::mpsc::Receiver<()>,
        approval_rx: tokio::sync::mpsc::Receiver<bool>,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        use crate::agent::v2::driver::run_event_driven;
        run_event_driven(self, history, event_tx, interrupt_rx, approval_rx).await
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
