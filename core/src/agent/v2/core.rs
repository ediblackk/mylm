//! Agent V2 Core Implementation
use crate::llm::{LlmClient, chat::{ChatMessage, ChatRequest, MessageRole}, TokenUsage};
use crate::agent::tool::{Tool, ToolKind, ToolOutput};
use crate::agent::v2::jobs::{JobRegistry, JobStatus};
use crate::agent::event::RuntimeEvent;
use crate::agent::v2::recovery::{RecoveryWorker, RecoveryContext};
use crate::agent::protocol::{AgentRequest, AgentResponse, AgentError};
use crate::memory::{MemoryCategorizer, scribe::Scribe, journal::InteractionType};
use crate::terminal::app::TuiEvent;
use std::error::Error as StdError;
use serde::{Deserialize, Serialize};
use serde_json;
use std::sync::Arc;
use std::collections::HashMap;
use regex::Regex;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortKeyAction {
    #[serde(rename = "t")]
    pub thought: String,
    #[serde(rename = "a")]
    pub action: Option<String>,
    #[serde(rename = "i")]
    pub input: Option<serde_json::Value>,
    #[serde(rename = "f")]
    pub final_answer: Option<String>,
}

/// Try to parse one or more [`ShortKeyAction`] from arbitrary model output.
///
/// This is intentionally defensive because some models/proxies produce:
/// - fenced JSON blocks (```json ... ```)
/// - JSON embedded in surrounding prose
/// - invalid JSON with literal newlines inside string values
pub(crate) fn parse_short_key_actions_from_content(content: &str) -> Result<Vec<ShortKeyAction>, AgentError> {
    let trimmed = content.trim();

    // 1. Check for fenced JSON blocks first (most explicit)
    let fenced_blocks = extract_json_code_fence_blocks(content);
    for block in fenced_blocks {
        if let Some(actions) = parse_batch_or_single(&block) {
            return Ok(actions);
        }
    }

    // 2. Try parsing the whole trimmed content
    if let Some(actions) = parse_batch_or_single(trimmed) {
        return Ok(actions);
    }

    // 3. Extract balanced JSON objects or arrays
    let candidates = extract_balanced_json_structures(content);
    for c in candidates {
        if let Some(actions) = parse_batch_or_single(&c) {
            return Ok(actions);
        }
    }

    // 4. If nothing worked, return a Parse Error
    Err(AgentError {
        message: "Failed to parse Short-Key JSON from model response.".to_string(),
        code: Some("PARSE_ERROR".to_string()),
        context: Some(serde_json::json!({ "raw_content": content })),
    })
}

fn parse_batch_or_single(candidate: &str) -> Option<Vec<ShortKeyAction>> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try as array
    if let Ok(batch) = serde_json::from_str::<Vec<ShortKeyAction>>(trimmed) {
        return Some(batch);
    }

    // Try as single object
    if let Some(single) = parse_short_key_action_candidate(trimmed) {
        return Some(vec![single]);
    }

    // Try normalization for both
    let normalized = escape_unescaped_newlines_in_json_strings(trimmed);
    if let Ok(batch) = serde_json::from_str::<Vec<ShortKeyAction>>(&normalized) {
        return Some(batch);
    }
    if let Ok(single) = serde_json::from_str::<ShortKeyAction>(&normalized) {
        return Some(vec![single]);
    }

    None
}

fn parse_short_key_action_candidate(candidate: &str) -> Option<ShortKeyAction> {
    match serde_json::from_str::<ShortKeyAction>(candidate) {
        Ok(v) => Some(v),
        Err(_) => {
            // Some models output invalid JSON with literal newlines inside string values.
            // Normalize it into valid JSON and try again.
            let normalized = escape_unescaped_newlines_in_json_strings(candidate);
            serde_json::from_str::<ShortKeyAction>(&normalized).ok()
        }
    }
}

