//! Agent V2 Core Implementation
use crate::llm::{LlmClient, chat::{ChatMessage, ChatRequest, MessageRole}, TokenUsage};
use crate::agent::tool::{Tool, ToolKind, ToolOutput};
use crate::agent::toolcall_log;
use crate::agent::v2::jobs::{JobRegistry, JobStatus};
use crate::agent::event::RuntimeEvent;
use crate::agent::v2::recovery::{RecoveryWorker, RecoveryContext};
use crate::agent::v2::protocol::{AgentDecision, AgentRequest, AgentResponse, AgentError, parse_short_key_actions_from_content};
use crate::agent::v2::prompt::PromptBuilder;
use crate::agent::v2::memory::MemoryManager;
use crate::memory::{MemoryCategorizer, scribe::Scribe, journal::InteractionType};
use crate::terminal::app::TuiEvent;
use crate::context::ContextManager;
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

    // State maintained between steps
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
            self.prompt_builder = PromptBuilder::new(
                self.system_prompt_prefix.clone(),
                self.tools.clone(),
                self.capabilities_context.clone(),
            );
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
            if let Err(e) = self.scribe.observe(InteractionType::Output, &obs).await {
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

        let mut request = ChatRequest::new(self.llm_client.model().to_string(), request_history);

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
        if let Err(e) = self.scribe.observe(InteractionType::Thought, &content).await {
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
            let results = self.execute_parallel_tools(agent_requests).await?;

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

    async fn execute_parallel_tools(&self, requests: Vec<AgentRequest>) -> Result<Vec<AgentResponse>, Box<dyn StdError + Send + Sync>> {
        let mut futures = Vec::new();

        for req in requests {
            let event_tx = self.event_tx.clone();
            let tool = self.tools.get(&req.action).cloned();
            let scribe = self.scribe.clone();

            futures.push(async move {
                if let Some(tx) = &event_tx {
                    let _: Result<(), _> = tx.send(RuntimeEvent::Step { request: req.clone() });
                }

                let response = match tool {
                    Some(t) => {
                        let args = if req.input.is_string() {
                            req.input.as_str().unwrap().to_string()
                        } else {
                            req.input.to_string()
                        };

                        if let Err(e) = scribe.observe(InteractionType::Tool, &format!("Action: {}\nInput: {}", req.action, args)).await {
                            crate::error_log!("Failed to log tool call to memory: {}", e);
                            if let Some(tx) = &event_tx {
                                let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", e) });
                            }
                        }

                        match t.call(&args).await {
                            Ok(output) => {
                                let output_str = output.as_string();
                                if let Err(log_err) = scribe.observe(InteractionType::Output, &output_str).await {
                                    crate::error_log!("Failed to log tool output to memory: {}", log_err);
                                    if let Some(tx) = &event_tx {
                                        let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                                    }
                                }
                                AgentResponse {
                                    result: Some(serde_json::json!({
                                        "output": output_str,
                                        "id": req.id.clone().unwrap_or_default(),
                                        "status": match output {
                                            ToolOutput::Immediate(_) => "immediate",
                                            ToolOutput::Background { .. } => "background",
                                        }
                                    })),
                                    error: None,
                                }
                            },
                            Err(e) => {
                                let error_msg = e.to_string();
                                if let Err(log_err) = scribe.observe(InteractionType::Output, &format!("Error: {}", error_msg)).await {
                                    crate::error_log!("Failed to log tool error to memory: {}", log_err);
                                    if let Some(tx) = &event_tx {
                                        let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                                    }
                                }
                                AgentResponse {
                                    result: None,
                                    error: Some(AgentError {
                                        message: error_msg,
                                        code: Some("TOOL_ERROR".to_string()),
                                        context: None,
                                    }),
                                }
                            },
                        }
                    }
                    None => AgentResponse {
                        result: None,
                        error: Some(AgentError {
                            message: format!("Tool '{}' not found", req.action),
                            code: Some("NOT_FOUND".to_string()),
                            context: None,
                        }),
                    },
                };

                if let Some(tx) = &event_tx {
                    let _: Result<(), _> = tx.send(RuntimeEvent::ToolOutput { response: response.clone() });
                }

                response
            });
        }

        let results = futures::future::join_all(futures).await;
        Ok(results)
    }

    /// Inject relevant memories into the conversation history based on the last user message.
    pub async fn inject_memory_context(&mut self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        self.memory_manager.inject_memory_context(
            &mut self.history,
            self.llm_client.config().memory.auto_context,
        ).await
    }

    /// Build context from memories.
    fn build_context_from_memories(&self, memories: &[crate::memory::store::Memory]) -> String {
        self.memory_manager.build_context_from_memories(memories)
    }

    /// Automatically categorize a newly added memory.
    pub async fn auto_categorize(&self, memory_id: i64, content: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        self.memory_manager.auto_categorize(memory_id, content).await
    }

    /// Legacy run method for backward compatibility.
    pub async fn run(
        &mut self,
        mut history: Vec<ChatMessage>,
        event_tx: tokio::sync::mpsc::UnboundedSender<TuiEvent>,
        interrupt_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        auto_approve: bool,
        max_driver_loops: usize,
        mut approval_rx: Option<tokio::sync::mpsc::Receiver<bool>>,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        // 1. Memory Context Injection (if enabled)
        if self.llm_client.config().memory.auto_context {
            if let Some(store) = &self.memory_store {
                if let Some(last_user_msg) = history.iter().rev().find(|m| m.role == MessageRole::User) {
                    let _ = event_tx.send(TuiEvent::StatusUpdate("Searching memory...".to_string()));
                    let memories = store.search_memory(&last_user_msg.content, 5).await.unwrap_or_default();
                    if !memories.is_empty() {
                        let context = self.build_context_from_memories(&memories);
                        if let Some(user_idx) = history.iter().rposition(|m| m.role == MessageRole::User) {
                            history[user_idx].content.push_str("\n\n");
                            history[user_idx].content.push_str(&context);
                        }
                    }
                }
            }
        }

        self.reset(history).await;
        self.inject_hot_memory(5).await;

        let mut last_observation = None;
        let mut retry_count = 0;
        let max_retries = 3;

        let mut loop_iteration = 0;
        loop {
            loop_iteration += 1;
            if loop_iteration > max_driver_loops {
                return Ok((format!("Error: Driver-level safety limit reached ({} loops). Potential infinite loop detected.", max_driver_loops), self.total_usage.clone()));
            }

            if interrupt_flag.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(("Interrupted by user.".to_string(), self.total_usage.clone()));
            }

            let _ = event_tx.send(TuiEvent::StatusUpdate("Thinking...".to_string()));

            match self.step(last_observation.take()).await? {
                AgentDecision::Message(msg, usage) => {
                    retry_count = 0;
                    let _ = event_tx.send(TuiEvent::AgentResponse(msg.clone(), usage.clone()));

                    if self.has_pending_decision() {
                        continue;
                    }

                    if msg.contains("Final Answer:") || msg.contains("\"f\":") || msg.contains("\"final_answer\":") {
                        let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                        return Ok((msg, usage));
                    }

                    if msg.trim().ends_with('?')
                        || msg.contains("Please")
                        || msg.contains("Would you")
                        || msg.contains("Acknowledged")
                        || msg.contains("I've memorized")
                        || msg.contains("Absolutely")
                    {
                        let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                        return Ok((msg, usage));
                    }

                    if msg.len() > 30 {
                        let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                        return Ok((msg, usage));
                    }

                    last_observation = Some("Please continue your task or provide a Final Answer if you are done.".to_string());
                    continue;
                }
                AgentDecision::Action { tool, args, kind } => {
                    retry_count = 0;
                    let _ = event_tx.send(TuiEvent::StatusUpdate(format!("Tool: '{}'", tool)));

                    if !auto_approve {
                        let suggestion = if tool == "execute_command" {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                                v.get("command").and_then(|c| c.as_str())
                                    .or_else(|| v.get("args").and_then(|c| c.as_str()))
                                    .unwrap_or(&args)
                                    .to_string()
                            } else {
                                args.clone()
                            }
                        } else {
                            format!("{} {}", tool, args)
                        };
                        let _ = event_tx.send(TuiEvent::SuggestCommand(suggestion));

                        if let Some(rx) = &mut approval_rx {
                            let _ = event_tx.send(TuiEvent::StatusUpdate("Waiting for approval...".to_string()));
                            match rx.recv().await {
                                Some(true) => {}
                                Some(false) => {
                                    let _ = event_tx.send(TuiEvent::StatusUpdate("Denied.".to_string()));
                                    last_observation = Some(format!("Error: User denied the execution of tool '{}'.", tool));
                                    continue;
                                }
                                None => {
                                    return Ok(("Error: Approval channel closed.".to_string(), self.total_usage.clone()));
                                }
                            }
                        } else {
                            return Ok((format!("Approval required to run tool '{}' but no approval channel is available (AUTO-APPROVE is OFF).", tool), self.total_usage.clone()));
                        }
                    }

                    let processed_args = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                        v.get("args")
                         .and_then(|a| a.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(args.clone())
                    } else {
                        args.clone()
                    };

                    if let Err(e) = self.scribe.observe(InteractionType::Tool, &format!("Action: {}\nInput: {}", tool, processed_args)).await {
                        crate::error_log!("Failed to log tool call to memory: {}", e);
                        if let Some(tx) = &self.event_tx {
                            let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", e) });
                        }
                    }

                    let observation = match self.tools.get(&tool) {
                        Some(t) => match t.call(&processed_args).await {
                            Ok(output) => {
                                let output_str = output.as_string();
                                if let Err(log_err) = self.scribe.observe(InteractionType::Output, &output_str).await {
                                    crate::error_log!("Failed to log tool output to memory: {}", log_err);
                                    if let Some(tx) = &self.event_tx {
                                        let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                                    }
                                }
                                output_str
                            },
                            Err(e) => {
                                let error_msg = format!("Tool Error: {}. Analyze the failure and try a different command or approach if possible.", e);
                                let _ = event_tx.send(TuiEvent::StatusUpdate(format!("❌ Tool '{}' failed", tool)));
                                if let Err(log_err) = self.scribe.observe(InteractionType::Output, &error_msg).await {
                                    crate::error_log!("Failed to log tool error to memory: {}", log_err);
                                    if let Some(tx) = &self.event_tx {
                                        let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                                    }
                                }
                                error_msg
                            },
                        },
                        None => {
                            let error_msg = format!("Error: Tool '{}' not found. Check the available tools list.", tool);
                            if let Err(log_err) = self.scribe.observe(InteractionType::Output, &error_msg).await {
                                crate::error_log!("Failed to log tool-not-found error to memory: {}", log_err);
                                if let Some(tx) = &self.event_tx {
                                    let _ = tx.send(RuntimeEvent::StatusUpdate { message: format!("Memory logging error: {}", log_err) });
                                }
                            }
                            error_msg
                        },
                    };

                    if kind == ToolKind::Internal {
                        let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", observation.trim());
                        let _ = event_tx.send(TuiEvent::InternalObservation(obs_log.into_bytes()));
                    }

                    last_observation = Some(observation);
                }
                AgentDecision::MalformedAction(error) => {
                    retry_count += 1;
                    if retry_count > max_retries {
                        let fatal_error = format!("Fatal: Failed to parse agent response after {} attempts. Last error: {}", max_retries, error);
                        let _ = event_tx.send(TuiEvent::StatusUpdate(fatal_error.clone()));
                        return Ok((fatal_error, self.total_usage.clone()));
                    }

                    let _ = event_tx.send(TuiEvent::StatusUpdate(format!("⚠️ {} Retrying ({}/{})", error, retry_count, max_retries)));

                    let nudge = format!(
                        "{}\n\n\
                        IMPORTANT: You must follow the ReAct format exactly:\n\
                        Thought: <your reasoning>\n\
                        Action: <tool name>\n\
                        Action Input: <tool arguments>\n\n\
                        Do not include any other text after Action Input.",
                        error
                    );
                    last_observation = Some(nudge);
                    continue;
                }
                AgentDecision::Error(e) => {
                    let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                    return Err(e.into());
                }
            }
        }
    }

    /// Condense the conversation history by summarizing older messages.
    pub async fn condense_history(&self, _history: &[ChatMessage]) -> Result<Vec<ChatMessage>, Box<dyn StdError + Send + Sync>> {
        let manager = self.context_manager.clone();
        match manager.condense_history(&self.llm_client).await {
            Ok(result) => Ok(result),
            Err(e) => Err(format!("Condensation failed: {}", e).into()),
        }
    }

    /// Event-driven run method with heartbeat loop and budget management.
    pub async fn run_event_driven(
        &mut self,
        history: Vec<ChatMessage>,
        event_tx: tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
        mut interrupt_rx: tokio::sync::mpsc::Receiver<()>,
        mut approval_rx: tokio::sync::mpsc::Receiver<bool>,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        self.reset(history).await;
        self.inject_hot_memory(5).await;

        let start_time = std::time::Instant::now();
        let heartbeat_interval = self.heartbeat_interval;
        let safety_timeout = self.safety_timeout;

        let mut last_observation = None;
        let mut heartbeat_timer = tokio::time::interval(heartbeat_interval);
        heartbeat_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut recovery_attempts = 0;

        loop {
            if start_time.elapsed() > safety_timeout {
                let message = format!("⚠️ Safety timeout reached ({:?}). Stopping autonomous run.", safety_timeout);
                let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });
                return Ok((message, self.total_usage.clone()));
            }

            if self.iteration_count >= self.max_steps {
                let message = format!("⚠️ Step budget exceeded ({}/{}). Requesting permission to continue...",
                    self.iteration_count, self.max_steps);
                let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });

                match approval_rx.recv().await {
                    Some(true) => {
                        self.max_steps = (self.max_steps as f64 * 1.5) as usize;
                        let continue_msg = format!("✅ Budget increased to {} steps. Continuing...", self.max_steps);
                        let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: continue_msg });
                    }
                    Some(false) => {
                        let stop_msg = "🛑 User denied budget increase. Stopping execution.".to_string();
                        let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: stop_msg.clone() });
                        return Ok((stop_msg, self.total_usage.clone()));
                    }
                    None => {
                        let error_msg = "❌ Approval channel closed. Stopping execution.".to_string();
                        let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: error_msg.clone() });
                        return Ok((error_msg, self.total_usage.clone()));
                    }
                }
            }

            tokio::select! {
                _ = heartbeat_timer.tick() => {
                    let active_jobs = self.job_registry.poll_updates();
                    if !active_jobs.is_empty() {
                        for job in active_jobs {
                            match job.status {
                                JobStatus::Completed => {
                                    let result_str = job.result
                                        .as_ref()
                                        .map(|r| r.to_string())
                                        .unwrap_or_else(|| "Job completed successfully".to_string());

                                    let message = format!("✅ Background job '{}' completed: {}", job.description, result_str);
                                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });

                                    let observation = format!("Background job '{}' result: {}", job.description, result_str);
                                    last_observation = Some(observation);
                                }
                                JobStatus::Failed => {
                                    let error_msg = job.error
                                        .as_ref()
                                        .map(|e| e.as_str())
                                        .unwrap_or("Unknown error");

                                    let message = format!("❌ Background job '{}' failed: {}", job.description, error_msg);
                                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });

                                    let observation = format!("Background job '{}' failed: {}", job.description, error_msg);
                                    last_observation = Some(observation);
                                }
                                JobStatus::Running => {
                                    let message = format!("⏳ Background job '{}' is still running...", job.description);
                                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message });
                                }
                            }
                        }
                    }
                }

                result = self.step(last_observation.take()) => {
                    match result {
                        Ok(decision) => {
                            recovery_attempts = 0;
                            match decision {
                                AgentDecision::Message(msg, usage) => {
                                    let _ = event_tx.send(RuntimeEvent::AgentResponse {
                                        content: msg.clone(),
                                        usage: usage.clone()
                                    });

                                    if msg.contains("Final Answer:") || msg.contains("\"f\":") || msg.contains("\"final_answer\":") {
                                        let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                            message: "Task completed successfully.".to_string()
                                        });
                                        return Ok((msg, usage));
                                    }

                                    if msg.trim().ends_with('?')
                                        || msg.contains("Please")
                                        || msg.contains("Would you")
                                        || msg.contains("Acknowledged")
                                        || msg.contains("I've memorized")
                                        || msg.contains("Absolutely")
                                        || msg.len() > 30
                                    {
                                        let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                            message: "Agent is waiting for user input.".to_string()
                                        });
                                        return Ok((msg, usage));
                                    }

                                    last_observation = Some("Please continue your task or provide a Final Answer if you are done.".to_string());
                                }
                                AgentDecision::Action { tool, args: _, kind: _ } => {
                                    let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                        message: format!("Executing tool: '{}'", tool)
                                    });
                                }
                                AgentDecision::MalformedAction(error) => {
                                    let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                        message: format!("⚠️ Malformed action: {}. Retrying...", error)
                                    });
                                    last_observation = Some(format!("Error: {}. Please follow the correct format.", error));
                                }
                                AgentDecision::Error(e) => {
                                    let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                        message: format!("❌ Agent error: {}", e)
                                    });
                                    return Ok((format!("Error: {}", e), self.total_usage.clone()));
                                }
                            }
                        }
                        Err(e) => {
                            recovery_attempts += 1;
                            if recovery_attempts > 3 {
                                let error_msg = format!("❌ Recovery failed after 3 attempts. Last error: {}", e);
                                let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                    message: error_msg.clone()
                                });
                                return Ok((error_msg, self.total_usage.clone()));
                            }

                            let recovery_msg = format!("⚠️ Hard Error detected: {}. Entering Recovery Mode (60s cooldown). Attempt {}/3...", e, recovery_attempts);
                            let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                message: recovery_msg.clone()
                            });

                            tokio::select! {
                                _ = tokio::time::sleep(tokio::time::Duration::from_secs(60)) => {}
                                _ = interrupt_rx.recv() => {
                                    let message = "🛑 Execution interrupted by user during recovery.".to_string();
                                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });
                                    return Ok((message, self.total_usage.clone()));
                                }
                            }
                            continue;
                        }
                    }
                }

                _ = interrupt_rx.recv() => {
                    let message = "🛑 Execution interrupted by user.".to_string();
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });
                    return Ok((message, self.total_usage.clone()));
                }
            }
        }
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
