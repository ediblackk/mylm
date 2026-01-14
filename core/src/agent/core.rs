//! Agent Core Implementation
use crate::llm::{LlmClient, chat::{ChatMessage, ChatRequest, MessageRole}, TokenUsage};
use crate::agent::tool::{Tool, ToolKind};
use crate::memory::{MemoryCategorizer};
use crate::terminal::app::TuiEvent;
use std::error::Error as StdError;
use serde_json;
use std::sync::Arc;
use std::collections::HashMap;
use regex::Regex;

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
    
    // State maintained between steps
    pub history: Vec<ChatMessage>,
    pub iteration_count: usize,
    pub total_usage: TokenUsage,
    pub pending_decision: Option<AgentDecision>,
    
    // Safety tracking
    last_tool_call: Option<(String, String)>,
    repetition_count: usize,
}

impl Agent {
    pub fn new_with_iterations(
        client: Arc<LlmClient>,
        tools: Vec<Box<dyn Tool>>,
        system_prompt_prefix: String,
        max_iterations: usize,
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

        // Ensure system prompt is present
        if self.history.is_empty() || self.history[0].role != MessageRole::System {
            self.history.insert(0, ChatMessage::system(self.generate_system_prompt()));
        }
    }

    /// Perform a single step in the agentic loop.
    pub async fn step(&mut self, observation: Option<String>) -> Result<AgentDecision, Box<dyn StdError + Send + Sync>> {
        // 1. Hard Iteration Limit Check
        if self.iteration_count >= self.max_iterations {
            return Ok(AgentDecision::Error(format!("Maximum iteration limit ({}) reached. Task aborted to prevent infinite loop.", self.max_iterations)));
        }

        // 2. Return pending decision if we have one (usually an Action queued after a Thought)
        if let Some(decision) = self.pending_decision.take() {
            return Ok(decision);
        }

        if let Some(obs) = observation {
            // If the last message was an assistant message with tool calls, we should use tool role
            // Otherwise use user role for "Observation: ..."
            let is_tool_call_response = self.history.last().map(|m| m.tool_calls.is_some()).unwrap_or(false);
            
            if is_tool_call_response {
                self.history.push(ChatMessage::user(format!("Observation: {}", obs)));
            } else {
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

        if let Some(usage) = &response.usage {
            self.total_usage.prompt_tokens += usage.prompt_tokens;
            self.total_usage.completion_tokens += usage.completion_tokens;
            self.total_usage.total_tokens += usage.total_tokens;
        }

        self.iteration_count += 1;

        // --- Process Decision ---

        // 1. Handle Tool Calls (Modern API)
        if let Some(tool_calls) = response.choices[0].message.tool_calls.as_ref() {
            if let Some(tool_call) = tool_calls.first() {
                let tool_name = tool_call.function.name.trim();
                let args = &tool_call.function.arguments;
                
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

        // 2. Handle ReAct format (Regex)
        // Improved ReAct parsing (handles multi-line Action Input)
        let action_re = Regex::new(r"(?m)^Action:\s*(.*)")?;
        let action_input_re = Regex::new(r"(?ms)^Action Input:\s*(.*)$")?;

        let action = action_re.captures(&content).map(|c| c[1].trim().to_string());
        
        // Action Input might be multi-line or followed by other tags, we need to be careful
        // Often it's at the end of the message or followed by "Observation:"
        let action_input = if let Some(caps) = action_input_re.captures(&content) {
            let mut val = caps[1].trim().to_string();
            if let Some(pos) = val.find("Observation:") {
                val.truncate(pos);
            }
            Some(val.trim().to_string())
        } else {
            None
        };

        if let (Some(tool_name), Some(args)) = (action, action_input) {
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
        }

        // 3. Final Answer or just a message
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
                    let _ = event_tx.send(TuiEvent::AgentResponse(msg.clone(), usage.clone()));
                    
                    // If we have a pending decision (like an Action queued after a Thought),
                    // continue the loop immediately to execute it.
                    if self.has_pending_decision() {
                        continue;
                    }

                    // For "Autonomous" mode, we look for "Final Answer:" to know when to stop.
                    // If it's just a message without Final Answer, and we're in the middle of a task,
                    // we might want to continue. But for now, we follow the ReAct pattern.
                    if msg.contains("Final Answer:") {
                        let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                        return Ok((msg, usage));
                    }
                    
                    // If we get a message without "Final Answer:" and we're in an autonomous loop,
                    // we should nudge the agent to continue if it hasn't reached a conclusion.
                    // However, if it looks like a question to the user, we should stop.
                    if msg.trim().ends_with('?') || msg.contains("Please") || msg.contains("Would you") {
                        let _ = event_tx.send(TuiEvent::StatusUpdate("".to_string()));
                        return Ok((msg, usage));
                    }

                    // Nudge to continue
                    last_observation = Some("Please continue your task or provide a Final Answer if you are done.".to_string());
                    continue;
                }
                AgentDecision::Action { tool, args, kind } => {
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
                            Err(e) => format!("Error: {}. Analyze the failure and try a different command or approach if possible.", e),
                        },
                        None => format!("Error: Tool '{}' not found. Check the available tools list.", tool),
                    };

                    if kind == ToolKind::Internal {
                        let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", observation.trim());
                        let _ = event_tx.send(TuiEvent::InternalObservation(obs_log.into_bytes()));
                    }

                    last_observation = Some(observation);
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
        let mut tools_desc = String::new();
        for tool in self.tools.values() {
            tools_desc.push_str(&format!("- {}: {}\n  Usage: {}\n", tool.name(), tool.description(), tool.usage()));
        }

        format!(
            "{}\n\n\
            # Operational Protocol (ReAct)\n\
            You have access to the following tools:\n\n\
            {}\n\
            CRITICAL: You MUST use the following format for every step. Do not skip tags. Do not output free-form text outside these tags.\n\n\
            Question: the input question you must answer\n\
            Thought: you should always think about what to do\n\
            Action: the action to take, should be one of [{}]\n\
            Action Input: the input to the action\n\
            Observation: the result of the action (STOP after providing Action Input and wait for this)\n\
            ... (this Thought/Action/Action Input/Observation can repeat N times)\n\
            Thought: I now know the final answer\n\
            Final Answer: the final answer to the original input question\n\n\
            ## Example\n\
            Question: list files in current directory\n\
            Thought: I need to use the ls command to list files.\n\
            Action: execute_command\n\
            Action Input: ls -la\n\
            Observation: total 0\n\
            Thought: The directory is empty.\n\
            Final Answer: The directory is empty.\n\n\
            IMPORTANT: \n\
            1. You MUST use the tools to interact with the system.\n\
            2. After providing an Action and Action Input, you MUST stop generating and wait for the Observation.\n\
            3. Do not hallucinate or predict the Observation.\n\
            4. ALWAYS prefix your thoughts with 'Thought:'.\n\
            5. If you are stuck or need clarification, use 'Final Answer:' to ask the user.\n\n\
            Begin!",
            self.system_prompt_prefix,
            tools_desc,
            self.tools.keys().cloned().collect::<Vec<_>>().join(", ")
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
