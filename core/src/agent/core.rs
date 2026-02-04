//! Agent Core Implementation (Legacy Agent)
//!
//! This is the legacy Agent implementation (pre-V2). For new code, consider using AgentV2.
//!
//! This module has been refactored to use:
//! - `protocol` module for parsing Short-Key, ReAct, and tool call formats
//! - `execution` module for tool execution and approval flow
//! - `context` module for history pruning and memory injection

use crate::agent::tool::{Tool, ToolKind};
use crate::agent::toolcall_log;
use crate::agent::protocol::{parse_short_key_action_from_content, parse_react_action, truncate_for_log};
use crate::agent::context::{prune_history, build_context_from_memories};
use crate::agent::execution::{process_tool_args, is_final_response, build_continue_nudge, build_format_nudge};
use crate::llm::{chat::{ChatMessage, ChatRequest, MessageRole}, LlmClient, TokenUsage};
use crate::memory::MemoryCategorizer;
use crate::terminal::app::TuiEvent;
use std::error::Error as StdError;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, mpsc::{UnboundedSender, Receiver}};

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

/// The core Agent that manages the agentic loop.
pub struct Agent {
    pub llm_client: Arc<LlmClient>,
    pub tool_registry: crate::agent::ToolRegistry,
    pub max_iterations: usize,
    pub system_prompt_prefix: String,
    pub memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    pub categorizer: Option<Arc<MemoryCategorizer>>,
    pub scribe: Option<Arc<crate::memory::scribe::Scribe>>,
    pub job_registry: crate::agent::v2::jobs::JobRegistry,
    pub session_id: String,
    pub version: crate::config::AgentVersion,
    pub scratchpad: Option<Arc<RwLock<String>>>,
    /// Disable memory recall and hot memory injection (incognito mode)
    pub disable_memory: bool,
    /// Optional permission controls for this agent
    pub permissions: Option<crate::config::v2::types::AgentPermissions>,
    
    // State maintained between steps
    pub history: Vec<ChatMessage>,
    pub iteration_count: usize,
    pub total_usage: TokenUsage,
    pub pending_decision: Option<AgentDecision>,
    
    // Safety tracking
    last_tool_call: Option<(String, String)>,
    repetition_count: usize,
    pending_tool_call_id: Option<String>,
}

impl Agent {
    pub async fn new_with_iterations(
        client: Arc<LlmClient>,
        tools: Vec<Arc<dyn Tool>>,
        system_prompt_prefix: String,
        max_iterations: usize,
        version: crate::config::AgentVersion,
        memory_store: Option<Arc<crate::memory::store::VectorStore>>,
        categorizer: Option<Arc<MemoryCategorizer>>,
        job_registry: Option<crate::agent::v2::jobs::JobRegistry>,
        scratchpad: Option<Arc<RwLock<String>>>,
        disable_memory: bool,
        permissions: Option<crate::config::v2::types::AgentPermissions>,
    ) -> Self {
        let tool_registry = crate::agent::ToolRegistry::new();
        
        for tool in tools {
            let _ = tool_registry.register_tool_arc(tool).await;
        }

        let session_id = chrono::Utc::now().timestamp_millis().to_string();
        let job_registry = job_registry.unwrap_or_default();
        
        // Initialize Scribe for V2 if store is available
        let scribe = if version == crate::config::AgentVersion::V2 {
            if let Some(store) = &memory_store {
                let journal = Arc::new(Mutex::new(crate::memory::journal::Journal::new().unwrap()));
                Some(Arc::new(crate::memory::scribe::Scribe::new(
                    journal,
                    store.clone(),
                    client.clone()
                )))
            } else {
                None
            }
        } else {
            None
        };

        Self {
            llm_client: client,
            tool_registry,
            max_iterations,
            system_prompt_prefix,
            categorizer,
            memory_store,
            scribe,
            job_registry,
            session_id,
            history: Vec::new(),
            iteration_count: 0,
            total_usage: TokenUsage::default(),
            pending_decision: None,
            last_tool_call: None,
            repetition_count: 0,
            pending_tool_call_id: None,
            version,
            scratchpad,
            disable_memory,
            permissions,
        }
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
        self.pending_tool_call_id = None;

        // Ensure system prompt is present
        if self.history.is_empty() || self.history[0].role != MessageRole::System {
            let prompt = self.generate_system_prompt().await;
            self.history.insert(0, ChatMessage::system(prompt));
        }
    }

