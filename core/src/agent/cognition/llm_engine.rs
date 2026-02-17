//! LLM-Based Cognitive Engine
//!
//! Real cognitive engine that uses LLM to make decisions.
//! Parses tool calls from LLM responses using Short-Key JSON format.

use crate::agent::cognition::{
    engine::CognitiveEngine,
    state::AgentState,
    input::InputEvent,
    decision::{Transition, AgentDecision, ToolCall, LLMRequest, AgentExitReason, ApprovalRequest},
    error::CognitiveError,
};

/// Short-Key Action representation (Simplified JSON Protocol)
/// 
/// Fields:
/// - `t`: Thought/reasoning
/// - `a`: Action/tool name to execute (optional)
/// - `i`: Input arguments for the action (optional JSON)
/// - `f`: Final answer/message to user (optional)
/// - `c`: Confirm flag - when true, wait for user approval
/// - `r`: Remember - save content to long-term memory (optional)
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
struct ShortKeyAction {
    #[serde(rename = "t", default)]
    thought: String,
    #[serde(rename = "a")]
    action: Option<String>,
    #[serde(rename = "i")]
    input: Option<serde_json::Value>,
    #[serde(rename = "f")]
    final_answer: Option<String>,
    #[serde(rename = "c", default)]
    confirm: bool,
    #[serde(rename = "r")]
    remember: Option<String>,
}

/// Tool description for dynamic prompt generation
#[derive(Debug, Clone)]
pub struct ToolDescription {
    pub name: String,
    pub description: String,
    pub usage: String,
}

/// LLM-based cognitive engine
/// 
/// This engine is PURE - it doesn't make actual LLM calls.
/// Instead, it emits AgentDecision::RequestLLM with the prompt,
/// and the Session/runtime layer fulfills it.
pub struct LLMBasedEngine {
    system_prompt: String,
    #[allow(dead_code)]
    max_tool_failures: usize,
    /// Dynamic tool descriptions for prompt generation
    tool_descriptions: Vec<ToolDescription>,
}

impl LLMBasedEngine {
    pub fn new() -> Self {
        Self {
            system_prompt: build_system_prompt(),
            max_tool_failures: 2,
            tool_descriptions: Vec::new(),
        }
    }
    
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }
    
    /// Set dynamic tool descriptions for prompt generation
    /// 
    /// This allows the engine to generate system prompts with
    /// the actual available tools rather than hardcoded lists.
    pub fn with_tool_descriptions(mut self, descriptions: Vec<ToolDescription>) -> Self {
        self.tool_descriptions = descriptions;
        self
    }
    
    /// Convert tool descriptions to ToolDef format for Context
    fn build_tool_defs(&self) -> Vec<crate::agent::types::intents::ToolDef> {
        self.tool_descriptions.iter().map(|desc| {
            crate::agent::types::intents::ToolDef {
                name: desc.name.clone(),
                description: desc.description.clone(),
                parameters: serde_json::json!({}), // Simplified - tools parse their own args
            }
        }).collect()
    }
    
    /// Parse LLM response to extract decision using Short-Key JSON Protocol
    fn parse_response(&self, _state: &AgentState, response: &str) -> Result<AgentDecision, String> {
        let trimmed = response.trim();
        
        // Try Short-Key JSON format first
        if let Some(action) = parse_short_key_action(trimmed) {
            // Check for remember field first (pure - just return the decision)
            if let Some(remember_content) = action.remember {
                return Ok(AgentDecision::Remember { content: remember_content });
            }
            
            // If has final answer, emit response
            if let Some(final_answer) = action.final_answer {
                return Ok(AgentDecision::EmitResponse(final_answer));
            }
            
            // If has action, create tool call
            if let Some(tool_name) = action.action {
                let args = action.input.unwrap_or(serde_json::Value::Null);
                return Ok(AgentDecision::CallTool(ToolCall {
                    name: tool_name,
                    arguments: args,
                    working_dir: None,
                    timeout_secs: None,
                }));
            }
            
            // Thought only - emit as response
            if !action.thought.is_empty() {
                return Ok(AgentDecision::EmitResponse(action.thought));
            }
        }
        
        // Fallback: Parse Kimi XML tool call format
        // <tool_call><function=name><parameter=key>value</parameter></function></tool_call>
        if let Some(tool_call) = parse_kimi_xml_tool_call(trimmed) {
            return Ok(AgentDecision::CallTool(tool_call));
        }
        
        // Fallback: Check for response to user (XML format legacy)
        if let Some(response_text) = parse_user_response(trimmed) {
            return Ok(AgentDecision::EmitResponse(response_text));
        }
        
        // Default: emit the response as-is
        Ok(AgentDecision::EmitResponse(trimmed.to_string()))
    }
}