/// Extract ```json ... ``` blocks.
///
/// Important: we terminate only on a *closing fence line* that is exactly ``` (plus whitespace),
/// so occurrences of ``` inside JSON string values (e.g. markdown in `f`) won't truncate.
fn extract_json_code_fence_blocks(content: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let lower = content.to_lowercase();
    let mut search_from = 0usize;

    // Closing fence must be on its own line.
    let end_fence_re = Regex::new(r"(?m)^[ \t]*```[ \t]*$").expect("valid regex");

    while let Some(rel_start) = lower[search_from..].find("```json") {
        let fence_start = search_from + rel_start;
        let after_tag = fence_start + "```json".len();

        // Fenced content begins after the next newline.
        let content_start = match content[after_tag..].find('\n') {
            Some(rel_nl) => after_tag + rel_nl + 1,
            None => break,
        };

        let hay = &content[content_start..];
        if let Some(m) = end_fence_re.find(hay) {
            let end_fence_start = content_start + m.start();
            blocks.push(content[content_start..end_fence_start].to_string());
            search_from = content_start + m.end();
        } else {
            break;
        }
    }

    blocks
}

/// Extract top-level `{ ... }` or `[ ... ]` candidates by scanning with brace/bracket balancing.
///
/// Respects JSON strings and escapes, so braces inside strings don't affect balancing.
fn extract_balanced_json_structures(content: &str) -> Vec<String> {
    let mut out = Vec::new();

    let mut in_string = false;
    let mut escape = false;
    let mut brace_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut start: Option<usize> = None;

    for (i, ch) in content.char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if brace_depth == 0 && bracket_depth == 0 {
                    start = Some(i);
                }
                brace_depth += 1;
            }
            '}' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                    if brace_depth == 0 && bracket_depth == 0 {
                        if let Some(s) = start.take() {
                            out.push(content[s..=i].to_string());
                        }
                    }
                }
            }
            '[' => {
                if brace_depth == 0 && bracket_depth == 0 {
                    start = Some(i);
                }
                bracket_depth += 1;
            }
            ']' => {
                if bracket_depth > 0 {
                    bracket_depth -= 1;
                    if brace_depth == 0 && bracket_depth == 0 {
                        if let Some(s) = start.take() {
                            out.push(content[s..=i].to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    out
}

/// Convert invalid JSON containing literal newlines inside string values into valid JSON.
///
/// Only escapes `\n`/`\r` when inside a JSON string literal.
fn escape_unescaped_newlines_in_json_strings(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escape = false;

    for ch in input.chars() {
        if in_string {
            if escape {
                out.push(ch);
                escape = false;
                continue;
            }
            match ch {
                '\\' => {
                    out.push(ch);
                    escape = true;
                }
                '"' => {
                    out.push(ch);
                    in_string = false;
                }
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                _ => out.push(ch),
            }
        } else {
            out.push(ch);
            if ch == '"' {
                in_string = true;
            }
        }
    }

    out
}

/// The decision made by the agent after a step.
#[derive(Debug, Clone)]
pub enum AgentDecision {
    /// The LLM produced a text response (final answer or question).
    Message(String, TokenUsage),
    /// The LLM wants to execute a tool.
    Action {
        tool: String,
        args: String,
        kind: ToolKind,
    },
    /// The LLM output a tool call that couldn't be parsed correctly.
    MalformedAction(String),
    /// The agent has reached maximum iterations or an error occurred.
    Error(String),
}

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
    pub budget: usize,                    // Maximum number of steps allowed
    pub max_steps: usize,                 // Current step limit (can be increased)
    pub heartbeat_interval: std::time::Duration, // How often to poll for job updates
    pub safety_timeout: std::time::Duration,     // Maximum duration for autonomous run
}

impl AgentV2 {
    pub fn new_with_iterations(
        client: Arc<LlmClient>,
        scribe: Arc<Scribe>,
        tools: Vec<Box<dyn Tool>>,
        system_prompt_prefix: String,
        max_iterations: usize,
        version: crate::config::AgentVersion,
        memory_store: Option<Arc<crate::memory::store::VectorStore>>,
        categorizer: Option<Arc<MemoryCategorizer>>,
        job_registry: Option<JobRegistry>,
    ) -> Self {
        let mut tool_map = HashMap::new();
        for tool in tools {
            tool_map.insert(tool.name().to_string(), Arc::from(tool));
        }

        let session_id = chrono::Utc::now().timestamp_millis().to_string();

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
            budget: max_iterations,  // Default budget equals max_iterations
            max_steps: max_iterations, // Initial step limit
            heartbeat_interval: std::time::Duration::from_secs(5), // 5 second heartbeat
            safety_timeout: std::time::Duration::from_secs(300), // 5 minute safety timeout
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
    pub fn reset(&mut self, history: Vec<ChatMessage>) {
        self.history = history;
        self.iteration_count = 0;
        self.total_usage = TokenUsage::default();
        self.pending_decision = None;
        self.last_tool_call = None;
        self.repetition_count = 0;
        self.parse_failure_count = 0;
        self.pending_tool_call_id = None;
        self.max_steps = self.budget; // Reset step limit to budget

        // Ensure system prompt is present
        if self.history.is_empty() || self.history[0].role != MessageRole::System {
            self.history.insert(0, ChatMessage::system(self.generate_system_prompt()));
        }
    }

    /// Perform a single step in the agentic loop.
    pub async fn step(&mut self, observation: Option<String>) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
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
            let _ = self.scribe.observe(InteractionType::Output, &obs).await;

            if let Some(tool_id) = self.pending_tool_call_id.take() {
                let tool_name = self.last_tool_call.as_ref().map(|(n, _)| n.clone()).unwrap_or_else(|| "unknown".to_string());
                self.history.push(ChatMessage::tool(tool_id, tool_name, obs));
            } else {
                self.history.push(ChatMessage::user(format!("Observation: {}", obs)));
            }
        }

        // --- Context Pruning ---
        let response_reserve = self.llm_client.config().max_tokens.unwrap_or(1000) as usize;
        let context_limit = self.llm_client.config().max_context_tokens.saturating_sub(response_reserve);
        self.history = self.prune_history(self.history.clone(), context_limit);

        // --- Memory Recall ---
        let query = self.history.iter().rev()
            .find(|m| m.role == MessageRole::User)
            .map(|m| m.content.as_str())
            .unwrap_or("");
        
        let mut request_history = self.history.clone();
        if let Ok(recalled_context) = self.scribe.recall(query, 5).await {
            if !recalled_context.is_empty() {
                request_history.push(ChatMessage::system(format!(
                    "## Recalled Context (Long-term & Recent Memory)\n{}\nUse this context to inform your decisions.",
                    recalled_context
                )));
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

        // Log the thought/response
        let _ = self.scribe.observe(InteractionType::Thought, &content).await;

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
                            self.parse_failure_count = 0; // Reset after successful recovery
                            short_key_actions = Some(recovered_actions);
                        }
                        Err(rec_err) => {
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
                return Ok(AgentDecision::Message(content, self.total_usage.clone()));
            }

            // Convert ShortKeyAction to AgentRequest
            let mut agent_requests = Vec::new();
            for (idx, sk) in tool_actions.into_iter().enumerate() {
                let tool_name = sk.action.unwrap();
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
                    (tool_call.function.name.trim().to_string(), tool_call.function.arguments.to_string(), tool_call.id.clone())
                };
                self.pending_tool_call_id = Some(tool_id);
                self.last_tool_call = Some((tool_name.clone(), args.clone()));
                let kind = self.tools.get(&tool_name).map(|t| t.kind()).unwrap_or(ToolKind::Internal);
                self.history.push(message);
                let action = AgentDecision::Action { tool: tool_name, args, kind };
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

        if let (Some(tool_name), Some(args)) = (action_match.as_ref().map(|c| c[1].trim().to_string()), action_input_match.as_ref().map(|c| c[1].trim().to_string())) {
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
        Ok(AgentDecision::Message(content, self.total_usage.clone()))
    }

    async fn execute_parallel_tools(&self, requests: Vec<AgentRequest>) -> Result<Vec<AgentResponse>, Box<dyn StdError + Send + Sync>> {
        let mut futures = Vec::new();

        for req in requests {
            let event_tx = self.event_tx.clone();
            let tool = self.tools.get(&req.action).cloned();
            let scribe = self.scribe.clone();
            
            futures.push(async move {
                // Emit Step event
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

                        // Log tool call
                        let _ = scribe.observe(InteractionType::Tool, &format!("Action: {}\nInput: {}", req.action, args)).await;

                        match t.call(&args).await {
                            Ok(output) => {
                                let output_str = output.as_string();
                                let _ = scribe.observe(InteractionType::Output, &output_str).await;
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
                                let _ = scribe.observe(InteractionType::Output, &format!("Error: {}", error_msg)).await;
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

                // Emit ToolOutput event
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
        if !self.llm_client.config().memory.auto_context {
            return Ok(());
        }

        if let Some(store) = &self.memory_store {
            // Find the last user message to use as a search query
            if let Some(last_user_msg) = self.history.iter().rev().find(|m| m.role == MessageRole::User) {
                let memories = store.search_memory(&last_user_msg.content, 5).await.unwrap_or_default();
                if !memories.is_empty() {
                    let context = self.build_context_from_memories(&memories);
                    // Append context to the last user message
                    if let Some(user_idx) = self.history.iter().rposition(|m| m.role == MessageRole::User) {
                        self.history[user_idx].content.push_str("\n\n");
                        self.history[user_idx].content.push_str(&context);
                    }
                }
            }
        }
        Ok(())
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
                        // Inject context appended to user msg (Context Pack style)
                        if let Some(user_idx) = history.iter().rposition(|m| m.role == MessageRole::User) {
                            history[user_idx].content.push_str("\n\n");
                            history[user_idx].content.push_str(&context);
                        }
                    }
                }
            }
        }

        self.reset(history);
        
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
                    retry_count = 0; // Reset on successful message
                    let _ = event_tx.send(TuiEvent::AgentResponse(msg.clone(), usage.clone()));
                    
                    // If we have a pending decision (like an Action queued after a Thought),
                    // continue the loop immediately to execute it.
                    if self.has_pending_decision() {
                        continue;
                    }

                    // For "Autonomous" mode, we look for "Final Answer:" or JSON equivalent to know when to stop.
                    if msg.contains("Final Answer:") || msg.contains("\"f\":") || msg.contains("\"final_answer\":") {
                        let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                        return Ok((msg, usage));
                    }
                    
                    // If we get a message without "Final Answer:" and we're in an autonomous loop,
                    // we should stop if it looks like a direct response to the user or a request for info.
                    // Common indicators that the model is talking to the user:
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

                    // If it's a non-empty message and no tool was called, and it's not a tiny nudge,
                    // we assume it's a response to the user.
                    if msg.len() > 30 {
                        let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                        return Ok((msg, usage));
                    }

                    // Nudge to continue only if it's very short or we suspect it's stuck/narrating
                    last_observation = Some("Please continue your task or provide a Final Answer if you are done.".to_string());
                    continue;
                }
                AgentDecision::Action { tool, args, kind } => {
                    retry_count = 0; // Reset on successful action
                    let _ = event_tx.send(TuiEvent::StatusUpdate(format!("Tool: '{}'", tool)));

                    // Approval gating (AUTO-APPROVE OFF)
                    //
                    // Requirement: every tool call must be approved when auto-approve is OFF.
                    // Headless callers may not provide an approval channel; in that case, we MUST halt
                    // before executing the tool.
                    if !auto_approve {
                        // Provide a human-readable suggestion of what would run.
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
                            // For non-shell tools, show tool+args (keeps UI event semantics stable).
                            format!("{} {}", tool, args)
                        };
                        let _ = event_tx.send(TuiEvent::SuggestCommand(suggestion));

                        if let Some(rx) = &mut approval_rx {
                            // Wait for approval (one tool execution == one approval)
                            let _ = event_tx.send(TuiEvent::StatusUpdate("Waiting for approval...".to_string()));
                            match rx.recv().await {
                                Some(true) => {
                                    let _ = event_tx.send(TuiEvent::StatusUpdate("Approved.".to_string()));
                                    // Proceed to execution
                                }
                                Some(false) => {
                                    let _ = event_tx.send(TuiEvent::StatusUpdate("Denied.".to_string()));
                                    last_observation = Some(format!(
                                        "Error: User denied the execution of tool '{}'.",
                                        tool
                                    ));
                                    continue;
                                }
                                None => {
                                    return Ok((
                                        "Error: Approval channel closed.".to_string(),
                                        self.total_usage.clone(),
                                    ));
                                }
                            }
                        } else {
                            // Legacy/headless behavior: halt and return control to the caller.
                            return Ok((
                                format!(
                                    "Approval required to run tool '{}' but no approval channel is available (AUTO-APPROVE is OFF).",
                                    tool
                                ),
                                self.total_usage.clone(),
                            ));
                        }
                    }

                    // Extract arguments from JSON if necessary (for tool calling API)
                    let processed_args = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                        v.get("args")
                         .and_then(|a| a.as_str())
                         .map(|s| s.to_string())
                         .unwrap_or(args.clone())
                    } else {
                        args.clone()
                    };

                    // Log tool call
                    let _ = self.scribe.observe(InteractionType::Tool, &format!("Action: {}\nInput: {}", tool, processed_args)).await;

                    let observation = match self.tools.get(&tool) {
                        Some(t) => match t.call(&processed_args).await {
                            Ok(output) => {
                                let output_str = output.as_string();
                                let _ = self.scribe.observe(InteractionType::Output, &output_str).await;
                                output_str
                            },
                            Err(e) => {
                                let error_msg = format!("Tool Error: {}. Analyze the failure and try a different command or approach if possible.", e);
                                let _ = event_tx.send(TuiEvent::StatusUpdate(format!("❌ Tool '{}' failed", tool)));
                                let _ = self.scribe.observe(InteractionType::Output, &error_msg).await;
                                error_msg
                            },
                        },
                        None => {
                            let error_msg = format!("Error: Tool '{}' not found. Check the available tools list.", tool);
                            let _ = self.scribe.observe(InteractionType::Output, &error_msg).await;
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
                    
                    // Nudge the model to follow the format
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

    /// Prune history to stay within token limits.
    fn prune_history(&self, history: Vec<ChatMessage>, limit: usize) -> Vec<ChatMessage> {
        if history.len() <= 1 {
            return history;
        }

        let mut total_chars = 0;
        for msg in &history {
            total_chars += msg.content.len();
        }

        let approx_tokens = total_chars / 4;
        if approx_tokens <= limit {
            return history;
        }

        let system_msg = history[0].clone();
        let mut pruned = Vec::new();
        pruned.push(system_msg.clone());

        let mut current_tokens = system_msg.content.len() / 4;
        let mut to_keep = Vec::new();

        // Iterate backwards to keep most recent messages
        for msg in history.iter().skip(1).rev() {
            let msg_tokens = msg.content.len() / 4;
            if current_tokens + msg_tokens < limit {
                to_keep.push(msg.clone());
                current_tokens += msg_tokens;
            } else {
                break;
            }
        }

        to_keep.reverse();

        // Gemini/Strict API Requirement: Ensure we don't start with an Assistant/Tool message
        // after the system prompt.
        while !to_keep.is_empty() && to_keep[0].role != MessageRole::User {
            to_keep.remove(0);
        }

        pruned.extend(to_keep);
        pruned
    }

    /// Condense the conversation history by summarizing older messages.
    pub async fn condense_history(&self, history: &[ChatMessage]) -> Result<Vec<ChatMessage>, Box<dyn StdError + Send + Sync>> {
        if history.len() <= 5 {
            return Ok(history.to_vec());
        }

        let system_prompt = if !history.is_empty() && history[0].role == MessageRole::System {
            Some(history[0].clone())
        } else {
            None
        };

        let to_summarize = &history[1..history.len() - 3];
        let latest = &history[history.len() - 3..];

        let mut summary_input = String::from("Summarize the following conversation history into a concise summary that preserves all key facts, decisions, and context for an AI assistant to continue the task:\n\n");
        for msg in to_summarize {
            summary_input.push_str(&format!("{}: {}\n", match msg.role {
                MessageRole::System => "System",
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
                MessageRole::Tool => "Tool",
            }, msg.content));
        }

        let summary_request = ChatRequest::new(
            self.llm_client.model().to_string(),
            vec![
                ChatMessage::system("You are a helpful assistant that summarizes technical conversations."),
                ChatMessage::user(&summary_input),
            ],
        );

        let response = self.llm_client.chat(&summary_request).await?;
        let summary = response.content();

        let mut new_history = Vec::new();
        if let Some(sys) = system_prompt {
            new_history.push(sys);
        }
        new_history.push(ChatMessage::assistant(format!("[Context Summary]: {}", summary)));
        new_history.extend_from_slice(latest);

        Ok(new_history)
    }

    /// Generate the system prompt with available tools and Short-Key JSON instructions.
    fn generate_system_prompt(&self) -> String {
        let mut tools_desc = String::new();
        for tool in self.tools.values() {
            tools_desc.push_str(&format!("- {}: {}\n  Usage: {}\n", tool.name(), tool.description(), tool.usage()));
        }

        format!(
            "{}\n\n\
            # Available Tools\n\
            {}\n\n\
            # Response Format: Short-Key JSON Protocol\n\
            You MUST respond using the Short-Key JSON protocol. This format minimizes token usage and ensures structural integrity.\n\n\
            ## Schema\n\
            - `t`: Thought. Your internal reasoning and next steps.\n\
            - `a`: Action. The name of the tool to execute (optional if providing final answer).\n\
            - `i`: Input. The arguments for the tool in strict JSON format (optional).\n\
            - `f`: Final Answer. Your final response to the user (optional).\n\n\
            ## Examples\n\
            ### Single Tool Call\n\
            ```json\n\
            {{\"t\": \"I need to list files in the current directory.\", \"a\": \"execute_command\", \"i\": \"ls\"}}\n\
            ```\n\n\
            ### Parallel Tool Calls\n\
            You can execute multiple tools in parallel by returning an array of objects. Use this for independent operations like reading multiple files or searching different sources.\n\
            ```json\n\
            [\n\
              {{\"t\": \"Checking config...\", \"a\": \"execute_command\", \"i\": \"cat config.json\"}},\n\
              {{\"t\": \"Checking logs...\", \"a\": \"execute_command\", \"i\": \"tail -n 20 error.log\"}}\n\
            ]\n\
            ```\n\n\
            ### Final Answer\n\
            ```json\n\
            {{\"t\": \"I have completed the task.\", \"f\": \"The project has been successfully initialized.\"}}\n\
            ```\n\n\
            IMPORTANT: Always wrap your JSON in a code block or return it as the raw response. Ensure all tool inputs are valid JSON objects.\n\n\
            Begin!",
            self.system_prompt_prefix,
            tools_desc
        )
    }

    fn build_context_from_memories(&self, memories: &[crate::memory::store::Memory]) -> String {
        if memories.is_empty() {
            return String::new();
        }
        
        let mut context = String::from("## Relevant Past Operations & Knowledge\n");
        for (i, mem) in memories.iter().enumerate() {
            let timestamp = chrono::DateTime::from_timestamp(mem.created_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "unknown time".to_string());
            
            context.push_str(&format!(
                "{}. [{}] {} ({})\n",
                i + 1,
                mem.r#type,
                mem.content,
                timestamp,
            ));
        }
        context.push_str("\nUse this context to inform your actions and avoid repeating mistakes.");
        context
    }

    /// Automatically categorize a newly added memory.
    pub async fn auto_categorize(&self, memory_id: i64, content: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if let (Some(categorizer), Some(store)) = (&self.categorizer, &self.memory_store) {
            let category_id = categorizer.categorize_memory(content).await?;
            store.update_memory_category(memory_id, category_id.clone()).await?;
            // Update summary for the category
            let _ = categorizer.update_category_summary(&category_id).await;
        }
        Ok(())
    }

    /// Event-driven run method with heartbeat loop and budget management.
    pub async fn run_event_driven(
        &mut self,
        history: Vec<ChatMessage>,
        event_tx: tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
        mut interrupt_rx: tokio::sync::mpsc::Receiver<()>,
        mut approval_rx: tokio::sync::mpsc::Receiver<bool>,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        // Reset state for new task
        self.reset(history);
        
        let start_time = std::time::Instant::now();
        let heartbeat_interval = self.heartbeat_interval;
        let safety_timeout = self.safety_timeout;
        
        let mut last_observation = None;
        let mut heartbeat_timer = tokio::time::interval(heartbeat_interval);
        heartbeat_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        
        loop {
            // Check safety timeout
            if start_time.elapsed() > safety_timeout {
                let message = format!("⚠️ Safety timeout reached ({:?}). Stopping autonomous run.", safety_timeout);
                let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });
                return Ok((message, self.total_usage.clone()));
            }
            
            // Check budget enforcement
            if self.iteration_count >= self.max_steps {
                let message = format!("⚠️ Step budget exceeded ({}/{}). Requesting permission to continue...",
                    self.iteration_count, self.max_steps);
                let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });
                
                // Wait for approval to continue
                match approval_rx.recv().await {
                    Some(true) => {
                        // Increase budget by 50% and continue
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
                // Heartbeat: poll for background job updates
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
                                    
                                    // Feed result back into agent context
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
                                    
                                    // Feed error back into agent context
                                    let observation = format!("Background job '{}' failed: {}", job.description, error_msg);
                                    last_observation = Some(observation);
                                }
                                JobStatus::Running => {
                                    // Still running, could send status update if needed
                                    let message = format!("⏳ Background job '{}' is still running...", job.description);
                                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message });
                                }
                            }
                        }
                    }
                }
                
                // Main cognitive step
                result = self.step(last_observation.take()) => {
                    match result {
                        Ok(decision) => {
                            match decision {
                                AgentDecision::Message(msg, usage) => {
                                    let _ = event_tx.send(RuntimeEvent::AgentResponse {
                                        content: msg.clone(),
                                        usage: usage.clone()
                                    });
                                    
                                    // Check for final answer indicators
                                    if msg.contains("Final Answer:") || msg.contains("\"f\":") || msg.contains("\"final_answer\":") {
                                        let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                            message: "Task completed successfully.".to_string()
                                        });
                                        return Ok((msg, usage));
                                    }
                                    
                                    // Check if this looks like a user-facing response
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
                                    
                                    // Continue with a nudge
                                    last_observation = Some("Please continue your task or provide a Final Answer if you are done.".to_string());
                                }
                                AgentDecision::Action { tool, args: _, kind: _ } => {
                                    let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                        message: format!("Executing tool: '{}'", tool)
                                    });
                                    
                                    // For now, execute the tool directly (could be made async in future)
                                    // This will be handled by the step() method's internal tool execution
                                    // The observation will be processed in the next iteration
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
                            let error_msg = format!("❌ Step failed: {}", e);
                            let _ = event_tx.send(RuntimeEvent::StatusUpdate {
                                message: error_msg.clone()
                            });
                            return Ok((error_msg, self.total_usage.clone()));
                        }
                    }
                }
                
                // Handle interrupt signal
                _ = interrupt_rx.recv() => {
                    let message = "🛑 Execution interrupted by user.".to_string();
                    let _ = event_tx.send(RuntimeEvent::StatusUpdate { message: message.clone() });
                    return Ok((message, self.total_usage.clone()));
                }
            }
        }
    }
}
