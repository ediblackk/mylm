//! Agent Core Implementation
use crate::llm::{LlmClient, chat::{ChatMessage, ChatRequest, MessageRole}, TokenUsage};
use crate::agent::tool::{Tool, ToolKind};
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
    /// Create a new Agent with the provided LLM client and tools.
    pub fn new(client: Arc<LlmClient>, tools: Vec<Box<dyn Tool>>, system_prompt_prefix: String) -> Self {
        Self::new_with_iterations(client, tools, system_prompt_prefix, 10)
    }

    pub fn new_with_iterations(client: Arc<LlmClient>, tools: Vec<Box<dyn Tool>>, system_prompt_prefix: String, max_iterations: usize) -> Self {
        let mut tool_map = HashMap::new();
        for tool in tools {
            tool_map.insert(tool.name().to_string(), tool);
        }

        Self {
            llm_client: client,
            tools: tool_map,
            max_iterations,
            system_prompt_prefix,
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
        let context_limit = self.llm_client.config().max_context_tokens.min(100000);
        self.history = self.prune_history(self.history.clone(), context_limit);

        let request = ChatRequest::new(self.llm_client.model().to_string(), self.history.clone());
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
        let action_re = Regex::new(r"Action:\s*(.*)")?;
        let action_input_re = Regex::new(r"Action Input:\s*(.*)")?;

        let action = action_re.captures(&content).map(|c| c[1].trim().to_string());
        let action_input = action_input_re.captures(&content).map(|c| c[1].trim().to_string());

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
        history: Vec<ChatMessage>,
        event_tx: tokio::sync::mpsc::UnboundedSender<crate::terminal::app::TuiEvent>,
        interrupt_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        auto_approve: bool,
    ) -> Result<(String, TokenUsage), Box<dyn StdError + Send + Sync>> {
        self.reset(history);
        
        let mut last_observation = None;

        loop {
            if interrupt_flag.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(("Interrupted by user.".to_string(), self.total_usage.clone()));
            }

            let _ = event_tx.send(crate::terminal::app::TuiEvent::StatusUpdate("Thinking...".to_string()));
            
            match self.step(last_observation.take()).await? {
                AgentDecision::Message(msg, usage) => {
                    let _ = event_tx.send(crate::terminal::app::TuiEvent::AgentResponse(msg.clone(), usage.clone()));
                    if self.has_pending_decision() {
                        continue;
                    }
                    let _ = event_tx.send(crate::terminal::app::TuiEvent::StatusUpdate("".to_string()));
                    return Ok((msg, usage));
                }
                AgentDecision::Action { tool, args, kind } => {
                    let _ = event_tx.send(crate::terminal::app::TuiEvent::StatusUpdate(format!("Tool: '{}'", tool)));

                    if tool == "execute_command" && !auto_approve {
                        let cmd = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                            v.get("command").and_then(|c| c.as_str())
                             .or_else(|| v.get("args").and_then(|c| c.as_str()))
                             .unwrap_or(&args).to_string()
                        } else {
                            args.clone()
                        };
                        let _ = event_tx.send(crate::terminal::app::TuiEvent::SuggestCommand(cmd));
                        
                        let last_msg = self.history.last().map(|m| m.content.clone()).unwrap_or_default();
                        let mut truncated = last_msg;
                        if let Some(pos) = truncated.find("Observation:") {
                            truncated.truncate(pos);
                        }
                        return Ok((truncated.trim().to_string(), self.total_usage.clone()));
                    }

                    let observation = match self.tools.get(&tool) {
                        Some(t) => match t.call(&args).await {
                            Ok(output) => output,
                            Err(e) => format!("Error: {}", e),
                        },
                        None => format!("Error: Tool '{}' not found.", tool),
                    };

                    if kind == ToolKind::Internal {
                        let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", observation.trim());
                        let _ = event_tx.send(crate::terminal::app::TuiEvent::PtyWrite(obs_log.into_bytes()));
                    }

                    last_observation = Some(observation);
                }
                AgentDecision::Error(e) => {
                    let _ = event_tx.send(crate::terminal::app::TuiEvent::StatusUpdate("".to_string()));
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
            Use the following format:\n\n\
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
            IMPORTANT: After providing an Action and Action Input, you MUST stop generating and wait for the Observation. Do not hallucinate or predict the Observation. You MUST use the tools to interact with the system.\n\n\
            Begin!",
            self.system_prompt_prefix,
            tools_desc,
            self.tools.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    }
}