impl Default for LLMBasedEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl From<crate::agent::runtime::tools::ToolDescription> for ToolDescription {
    fn from(desc: crate::agent::runtime::tools::ToolDescription) -> Self {
        Self {
            name: desc.name.to_string(),
            description: desc.description.to_string(),
            usage: desc.usage.to_string(),
        }
    }
}

impl CognitiveEngine for LLMBasedEngine {
    fn step(
        &mut self,
        state: &AgentState,
        input: Option<InputEvent>,
    ) -> Result<Transition, CognitiveError> {
        // Check limits
        if state.at_limit() {
            return Ok(Transition::exit(
                state.clone(),
                AgentExitReason::StepLimit
            ));
        }
        
        if state.too_many_rejections() {
            return Ok(Transition::exit(
                state.clone(),
                AgentExitReason::Error("Too many tool rejections".to_string())
            ));
        }
        
        // Handle input
        if let Some(InputEvent::RuntimeError { .. }) = input {
            crate::info_log!("[LLM_ENGINE] Received RuntimeError input");
        }

        match input {
            // User message - request LLM to decide (works for any step count)
            Some(InputEvent::UserMessage(msg)) => {
                // CRITICAL: Add user message to history FIRST
                let state_with_message = state.clone()
                    .with_message(crate::agent::cognition::history::Message::user(&msg));
                
                // Include the actual user message in the scratchpad
                let scratchpad = format!("User: {}\n\nWhat should I do?", msg);
                
                // Convert state history to context history format
                let history: Vec<crate::agent::types::intents::Message> = state_with_message.history.iter().map(|m| {
                    let role = match m.role {
                        crate::agent::cognition::history::MessageRole::User => 
                            crate::agent::types::intents::Role::User,
                        crate::agent::cognition::history::MessageRole::Assistant => 
                            crate::agent::types::intents::Role::Assistant,
                        crate::agent::cognition::history::MessageRole::System => 
                            crate::agent::types::intents::Role::System,
                        crate::agent::cognition::history::MessageRole::Tool => 
                            crate::agent::types::intents::Role::Tool,
                    };
                    crate::agent::types::intents::Message {
                        role,
                        content: m.content.clone(),
                    }
                }).collect();
                
                let context = crate::agent::types::intents::Context::new(scratchpad)
                    .with_system(self.system_prompt.clone())
                    .with_history(history)
                    .with_tools(self.build_tool_defs());
                
                let decision = AgentDecision::RequestLLM(LLMRequest {
                    context,
                    max_tokens: None,
                    temperature: None,
                    model: None,
                    response_format: None,
                    stream: false,
                });
                
                let next_state = state_with_message.increment_step();
                Ok(Transition::new(next_state, decision))
            }
            
            // LLM response - parse and act
            Some(InputEvent::LLMResponse(llm_resp)) => {
                // CRITICAL: Add assistant response to history
                let state_with_response = state.clone()
                    .with_message(crate::agent::cognition::history::Message::assistant(&llm_resp.content));
                
                match self.parse_response(state, &llm_resp.content) {
                    Ok(decision) => {
                        // Check if tool needs approval
                        let (final_decision, pending_tool) = if let AgentDecision::CallTool(ref call) = decision {
                            let args_str = call.arguments.to_string();
                            if self.requires_approval(&call.name, &args_str) {
                                // Store the tool call as pending and request approval
                                (AgentDecision::RequestApproval(ApprovalRequest {
                                    tool: call.name.clone(),
                                    args: args_str.clone(),
                                    reason: format!("Tool '{}' requires approval", call.name),
                                }), Some(call.clone()))
                            } else {
                                (decision, None)
                            }
                        } else {
                            (decision, None)
                        };
                        
                        let next_state = state_with_response
                            .increment_step()
                            .with_pending_tool(pending_tool);
                        Ok(Transition::new(next_state, final_decision))
                    }
                    Err(e) => {
                        // Parsing failed - emit error response
                        Ok(Transition::new(
                            state_with_response.increment_step(),
                            AgentDecision::EmitResponse(format!("Error: {}", e))
                        ))
                    }
                }
            }
            
            // Tool result - request LLM to interpret
            Some(InputEvent::ToolResult { tool, result }) => {
                let (status, output) = match result {
                    crate::agent::types::events::ToolResult::Success { output, .. } => {
                        ("succeeded", output.clone())
                    }
                    crate::agent::types::events::ToolResult::Error { message, .. } => {
                        ("failed", message.clone())
                    }
                    crate::agent::types::events::ToolResult::Cancelled => {
                        ("cancelled", "Cancelled".to_string())
                    }
                };
                
                // CRITICAL: Add tool result to history
                let tool_content = format!("Tool '{}' {}: {}", tool, status, output);
                let state_with_tool = state.clone()
                    .with_message(crate::agent::cognition::history::Message::tool(&tool_content));
                
                // Convert history for context
                let history: Vec<crate::agent::types::intents::Message> = state_with_tool.history.iter().map(|m| {
                    let role = match m.role {
                        crate::agent::cognition::history::MessageRole::User => 
                            crate::agent::types::intents::Role::User,
                        crate::agent::cognition::history::MessageRole::Assistant => 
                            crate::agent::types::intents::Role::Assistant,
                        crate::agent::cognition::history::MessageRole::System => 
                            crate::agent::types::intents::Role::System,
                        crate::agent::cognition::history::MessageRole::Tool => 
                            crate::agent::types::intents::Role::Tool,
                    };
                    crate::agent::types::intents::Message {
                        role,
                        content: m.content.clone(),
                    }
                }).collect();
                
                let scratchpad = format!(
                    "Tool '{}' {} with output: {}\n\nWhat should I do next?",
                    tool, status, output
                );
                
                let context = crate::agent::types::intents::Context::new(scratchpad)
                    .with_system(self.system_prompt.clone())
                    .with_history(history)
                    .with_tools(self.build_tool_defs());
                
                let decision = AgentDecision::RequestLLM(LLMRequest {
                    context,
                    max_tokens: None,
                    temperature: None,
                    model: None,
                    response_format: None,
                    stream: false,
                });
                
                let next_state = state_with_tool.increment_step();
                Ok(Transition::new(next_state, decision))
            }
            
            // Approval result - continue or abort
            Some(InputEvent::ApprovalResult(approval)) => {
                match approval {
                    crate::agent::cognition::input::ApprovalOutcome::Granted => {
                        // Execute the pending tool if we have one
                        if let Some(ref tool_call) = state.pending_tool {
                            crate::info_log!("[LLM_ENGINE] Approval granted, executing pending tool: {}", tool_call.name);
                            let next_state = state.clone()
                                .increment_step()
                                .with_pending_tool(None); // Clear pending tool
                            Ok(Transition::new(next_state, AgentDecision::CallTool(tool_call.clone())))
                        } else {
                            // No pending tool, just continue
                            crate::warn_log!("[LLM_ENGINE] Approval granted but no pending tool found");
                            let next_state = state.clone().increment_step();
                            Ok(Transition::new(next_state, AgentDecision::None))
                        }
                    }
                    crate::agent::cognition::input::ApprovalOutcome::Denied { .. } => {
                        let next_state = state.clone().increment_rejection();
                        let scratchpad = "Tool execution was denied by user. What should I do instead?".to_string();
                        let context = crate::agent::types::intents::Context::new(scratchpad)
                            .with_system(self.system_prompt.clone())
                            .with_tools(self.build_tool_defs());
                        let decision = AgentDecision::RequestLLM(LLMRequest {
                            context,
                            max_tokens: None,
                            temperature: None,
                            model: None,
                            response_format: None,
                            stream: false,
                        });
                        Ok(Transition::new(next_state, decision))
                    }
                }
            }
            
            // Worker result
            Some(InputEvent::WorkerResult(id, result)) => {
                let output = match result {
                    Ok(output) => format!("Worker {} completed: {}", id.0, output),
                    Err(e) => format!("Worker {} failed: {}", id.0, e.message),
                };
                
                let scratchpad = format!("{}\n\nWhat should I do next?", output);
                let context = crate::agent::types::intents::Context::new(scratchpad)
                    .with_system(self.system_prompt.clone())
                    .with_tools(self.build_tool_defs());
                let decision = AgentDecision::RequestLLM(LLMRequest {
                    context,
                    max_tokens: None,
                    temperature: None,
                    model: None,
                    response_format: None,
                    stream: false,
                });
                
                let next_state = state.clone().increment_step();
                Ok(Transition::new(next_state, decision))
            }
            
            // Tick - no action needed (used for session heartbeat)
            Some(InputEvent::Tick) => {
                Ok(Transition::new(state.clone(), AgentDecision::None))
            }
            
            // Shutdown - exit
            Some(InputEvent::Shutdown) => {
                Ok(Transition::exit(state.clone(), AgentExitReason::UserRequest))
            }

            // Runtime error - exit with error instead of retrying infinitely
            Some(InputEvent::RuntimeError { error, .. }) => {
                crate::info_log!("[LLM_ENGINE] RuntimeError received: {}. Exiting.", error);
                Ok(Transition::exit(
                    state.clone(),
                    AgentExitReason::Error(format!("Runtime error: {}", error))
                ))
            }
            
            // Default - no action (was causing infinite loop by requesting LLM)
            _ => {
                Ok(Transition::new(state.clone(), AgentDecision::None))
            }
        }
    }
    