    /// Perform a single step in the agentic loop.
    pub async fn step(&mut self, observation: Option<String>) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
        // 1. Hard Iteration Limit Check
        if self.iteration_count >= self.max_iterations {
            crate::info_log!(
                "Agent step halted: iteration limit reached (iteration_count={}, max_iterations={}, had_pending_decision={})",
                self.iteration_count,
                self.max_iterations,
                self.pending_decision.is_some()
            );

            // Clear any queued tool action so driver loops don't interpret this as "keep going".
            self.pending_decision = None;
            self.pending_tool_call_id = None;

            return Ok(AgentDecision::Message(
                format!(
                    "⚠️ Maximum iteration limit ({}) reached. I've paused to prevent an infinite loop. You can continue by asking me to keep going.",
                    self.max_iterations
                ),
                self.total_usage.clone(),
            ));
        }

        // 2. Return pending decision if we have one (usually an Action queued after a Thought)
        if let Some(decision) = self.pending_decision.take() {
            return Ok(decision);
        }

        // 3. Add observation to history if provided
        if let Some(obs) = observation {
            if let Some(tool_id) = self.pending_tool_call_id.take() {
                // Respond to the specific tool call
                let tool_name = self.last_tool_call.as_ref().map(|(n, _)| n.clone()).unwrap_or_else(|| "unknown".to_string());
                self.history.push(ChatMessage::tool(tool_id, tool_name, obs));
            } else {
                // Legacy ReAct observation
                self.history.push(ChatMessage::user(format!("Observation: {}", obs)));
            }
        }

        // 4. Context Pruning
        let response_reserve = self.llm_client.config().max_tokens.unwrap_or(1000) as usize;
        let context_limit = self.llm_client.config().max_context_tokens.saturating_sub(response_reserve);
        self.history = prune_history(self.history.clone(), context_limit);

        // 5. Prepare and send LLM request
        let mut request = ChatRequest::new(self.llm_client.model().to_string(), self.history.clone());
        
        // Provide tool definitions if supported
        let tool_definitions = self.tool_registry.get_tool_definitions().await;
        if !tool_definitions.is_empty() {
            request = request.with_tools(tool_definitions);
        }

        let response = self.llm_client.chat(&request).await?;
        let content = response.content();

        crate::debug_log!("LLM Response: {}", content);

        // 6. Log the response
        self.log_llm_response(&response, &content).await;

        // 7. Update usage tracking
        if let Some(usage) = &response.usage {
            self.total_usage.prompt_tokens += usage.prompt_tokens;
            self.total_usage.completion_tokens += usage.completion_tokens;
            self.total_usage.total_tokens += usage.total_tokens;
        }

        self.iteration_count += 1;

