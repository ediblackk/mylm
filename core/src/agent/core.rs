//! Agent Core Implementation
use crate::llm::{LlmClient, chat::{ChatMessage, ChatRequest, MessageRole}, TokenUsage};
use crate::agent::tool::{Tool, ToolKind};
use crate::memory::{MemoryCategorizer};
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
    pub tools: HashMap<String, Box<dyn Tool>>,
    pub max_iterations: usize,
    pub system_prompt_prefix: String,
    pub memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    pub categorizer: Option<Arc<MemoryCategorizer>>,
    pub session_id: String,
    pub version: crate::config::AgentVersion,
    
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
    pub fn new_with_iterations(
        client: Arc<LlmClient>,
        tools: Vec<Box<dyn Tool>>,
        system_prompt_prefix: String,
        max_iterations: usize,
        version: crate::config::AgentVersion,
        memory_store: Option<Arc<crate::memory::store::VectorStore>>,
        categorizer: Option<Arc<MemoryCategorizer>>,
    ) -> Self {
        let mut tool_map = HashMap::new();
        for tool in tools {
            tool_map.insert(tool.name().to_string(), tool);
        }

        let session_id = chrono::Utc::now().timestamp_millis().to_string();

        Self {
            llm_client: client,
            tools: tool_map,
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
        }
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
        self.pending_tool_call_id = None;

        // Ensure system prompt is present
        if self.history.is_empty() || self.history[0].role != MessageRole::System {
            self.history.insert(0, ChatMessage::system(self.generate_system_prompt()));
        }
    }

    /// Perform a single step in the agentic loop.
    pub async fn step(&mut self, observation: Option<String>) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
        // 0. Version Switch
        if self.version == crate::config::AgentVersion::V2 {
            return Ok(AgentDecision::Message(
                "V2 Agent Loop is active (Placeholder). To use full V2 capabilities, ensure 'mylm-v2' is installed correctly.".to_string(),
                self.total_usage.clone()
            ));
        }

        // 1. Hard Iteration Limit Check
        if self.iteration_count >= self.max_iterations {
            return Ok(AgentDecision::Message(
                format!("⚠️ Maximum iteration limit ({}) reached. I've paused to prevent an infinite loop. You can continue by asking me to keep going.", self.max_iterations),
                self.total_usage.clone()
            ));
        }

        // 2. Return pending decision if we have one (usually an Action queued after a Thought)
        if let Some(decision) = self.pending_decision.take() {
            return Ok(decision);
        }

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

        // --- Context Pruning ---
        // Reserve space for the response (default 1000 if not specified, or config max_tokens)
        let response_reserve = self.llm_client.config().max_tokens.unwrap_or(1000) as usize;
        let context_limit = self.llm_client.config().max_context_tokens.saturating_sub(response_reserve);
        
        self.history = self.prune_history(self.history.clone(), context_limit);

        let mut request = ChatRequest::new(self.llm_client.model().to_string(), self.history.clone());
        
        // Provide tool definitions if supported
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

        crate::debug_log!("LLM Response: {}", content);

        if let Some(usage) = &response.usage {
            self.total_usage.prompt_tokens += usage.prompt_tokens;
            self.total_usage.completion_tokens += usage.completion_tokens;
            self.total_usage.total_tokens += usage.total_tokens;
        }

        self.iteration_count += 1;

        // --- Process Decision ---

        // 1. Handle Short-Key JSON Protocol (V2)
        // Look for JSON blocks anywhere in the content.
        let json_blocks: Vec<&str> = if content.contains("```json") {
            content.split("```json")
                .skip(1)
                .filter_map(|s| s.split("```").next())
                .map(|s| s.trim())
                .collect()
        } else {
            // Find all potential { ... } blocks
            let mut blocks = Vec::new();
            let mut start_idx = 0;
            while let Some(s) = content[start_idx..].find('{') {
                let actual_start = start_idx + s;
                if let Some(e) = content[actual_start..].rfind('}') {
                    let actual_end = actual_start + e;
                    blocks.push(&content[actual_start..=actual_end]);
                    start_idx = actual_end + 1;
                } else {
                    break;
                }
            }
            blocks
        };

        for block in json_blocks {
            if let Ok(sk_action) = serde_json::from_str::<ShortKeyAction>(block) {
                crate::info_log!("Parsed Action (Short-Key): {:?}", sk_action);
                
                let thought = sk_action.thought.clone();
                
                if let Some(final_answer) = sk_action.final_answer {
                    self.history.push(ChatMessage::assistant(content.clone()));
                    return Ok(AgentDecision::Message(format!("Thought: {}\nFinal Answer: {}", thought, final_answer), self.total_usage.clone()));
                }

                if let Some(tool_name) = sk_action.action {
                    let tool_name = tool_name.trim();
                    let args = sk_action.input.map(|v| {
                        if v.is_string() {
                            v.as_str().unwrap().to_string()
                        } else {
                            v.to_string()
                        }
                    }).unwrap_or_default();

                    // Repetition Check
                    if let Some((last_tool, last_args)) = &self.last_tool_call {
                        if last_tool == tool_name && last_args == &args {
                            self.repetition_count += 1;
                            if self.repetition_count >= 3 {
                                return Ok(AgentDecision::Error(format!("Detected repeated tool call to '{}' with identical arguments. Breaking loop.", tool_name)));
                            }
                        } else {
                            self.repetition_count = 0;
                        }
                    }
                    self.last_tool_call = Some((tool_name.to_string(), args.clone()));

                    let kind = self.tools.get(tool_name).map(|t| t.kind()).unwrap_or(ToolKind::Internal);
                    
                    // Add thought to history if it's substantial
                    self.history.push(ChatMessage::assistant(content.clone()));

                    let action = AgentDecision::Action {
                        tool: tool_name.to_string(),
                        args,
                        kind,
                    };

                    self.pending_decision = Some(action);
                    return Ok(AgentDecision::Message(format!("Thought: {}", thought), self.total_usage.clone()));
                }
            }
        }

        // 2. Handle Tool Calls (Modern API)
        if let Some(tool_calls) = response.choices[0].message.tool_calls.as_ref() {
            if let Some(tool_call) = tool_calls.first() {
                let tool_name = tool_call.function.name.trim();
                let args = &tool_call.function.arguments;
                
                // Store the ID for the next step
                self.pending_tool_call_id = Some(tool_call.id.clone());

                crate::info_log!("Parsed Action (Tool Call): {} with args: {}", tool_name, args);
                
                // Repetition Check
                if let Some((last_tool, last_args)) = &self.last_tool_call {
                    if last_tool == tool_name && last_args == args {
                        self.repetition_count += 1;
                        if self.repetition_count >= 3 {
                            return Ok(AgentDecision::Error(format!("Detected repeated tool call to '{}' with identical arguments. Breaking loop.", tool_name)));
                        }
                    } else {
                        self.repetition_count = 0;
                    }
                }
                self.last_tool_call = Some((tool_name.to_string(), args.to_string()));

                let kind = self.tools.get(tool_name).map(|t| t.kind()).unwrap_or(ToolKind::Internal);
                self.history.push(response.choices[0].message.clone());

                let action = AgentDecision::Action {
                    tool: tool_name.to_string(),
                    args: args.to_string(),
                    kind,
                };

                // Check if there's also text content (Thoughts)
                if !content.trim().is_empty() {
                    self.pending_decision = Some(action);
                    return Ok(AgentDecision::Message(content, self.total_usage.clone()));
                }

                return Ok(action);
            }
        }

        // 3. Handle ReAct format (Regex)
        // Improved ReAct parsing (handles multi-line Action Input)
        let action_re = Regex::new(r"(?m)^Action:\s*(.*)")?;
        // Fix: Use non-greedy match and stop at next potential block
        let action_input_re = Regex::new(r"(?ms)^Action Input:\s*(.*?)(?:\nThought:|\nObservation:|\nFinal Answer:|\z)")?;

        let action_match = action_re.captures(&content);
        let action_input_match = action_input_re.captures(&content);

        if action_match.is_some() || action_input_match.is_some() {
            let tool_name = action_match.as_ref().map(|c| c[1].trim().to_string());
            
            let args = action_input_match.as_ref().map(|caps| {
                let mut val = caps[1].trim().to_string();
                if let Some(pos) = val.find("Observation:") {
                    val.truncate(pos);
                }
                val.trim().to_string()
            });

            if let (Some(tool_name), Some(args)) = (tool_name, args) {
            crate::info_log!("Parsed Action (ReAct): {} with args: {}", tool_name, args);
            // Repetition Check
            if let Some((last_tool, last_args)) = &self.last_tool_call {
                if *last_tool == tool_name && *last_args == args {
                    self.repetition_count += 1;
                    if self.repetition_count >= 3 {
                        return Ok(AgentDecision::Error(format!("Detected repeated tool call to '{}' with identical arguments (ReAct). Breaking loop.", tool_name)));
                    }
                } else {
                    self.repetition_count = 0;
                }
            }
            self.last_tool_call = Some((tool_name.clone(), args.clone()));

            // If the content also contains "Final Answer:", prioritize returning the whole thing as a message
            if content.contains("Final Answer:") {
                self.history.push(ChatMessage::assistant(content.clone()));
                return Ok(AgentDecision::Message(content, self.total_usage.clone()));
            }

            let kind = self.tools.get(&tool_name).map(|t| t.kind()).unwrap_or(ToolKind::Internal);
            self.history.push(ChatMessage::assistant(content.clone()));
            
            let action_decision = AgentDecision::Action {
                tool: tool_name,
                args,
                kind,
            };

            // Extract everything before "Action:" as Thought
            if let Some(pos) = content.find("Action:") {
                let thought = content[..pos].trim().to_string();
                if !thought.is_empty() {
                    self.pending_decision = Some(action_decision);
                    return Ok(AgentDecision::Message(thought, self.total_usage.clone()));
                }
            }
            
                return Ok(action_decision);
            } else {
                // Detected partial or malformed action
                let mut error_msg = String::from("Malformed tool call detected.");
                if action_match.is_none() {
                    error_msg.push_str(" Missing 'Action:' tag.");
                } else if action_input_re.captures(&content).is_none() {
                    error_msg.push_str(" Missing 'Action Input:' tag.");
                }
                crate::error_log!("Malformed action detected: {}", error_msg);
                return Ok(AgentDecision::MalformedAction(error_msg));
            }
        }

        // 4. Final Answer or just a message
        self.history.push(ChatMessage::assistant(content.clone()));
        Ok(AgentDecision::Message(content, self.total_usage.clone()))
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

                    if tool == "execute_command" && !auto_approve {
                        let cmd = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                            v.get("command").and_then(|c| c.as_str())
                             .or_else(|| v.get("args").and_then(|c| c.as_str()))
                             .unwrap_or(&args).to_string()
                        } else {
                            args.clone()
                        };
                        let _ = event_tx.send(TuiEvent::SuggestCommand(cmd));

                        if let Some(rx) = &mut approval_rx {
                            // Wait for approval
                            let _ = event_tx.send(TuiEvent::StatusUpdate("Waiting for approval...".to_string()));
                            match rx.recv().await {
                                Some(true) => {
                                    let _ = event_tx.send(TuiEvent::StatusUpdate("Approved.".to_string()));
                                    // Proceed to execution
                                }
                                Some(false) => {
                                    let _ = event_tx.send(TuiEvent::StatusUpdate("Denied.".to_string()));
                                    last_observation = Some("Error: User denied the execution of this command.".to_string());
                                    continue;
                                }
                                None => {
                                    return Ok(("Error: Approval channel closed.".to_string(), self.total_usage.clone()));
                                }
                            }
                        } else {
                            // Legacy behavior: return to caller to handle approval
                            let last_msg = self.history.last().map(|m| m.content.clone()).unwrap_or_default();
                            let mut truncated = last_msg;
                            if let Some(pos) = truncated.find("Observation:") {
                                truncated.truncate(pos);
                            }
                            return Ok((truncated.trim().to_string(), self.total_usage.clone()));
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

                    let observation = match self.tools.get(&tool) {
                        Some(t) => match t.call(&processed_args).await {
                            Ok(output) => output,
                            Err(e) => {
                                let error_msg = format!("Tool Error: {}. Analyze the failure and try a different command or approach if possible.", e);
                                let _ = event_tx.send(TuiEvent::StatusUpdate(format!("❌ Tool '{}' failed", tool)));
                                error_msg
                            },
                        },
                        None => format!("Error: Tool '{}' not found. Check the available tools list.", tool),
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

    /// Generate the system prompt with available tools and ReAct instructions.
    fn generate_system_prompt(&self) -> String {
        use crate::config::prompt::{get_memory_protocol, get_react_protocol};

        let mut tools_desc = String::new();
        for tool in self.tools.values() {
            tools_desc.push_str(&format!("- {}: {}\n  Usage: {}\n", tool.name(), tool.description(), tool.usage()));
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
}