    fn build_prompt(&self, _state: &AgentState) -> String {
        // Note: This method is required by the trait but not used in the current
        // architecture. The actual prompt building happens in build_messages_from_context()
        // which includes tools dynamically.
        self.system_prompt.clone()
    }
    
    fn requires_approval(&self, tool: &str, args: &str) -> bool {
        // Safety policy
        let dangerous_tools = ["shell", "write_file", "rm", "sudo"];
        let dangerous_patterns = ["rm -rf", "sudo", "curl | sh", "wget | sh"];
        
        if dangerous_tools.contains(&tool) {
            return true;
        }
        
        let command = format!("{} {}", tool, args);
        dangerous_patterns.iter().any(|p| command.contains(p))
    }
}

// ===== Helper Functions =====

fn build_system_prompt() -> String {
    // Get current date/time in a human-readable format
    let now = chrono::Local::now();
    let date_time_str = now.format("%A, %B %d, %Y at %I:%M:%S %p %Z").to_string();
    
    format!(r#"You are an AI assistant that helps users by using tools and reasoning step by step.

Current Date and Time: {date_time}

Response Format (Short-Key JSON - ALWAYS use this format):

1. For tool calls:
   {{"t": "your reasoning", "a": "tool_name", "i": {{"arg": "value"}}}}

2. For final answers to user:
   {{"t": "your reasoning", "f": "your response to user"}}

3. To remember something (can add to any response):
   {{"t": "Learning user preference", "r": "User prefers dark mode", "f": "I'll use dark mode for you"}}

Field meanings:
- "t": Your internal thought/reasoning (required)
- "a": Action/tool name to execute (for tool calls)
- "i": Input arguments as JSON object (for tool calls)
- "f": Final answer message to user (for responses)
- "r": Remember - save content to long-term memory (optional, works with any response type)

Rules:
- ALWAYS respond with valid JSON
- Use "f" to respond to the user
- Use "a" + "i" when calling tools
- Use "r" anytime you learn something worth remembering (user preferences, facts, context)
- Do not use both "a" and "f" in same response
- Keep thoughts concise but clear

Examples:
{{"t": "Need to check directory contents", "a": "shell", "i": {{"command": "ls -la"}}}}
{{"t": "Found the files", "f": "Here are the files in your directory..."}}
{{"t": "User likes Python", "r": "User prefers Python over other languages", "f": "I'll use Python for this task"}}"#, date_time = date_time_str)
}

// ===== Response Parsers =====

fn parse_user_response(response: &str) -> Option<String> {
    // Check for <response>...</response>
    let re = regex::Regex::new(r"<response>(.*?)</response>").ok()?;
    
    if let Some(caps) = re.captures(response) {
        return Some(caps.get(1)?.as_str().trim().to_string());
    }
    
    // If no XML tags and looks like a response, return as-is
    if !response.contains('<') {
        return Some(response.to_string());
    }
    
    None
}

/// Parse Kimi XML tool call format
/// 
/// Format: <tool_call><function=name><parameter=key>value</parameter></function></tool_call>
/// Or: <tool_call>\n<function=name>\n<parameter=key>\nvalue\n</parameter>\n</function>\n</tool_call>
fn parse_kimi_xml_tool_call(response: &str) -> Option<ToolCall> {
    // Check if it contains tool_call tag
    if !response.contains("<tool_call>") {
        return None;
    }
    
    // Extract function name: <function=name> or <function name="...">
    let func_re = regex::Regex::new(r"<function=([^>]+)>").ok()?;
    let tool_name = func_re.captures(response)?
        .get(1)?
        .as_str()
        .trim()
        .to_string();
    
    // Extract parameters - handle multiple <parameter=name>value</parameter>
    let param_re = regex::Regex::new(r"<parameter=([^>]+)>\s*(.*?)\s*</parameter>").ok()?;
    let mut args = serde_json::Map::new();
    
    for caps in param_re.captures_iter(response) {
        if let (Some(key), Some(value)) = (caps.get(1), caps.get(2)) {
            args.insert(
                key.as_str().trim().to_string(),
                serde_json::Value::String(value.as_str().trim().to_string())
            );
        }
    }
    
    if tool_name.is_empty() {
        return None;
    }
    
    Some(ToolCall {
        name: tool_name,
        arguments: serde_json::Value::Object(args),
        working_dir: None,
        timeout_secs: None,
    })
}

/// Response parser for different LLM output formats
pub struct ResponseParser;

impl ResponseParser {
    /// Parse tool calls from various LLM formats
    pub fn parse_tool_calls(content: &str) -> Vec<ToolCall> {
        let mut calls = Vec::new();
        
        // XML format: <tool>name</tool><args>args</args>
        if let Ok(re) = regex::Regex::new(r"<tool>(.*?)</tool>\s*<args>(.*?)</args>") {
            for caps in re.captures_iter(content) {
                if let (Some(tool), Some(args)) = (caps.get(1), caps.get(2)) {
                    calls.push(ToolCall {
                        name: tool.as_str().trim().to_string(),
                        arguments: serde_json::json!(args.as_str().trim().to_string()),
                        working_dir: None,
                        timeout_secs: None,
                    });
                }
            }
        }
        
        // JSON format: {"tool": "name", "args": "args"}
        if calls.is_empty() && content.trim().starts_with('{') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
                if let (Some(tool), Some(args)) = (
                    json.get("tool").and_then(|v| v.as_str()),
                    json.get("args").and_then(|v| v.as_str())
                ) {
                    calls.push(ToolCall {
                        name: tool.to_string(),
                        arguments: serde_json::json!(args.to_string()),
                        working_dir: None,
                        timeout_secs: None,
                    });
                }
            }
        }
        
        calls
    }
}