        // 8. Process the decision from LLM response
        self.process_decision(content, response).await
    }

    /// Log LLM response for debugging
    async fn log_llm_response(&self, response: &crate::llm::ChatResponse, content: &str) {
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
            "content": truncate_for_log(content, 4000),
            "tool_calls": tool_calls_json,
        }));
    }

    /// Process the decision from LLM response
    async fn process_decision(
        &mut self,
        content: String,
        response: crate::llm::ChatResponse,
    ) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
        // 1. Handle Short-Key JSON Protocol (Preferred)
        if let Some(sk_action) = parse_short_key_action_from_content(&content) {
            return self.handle_short_key_decision(content, sk_action).await;
        }

        // 2. Handle Tool Calls (Modern API)
        if let Some(tool_calls) = response.choices[0].message.tool_calls.as_ref() {
            if !tool_calls.is_empty() {
                return self.handle_native_tool_call(content, response).await;
            }
        }

        // 3. Handle ReAct format
        if let Some(react_action) = parse_react_action(&content) {
            return self.handle_react_decision(content, react_action).await;
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

    /// Handle Short-Key protocol decision
    async fn handle_short_key_decision(
        &mut self,
        content: String,
        sk_action: crate::agent::protocol::ShortKeyAction,
    ) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
        crate::info_log!("Parsed Action (Short-Key): {:?}", sk_action);

        let thought = sk_action.thought.clone();

        // Handle final answer
        if let Some(final_answer) = sk_action.final_answer {
            self.history.push(ChatMessage::assistant(content.clone()));
            return Ok(AgentDecision::Message(
                format!("Thought: {}\nFinal Answer: {}", thought, final_answer),
                self.total_usage.clone(),
            ));
        }

        // Handle tool call
        if let Some(raw_tool_name) = sk_action.action {
            let tool_name = crate::agent::tool_registry::normalize_tool_name(&raw_tool_name).to_string();

            let args = sk_action
                .input
                .map(|v| {
                    if v.is_string() {
                        v.as_str().unwrap().to_string()
                    } else {
                        v.to_string()
                    }
                })
                .unwrap_or_default();

            // Repetition Check
            self.check_repetition(&tool_name, &args)?;

            let kind = self.tool_registry.get_tool_kind(&tool_name).await
                .unwrap_or(ToolKind::Internal);

            self.history.push(ChatMessage::assistant(content.clone()));

            let action = AgentDecision::Action {
                tool: tool_name.clone(),
                args: args.clone(),
                kind,
            };

            toolcall_log::append_jsonl_owned(serde_json::json!({
                "kind": "agent_decision",
                "session_id": self.session_id,
                "iteration": self.iteration_count,
                "decision": "tool_call_short_key",
                "tool": tool_name,
                "args": truncate_for_log(&args, 4000),
            }));

            // Queue the action and return thought first
            self.pending_decision = Some(action);
            return Ok(AgentDecision::Message(
                format!("Thought: {}", thought),
                self.total_usage.clone(),
            ));
        }

        // Just a thought with no action
        self.history.push(ChatMessage::assistant(content.clone()));
        Ok(AgentDecision::Message(thought, self.total_usage.clone()))
    }

    /// Handle native tool call (OpenAI-compatible)
    async fn handle_native_tool_call(
        &mut self,
        content: String,
        response: crate::llm::ChatResponse,
    ) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();
        let mut message = response.choices[0].message.clone();

        // For V1, we only support sequential tool calls
        if tool_calls.len() > 1 {
            crate::info_log!("Parallel tool calls detected. Truncating to 1 for V1 sequential enforcement.");
            if let Some(tc) = message.tool_calls.as_mut() {
                tc.truncate(1);
            }
        }

        let tool_call = &message.tool_calls.as_ref().expect("tool_calls should be present")[0];
        let raw_name = &tool_call.function.name;
        let tool_name = crate::agent::tool_registry::normalize_tool_name(raw_name).to_string();
        let args = tool_call.function.arguments.to_string();
        let tool_id = tool_call.id.clone();

        self.pending_tool_call_id = Some(tool_id);

        crate::info_log!("Parsed Action (Tool Call): {} with args: {}", tool_name, args);

        toolcall_log::append_jsonl_owned(serde_json::json!({
            "kind": "agent_decision",
            "session_id": self.session_id,
            "iteration": self.iteration_count,
            "decision": "tool_call_native",
            "tool": tool_name,
            "args": truncate_for_log(&args, 4000),
        }));

        // Repetition Check
        self.check_repetition(&tool_name, &args)?;

        let kind = self.tool_registry.get_tool_kind(&tool_name).await
            .unwrap_or(ToolKind::Internal);
        self.history.push(message);

        let action = AgentDecision::Action {
            tool: tool_name,
            args,
            kind,
        };

        // Check if there's also text content (Thoughts)
        if !content.trim().is_empty() {
            self.pending_decision = Some(action);
            return Ok(AgentDecision::Message(content, self.total_usage.clone()));
        }

        Ok(action)
    }

    /// Handle ReAct format decision
    async fn handle_react_decision(
        &mut self,
        content: String,
        react_action: crate::agent::protocol::ReActAction,
    ) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
        crate::info_log!("Parsed Action (ReAct): {} with args: {}", react_action.tool_name, react_action.args);

        // Repetition Check
        self.check_repetition(&react_action.tool_name, &react_action.args)?;

        // If the content also contains "Final Answer:", prioritize returning the whole thing as a message
        if react_action.has_final_answer {
            self.history.push(ChatMessage::assistant(content.clone()));
            return Ok(AgentDecision::Message(content, self.total_usage.clone()));
        }

        let kind = self.tool_registry.get_tool_kind(&react_action.tool_name).await
            .unwrap_or(ToolKind::Internal);
        self.history.push(ChatMessage::assistant(content.clone()));

        toolcall_log::append_jsonl_owned(serde_json::json!({
            "kind": "agent_decision",
            "session_id": self.session_id,
            "iteration": self.iteration_count,
            "decision": "tool_call_react",
            "tool": react_action.tool_name,
            "args": truncate_for_log(&react_action.args, 4000),
        }));

        let action_decision = AgentDecision::Action {
            tool: react_action.tool_name.clone(),
            args: react_action.args,
            kind,
        };

        // Extract everything before "Action:" as Thought
        if let Some(thought) = react_action.thought {
            if !thought.is_empty() {
                self.pending_decision = Some(action_decision);
                return Ok(AgentDecision::Message(thought, self.total_usage.clone()));
            }
        }

        Ok(action_decision)
    }

    /// Check for repeated tool calls and return error if detected
    fn check_repetition(&mut self, tool_name: &str, args: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if let Some((last_tool, last_args)) = &self.last_tool_call {
            if last_tool == tool_name && last_args == args {
                self.repetition_count += 1;
                if self.repetition_count >= 3 {
                    return Err(format!(
                        "Detected repeated tool call to '{}' with identical arguments. Breaking loop.",
                        tool_name
                    ).into());
                }
            } else {
                self.repetition_count = 0;
            }
        }
        self.last_tool_call = Some((tool_name.to_string(), args.to_string()));
        Ok(())
    }

    /// Legacy run method for backward compatibility.
    pub async fn run(
        &mut self,
        mut history: Vec<ChatMessage>,
        event_tx: UnboundedSender<TuiEvent>,
        interrupt_flag: Arc<AtomicBool>,
        auto_approve: bool,
        max_driver_loops: usize,
        mut approval_rx: Option<Receiver<bool>>,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        // Delegate to AgentV2 if using V2 version
        if self.version == crate::config::AgentVersion::V2 {
            return self.run_v2_bridge(
                history,
                event_tx,
                interrupt_flag,
                approval_rx,
            ).await;
        }

        // 1. Memory Context Injection (if enabled)
        self.inject_memory_context_internal(&mut history, &event_tx).await?;

        // 2. Reset agent state
        self.reset(history).await;
        
        let mut last_observation = None;
        let mut retry_count = 0;
        let max_retries = 3;

        // 3. Main execution loop
        for _loop_iteration in 1..=max_driver_loops {
            if interrupt_flag.load(Ordering::SeqCst) {
                return Ok(("Interrupted by user.".to_string(), self.total_usage.clone()));
            }

            let _ = event_tx.send(TuiEvent::StatusUpdate("Thinking...".to_string()));
            
            match self.step(last_observation.take()).await? {
                AgentDecision::Message(msg, usage) => {
                    retry_count = 0;
                    let _ = event_tx.send(TuiEvent::AgentResponse(msg.clone(), usage.clone()));
                    
                    // If we have a pending decision, continue immediately
                    if self.has_pending_decision() {
                        continue;
                    }

                    // Check if this is a final response
                    if is_final_response(&msg) {
                        let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                        return Ok((msg, usage));
                    }

                    // Nudge to continue
                    last_observation = Some(build_continue_nudge());
                }
                AgentDecision::Action { tool, args, kind } => {
                    retry_count = 0;
                    let _ = event_tx.send(TuiEvent::StatusUpdate(format!("Tool: '{}'", tool)));

                    // Handle approval
                    if !auto_approve {
                        match self.handle_approval_flow(&tool, &args, &event_tx, &mut approval_rx).await {
                            Ok(true) => {}, // Approved, continue
                            Ok(false) => {
                                // Denied
                                last_observation = Some(format!(
                                    "Error: User denied the execution of tool '{}'.",
                                    tool
                                ));
                                continue;
                            }
                            Err(e) => {
                                return Ok((e, self.total_usage.clone()));
                            }
                        }
                    }

                    // Execute the tool
                    let processed_args = process_tool_args(&args);
                    let observation = match self.tool_registry.execute_tool(&tool, &processed_args).await {
                        Ok(output) => output.as_string(),
                        Err(e) => {
                            let _ = event_tx.send(TuiEvent::StatusUpdate(format!("❌ Tool '{}' failed", tool)));
                            format!("Tool Error: {}. Analyze the failure and try a different approach.", e)
                        }
                    };

                    // Emit observation
                    if kind == ToolKind::Internal {
                        let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", observation.trim());
                        let _ = event_tx.send(TuiEvent::InternalObservation(obs_log.into_bytes()));
                    }

                    last_observation = Some(observation);
                }
                AgentDecision::MalformedAction(error) => {
                    retry_count += 1;
                    if retry_count > max_retries {
                        let fatal_error = format!(
                            "Fatal: Failed to parse agent response after {} attempts. Last error: {}",
                            max_retries, error
                        );
                        let _ = event_tx.send(TuiEvent::StatusUpdate(fatal_error.clone()));
                        return Ok((fatal_error, self.total_usage.clone()));
                    }

                    let _ = event_tx.send(TuiEvent::StatusUpdate(
                        format!("⚠️ {} Retrying ({}/{})", error, retry_count, max_retries)
                    ));
                    
                    last_observation = Some(build_format_nudge(&error));
                }
                AgentDecision::Error(e) => {
                    let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                    return Ok((format!("Error: {}", e), self.total_usage.clone()));
                }
            }
        }

        // Max driver loops exceeded
        Ok((
            format!("Error: Driver-level safety limit reached ({} loops).", max_driver_loops),
            self.total_usage.clone()
        ))
    }

    /// Handle V2 bridge - delegates to AgentV2
    async fn run_v2_bridge(
        &mut self,
        history: Vec<ChatMessage>,
        event_tx: UnboundedSender<TuiEvent>,
        interrupt_flag: Arc<AtomicBool>,
        approval_rx: Option<Receiver<bool>>,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        // 1. Initialize V2 Dependencies
        let store = self.memory_store.clone().ok_or("Memory store required for V2")?;
        let categorizer = self.categorizer.clone();
        let job_registry = self.job_registry.clone();
        let scribe = self.scribe.clone().ok_or("Scribe required for V2")?;
        
        // 2. Convert Tools
        let tools_list = self.tool_registry.get_all_tools().await;
        
        // 3. Create AgentV2
        let mut agent_v2 = crate::agent::v2::AgentV2::new_with_iterations(
            self.llm_client.clone(),
            scribe,
            tools_list,
            self.system_prompt_prefix.clone(),
            self.max_iterations,
            self.version,
            Some(store),
            categorizer,
            Some(job_registry),
            None, // capabilities_context
            None, // permissions
            self.scratchpad.clone(),
            self.disable_memory,
        );

        // 4. Bridge Channels
        let (v2_event_tx, mut v2_event_rx) = tokio::sync::mpsc::unbounded_channel::<crate::agent::event::RuntimeEvent>();
        let (interrupt_tx, interrupt_rx) = tokio::sync::mpsc::channel(1);
        let interrupt_flag_clone = interrupt_flag.clone();
        
        // Interrupt Watcher Task
        tokio::spawn(async move {
            loop {
                if interrupt_flag_clone.load(Ordering::SeqCst) {
                    let _ = interrupt_tx.send(()).await;
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        });

        // Event Translator Task
        let tui_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = v2_event_rx.recv().await {
                match event {
                    crate::agent::event::RuntimeEvent::StatusUpdate { message } => {
                        let _ = tui_tx.send(TuiEvent::StatusUpdate(message));
                    }
                    crate::agent::event::RuntimeEvent::AgentResponse { content, usage } => {
                        let _ = tui_tx.send(TuiEvent::AgentResponse(content, usage));
                    }
                    crate::agent::event::RuntimeEvent::InternalObservation { data } => {
                        let _ = tui_tx.send(TuiEvent::InternalObservation(data));
                    }
                    crate::agent::event::RuntimeEvent::SuggestCommand { command } => {
                        let _ = tui_tx.send(TuiEvent::SuggestCommand(command));
                    }
                    crate::agent::event::RuntimeEvent::ExecuteTerminalCommand { command, tx } => {
                        let _ = tui_tx.send(TuiEvent::ExecuteTerminalCommand(command, tx));
                    }
                    crate::agent::event::RuntimeEvent::GetTerminalScreen { tx } => {
                        let _ = tui_tx.send(TuiEvent::GetTerminalScreen(tx));
                    }
                    _ => {} // Ignore internal protocol events
                }
            }
        });

        // 5. Run V2
        let approval_rx_to_pass = approval_rx.unwrap_or_else(|| {
            let (_, rx) = tokio::sync::mpsc::channel(1);
            rx
        });

        agent_v2.run_event_driven(
            history,
            v2_event_tx,
            interrupt_rx,
            approval_rx_to_pass
        ).await
    }

    /// Inject memory context into history (internal helper for run method)
    async fn inject_memory_context_internal(
        &self,
        history: &mut Vec<ChatMessage>,
        event_tx: &UnboundedSender<TuiEvent>,
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if !self.llm_client.config().memory.auto_context {
            return Ok(());
        }
        
        if let Some(store) = &self.memory_store {
            if let Some(last_user_msg) = history.iter().rev().find(|m| m.role == MessageRole::User) {
                let _ = event_tx.send(TuiEvent::StatusUpdate("Searching memory...".to_string()));
                let memories = store.search_memory(&last_user_msg.content, 5).await.unwrap_or_default();
                if !memories.is_empty() {
                    let context = build_context_from_memories(&memories);
                    if let Some(user_idx) = history.iter().rposition(|m| m.role == MessageRole::User) {
                        history[user_idx].content.push('\n');
                        history[user_idx].content.push('\n');
                        history[user_idx].content.push_str(&context);
                    }
                }
            }
        }
        Ok(())
    }

    /// Handle approval flow for tool execution
    async fn handle_approval_flow(
        &self,
        tool: &str,
        args: &str,
        event_tx: &UnboundedSender<TuiEvent>,
        approval_rx: &mut Option<Receiver<bool>>,
    ) -> Result<bool, String> {
        // Provide a human-readable suggestion
        let suggestion = if tool == "execute_command" {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
                v.get("command")
                    .and_then(|c| c.as_str())
                    .or_else(|| v.get("args").and_then(|c| c.as_str()))
                    .unwrap_or(args)
                    .to_string()
            } else {
                args.to_string()
            }
        } else {
            format!("{} {}", tool, args)
        };
        
        let _ = event_tx.send(TuiEvent::SuggestCommand(suggestion));

        if let Some(rx) = approval_rx {
            let _ = event_tx.send(TuiEvent::StatusUpdate("Waiting for approval...".to_string()));
            
            match rx.recv().await {
                Some(true) => {
                    let _ = event_tx.send(TuiEvent::StatusUpdate("Approved.".to_string()));
                    Ok(true)
                }
                Some(false) => {
                    let _ = event_tx.send(TuiEvent::StatusUpdate("Denied.".to_string()));
                    Ok(false)
                }
                None => Err("Approval channel closed.".to_string()),
            }
        } else {
            Err(format!(
                "Approval required to run tool '{}' but no approval channel is available (AUTO-APPROVE is OFF).",
                tool
            ))
        }
    }

    /// Generate the system prompt with available tools and ReAct instructions.
    async fn generate_system_prompt(&self) -> String {
        use crate::config::{get_memory_protocol, get_react_protocol};

        let mut tools_desc = String::new();
        
        let tools = self.tool_registry.get_all_tools().await;
        for tool in tools {
            tools_desc.push_str(&format!("- {}: {}\n  Usage: {}\n", 
                tool.name(), 
                tool.description(), 
                tool.usage()
            ));
        }

        format!(
            "{}\n\n\
            # Available Tools\n\
            {}\n\n\
            {}\n\n\
            {}\n\n\
            Begin!",
            self.system_prompt_prefix,
            tools_desc,
            get_memory_protocol(),
            get_react_protocol()
        )
    }

    /// Automatically categorize a newly added memory.
    pub async fn auto_categorize(&self, memory_id: i64, content: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if let (Some(categorizer), Some(store)) = (&self.categorizer, &self.memory_store) {
            let category_id = categorizer.categorize_memory(content).await?;
            store.update_memory_category(memory_id, category_id.clone()).await?;
            let _ = categorizer.update_category_summary(&category_id).await;
        }
        Ok(())
    }

    /// Condense the conversation history by summarizing older messages.
    /// 
    /// This is a convenience wrapper around the context module's condense_history function.
    pub async fn condense_history(
        &self,
        history: &[ChatMessage],
    ) -> Result<Vec<ChatMessage>, Box<dyn StdError + Send + Sync>> {
        crate::agent::context::condense_history(history, &self.llm_client).await
    }

    /// Inject relevant memories into the conversation history.
    /// 
    /// This is the public API for memory context injection used by external callers.
    /// It operates on the agent's internal history.
    pub async fn inject_memory_context(&mut self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let auto_context = self.llm_client.config().memory.auto_context;
        crate::agent::context::inject_memory_context(
            &mut self.history,
            self.memory_store.as_ref(),
            auto_context,
        ).await
    }
}
