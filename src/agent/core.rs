//! TESTING BUILD NUMBER CHANGE
use crate::llm::{LlmClient, chat::{ChatMessage, ChatRequest, MessageRole}, TokenUsage};
use crate::agent::tool::Tool;
use serde_json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use regex::Regex;

/// The core Agent that manages the agentic loop.
pub struct Agent {
    pub llm_client: Arc<LlmClient>,
    pub tools: HashMap<String, Box<dyn Tool>>,
    pub max_iterations: usize,
    pub system_prompt_prefix: String,
}

impl Agent {
    /// Create a new Agent with the provided LLM client and tools.
    ///
    /// The tools are registered by their name for easy lookup during the loop.
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
        }
    }


    /// Run the agentic loop for a given user input and conversation history.
    pub async fn run(
        &mut self,
        history: Vec<ChatMessage>,
        event_tx: tokio::sync::mpsc::UnboundedSender<crate::terminal::app::TuiEvent>,
        interrupt_flag: Arc<AtomicBool>,
        auto_approve: bool,
    ) -> Result<(String, TokenUsage), Box<dyn std::error::Error>> {
        let mut history = history;

        // --- Auto-RAG: Proactive Memory Retrieval ---
        if let Some(user_msg) = history.iter().rev().find(|m| m.role == MessageRole::User) {
            if let Some(memory_tool) = self.tools.get("memory") {
                let query = format!("search: {}", user_msg.content);
                if let Ok(memory_context) = memory_tool.call(&query).await {
                    if !memory_context.contains("No relevant memories found") {
                        let context_injection = format!(
                            "\n\n# Relevant Context from Memory\n{}\n",
                            memory_context
                        );
                        
                        if let Some(sys_msg) = history.iter_mut().find(|m| m.role == MessageRole::System) {
                            sys_msg.content.push_str(&context_injection);
                        } else {
                            history.insert(0, ChatMessage::system(format!("{}{}", self.generate_system_prompt(), context_injection)));
                        }
                    }
                }
            }
        }
        
        if history.is_empty() || history[0].role != MessageRole::System {
            history.insert(0, ChatMessage::system(self.generate_system_prompt()));
        }

        // --- Context Pruning ---
        let context_limit = self.llm_client.config().max_context_tokens.min(100000); // Heuristic or from config
        history = self.prune_history(history, context_limit);

        let mut total_usage = TokenUsage::default();

        let action_re = Regex::new(r"Action:\s*(.*)")?;
        let action_input_re = Regex::new(r"Action Input:\s*(.*)")?;

        for _ in 0..self.max_iterations {
            // Check for interruption
            if interrupt_flag.load(Ordering::SeqCst) {
                return Ok(("Interrupted by user.".to_string(), total_usage));
            }

            let _ = event_tx.send(crate::terminal::app::TuiEvent::StatusUpdate("Thinking...".to_string()));
            let request = ChatRequest::new(self.llm_client.model().to_string(), history.clone());
            let response = self.llm_client.chat(&request).await?;
            let content = response.content();

            if let Some(usage) = &response.usage {
                total_usage.prompt_tokens += usage.prompt_tokens;
                total_usage.completion_tokens += usage.completion_tokens;
                total_usage.total_tokens += usage.total_tokens;
            }

            if let Some(tool_calls) = response.choices[0].message.tool_calls.as_ref() {
                let assistant_msg = response.choices[0].message.clone();
                history.push(assistant_msg.clone());

                for tool_call in tool_calls {
                    if interrupt_flag.load(Ordering::SeqCst) {
                        return Ok(("Interrupted by user during tool execution.".to_string(), total_usage));
                    }

                    let tool_name = tool_call.function.name.trim();
                    let args = &tool_call.function.arguments;

                    let _ = event_tx.send(crate::terminal::app::TuiEvent::StatusUpdate(format!("Tool: '{}' (Auto: {})", tool_name, auto_approve)));

                    // Check for auto-approve intercept
                    if tool_name == "execute_command" && !auto_approve {
                        let cmd = if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
                            v.get("command").and_then(|c| c.as_str())
                                 .or_else(|| v.get("args").and_then(|c| c.as_str()))
                                 .unwrap_or(args).to_string()
                        } else {
                            args.to_string()
                        };
                        let _ = event_tx.send(crate::terminal::app::TuiEvent::SuggestCommand(cmd));
                        
                        // Return a cleaned version of the assistant message up to the Action Input
                        let mut truncated = assistant_msg.content.clone();
                        if let Some(pos) = truncated.find("Observation:") {
                            truncated.truncate(pos);
                        }
                        return Ok((truncated.trim().to_string(), total_usage));
                    }

                    let observation_text = match self.tools.get(tool_name) {
                        Some(tool) => match tool.call(args).await {
                            Ok(output) => output,
                            Err(e) => format!("Error: {}", e),
                        },
                        None => format!("Error: Tool '{}' not found.", tool_name),
                    };

                    // Mirror to PTY if NOT a ShellTool (which handles its own mirroring via visible PTY execution)
                    if tool_name != "execute_command" {
                        let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", observation_text.trim());
                        let _ = event_tx.send(crate::terminal::app::TuiEvent::PtyWrite(obs_log.into_bytes()));
                    }

                    history.push(ChatMessage::tool(tool_call.id.clone(), tool_name.to_string(), observation_text));
                }
                continue;
            }

            if content.contains("Final Answer:") {
                // Return full content to preserve thoughts/actions for the UI
                return Ok((content, total_usage));
            }

            let action = action_re.captures(&content).map(|c| c[1].trim().to_string());
            let action_input = action_input_re.captures(&content).map(|c| c[1].trim().to_string());

            if let (Some(tool_name_raw), Some(args)) = (action, action_input) {
                if interrupt_flag.load(Ordering::SeqCst) {
                    return Ok(("Interrupted by user.".to_string(), total_usage));
                }

                let tool_name = tool_name_raw.trim();
                let _ = event_tx.send(crate::terminal::app::TuiEvent::StatusUpdate(format!("Tool: '{}' (Auto: {})", tool_name, auto_approve)));

                // Check for auto-approve intercept
                if tool_name == "execute_command" && !auto_approve {
                    let cmd = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                        v.get("command").and_then(|c| c.as_str())
                         .or_else(|| v.get("args").and_then(|c| c.as_str()))
                         .unwrap_or(&args).to_string()
                    } else {
                        args.clone()
                    };
                    let _ = event_tx.send(crate::terminal::app::TuiEvent::SuggestCommand(cmd));
                    
                    // Truncate LLM response if it hallucinated an observation
                    let mut truncated = content.clone();
                    if let Some(pos) = truncated.find("Observation:") {
                        truncated.truncate(pos);
                    }
                    return Ok((truncated.trim().to_string(), total_usage));
                }

                let observation_text = match self.tools.get(tool_name) {
                    Some(tool) => match tool.call(&args).await {
                        Ok(output) => output,
                        Err(e) => format!("Error: {}", e),
                    },
                    None => format!("Error: Tool '{}' not found.", tool_name),
                };

                // Mirror to PTY if NOT a ShellTool (which handles its own mirroring via visible PTY execution)
                if tool_name != "execute_command" {
                    let obs_log = format!("\x1b[32m[Observation]:\x1b[0m {}\r\n", observation_text.trim());
                    let _ = event_tx.send(crate::terminal::app::TuiEvent::PtyWrite(obs_log.into_bytes()));
                }

                let observation = format!("Observation: {}", observation_text);
                history.push(ChatMessage::assistant(content));
                history.push(ChatMessage::user(observation));
            } else {
                history.push(ChatMessage::assistant(content.clone()));
                return Ok((content, total_usage));
            }
        }

        let _ = event_tx.send(crate::terminal::app::TuiEvent::StatusUpdate("".to_string()));
        Ok(("Error: Maximum iterations reached without a final answer.".to_string(), total_usage))
    }

    /// Prune history to stay within token limits.
    /// Uses char_count / 4 as a simple heuristic for tokens.
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

        // Always keep system message
        let system_msg = history[0].clone();
        let mut pruned = Vec::new();
        pruned.push(system_msg.clone());

        let mut current_tokens = system_msg.content.len() / 4;
        let mut to_keep = Vec::new();

        // Keep as many recent messages as fit
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
        pruned.extend(to_keep);
        pruned
    }

    /// Condense the conversation history by summarizing older messages.
    pub async fn condense_history(&self, history: &[ChatMessage]) -> Result<Vec<ChatMessage>, Box<dyn std::error::Error>> {
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