/// Parse Short-Key JSON action from LLM response
/// 
/// Handles:
/// - Fenced JSON blocks (```json ... ```)
/// - Inline JSON objects
/// - Normalized JSON with unescaped newlines
fn parse_short_key_action(content: &str) -> Option<ShortKeyAction> {
    let trimmed = content.trim();
    
    // Try parsing directly first
    if let Ok(action) = serde_json::from_str::<ShortKeyAction>(trimmed) {
        return Some(action);
    }
    
    // Extract from fenced code blocks
    let fence_re = regex::Regex::new(r"```(?:json)?\s*\n(.*?)\n```").ok()?;
    for caps in fence_re.captures_iter(content) {
        if let Some(block) = caps.get(1) {
            if let Ok(action) = serde_json::from_str::<ShortKeyAction>(block.as_str().trim()) {
                return Some(action);
            }
        }
    }
    
    // Extract balanced JSON objects
    let candidates = extract_json_objects(content);
    for c in candidates {
        if let Ok(action) = serde_json::from_str::<ShortKeyAction>(&c) {
            return Some(action);
        }
    }
    
    None
}

/// Extract top-level JSON objects from text by brace balancing
fn extract_json_objects(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_string = false;
    let mut escape = false;
    let mut depth: i32 = 0;
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
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
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
