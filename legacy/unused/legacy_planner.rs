//! Legacy Single-Step Planner (DEPRECATED)
//!
//! This is the old single-step cognitive engine.
//! 
//! DEPRECATED: Use `Planner` from `crate::agent::cognition::planner` instead.
//! This module is kept for reference and will be removed in a future version.
//!
//! Real cognitive engine that uses LLM to make decisions.
//! Parses LLM responses using the parser module (types::parser).

use crate::agent::cognition::{
    engine::CognitiveEngine,
    state::AgentState,
    input::InputEvent,
    decision::{Transition, AgentDecision, LLMRequest, AgentExitReason, ApprovalRequest},
    error::CognitiveError,
    prompts::system::build_system_prompt,
};
use crate::agent::types::parser::{ShortKeyParser, ParsedResponse};

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
    /// Parser for LLM responses
    parser: ShortKeyParser,
}

impl LLMBasedEngine {
    pub fn new() -> Self {
        Self {
            system_prompt: build_system_prompt(),
            max_tool_failures: 2,
            tool_descriptions: Vec::new(),
            parser: ShortKeyParser::new(),
        }
    }
    
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }
    
    /// Set dynamic tool descriptions for prompt generation
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
                parameters: serde_json::json!({}),
                usage: Some(desc.usage.clone()),
            }
        }).collect()
    }
    
    /// Parse LLM response to extract decision
    fn parse_response(&self, _state: &AgentState, response: &str) -> Result<AgentDecision, String> {
        match self.parser.parse_to_response(response) {
            Ok(ParsedResponse::ToolCalls(calls)) => {
                if let Some(call) = calls.into_iter().next() {
                    Ok(AgentDecision::CallTool(call))
                } else {
                    Ok(AgentDecision::EmitResponse("No tool calls found".to_string()))
                }
            }
            Ok(ParsedResponse::FinalAnswer(answer)) => {
                Ok(AgentDecision::EmitResponse(answer))
            }
            Ok(ParsedResponse::Remember { content, .. }) => {
                Ok(AgentDecision::Remember { content })
            }
            Ok(ParsedResponse::RememberAndCall { content: _, tool }) => {
                // For now, prioritize the tool call - memory happens in background
                Ok(AgentDecision::CallTool(tool))
            }
            Ok(ParsedResponse::ConfirmRequest { tool, .. }) => {
                Ok(AgentDecision::RequestApproval(ApprovalRequest {
                    tool: tool.name,
                    args: tool.arguments.to_string(),
                    reason: "Tool requires confirmation".to_string(),
                }))
            }
            Ok(ParsedResponse::Malformed { error, .. }) => {
                Err(format!("Parse error: {}", error))
            }
            Err(_e) => {
                // Fallback: emit the raw response
                Ok(AgentDecision::EmitResponse(response.trim().to_string()))
            }
        }
    }
}

impl Default for LLMBasedEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl From<crate::agent::tools::ToolDescription> for ToolDescription {
    fn from(desc: crate::agent::tools::ToolDescription) -> Self {
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

        match input {
            // User message - request LLM to decide
            Some(InputEvent::UserMessage(msg)) => {
                let state_with_message = state.clone()
                    .with_message(crate::agent::cognition::history::Message::user(&msg));
                
                let scratchpad = format!("User: {}\n\nWhat should I do?", msg);
                
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
                    retry_attempt: 0,
                    extra_system_messages: Vec::new(),
                });
                
                let next_state = state_with_message.increment_step();
                Ok(Transition::new(next_state, decision))
            }
            
            // LLM response - parse and act
            Some(InputEvent::LLMResponse(llm_resp)) => {
                let state_with_response = state.clone()
                    .with_message(crate::agent::cognition::history::Message::assistant(&llm_resp.content));
                
                match self.parse_response(state, &llm_resp.content) {
                    Ok(decision) => {
                        let (final_decision, pending_tool) = if let AgentDecision::CallTool(ref call) = decision {
                            let args_str = call.arguments.to_string();
                            if self.requires_approval(&call.name, &args_str) {
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
                
                let tool_content = format!("Tool '{}' {}: {}", tool, status, output);
                let state_with_tool = state.clone()
                    .with_message(crate::agent::cognition::history::Message::tool(&tool_content));
                
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
                    retry_attempt: 0,
                    extra_system_messages: Vec::new(),
                });
                
                let next_state = state_with_tool.increment_step();
                Ok(Transition::new(next_state, decision))
            }
            
            // Approval result
            Some(InputEvent::ApprovalResult(approval)) => {
                match approval {
                    crate::agent::cognition::input::ApprovalOutcome::Granted => {
                        if let Some(ref tool_call) = state.pending_tool {
                            crate::info_log!("[LLM_ENGINE] Approval granted, executing pending tool: {}", tool_call.name);
                            let next_state = state.clone()
                                .increment_step()
                                .with_pending_tool(None);
                            Ok(Transition::new(next_state, AgentDecision::CallTool(tool_call.clone())))
                        } else {
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
                            retry_attempt: 0,
                            extra_system_messages: Vec::new(),
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
                    retry_attempt: 0,
                    extra_system_messages: Vec::new(),
                });
                
                let next_state = state.clone().increment_step();
                Ok(Transition::new(next_state, decision))
            }
            
            // Tick - no action needed
            Some(InputEvent::Tick) => {
                Ok(Transition::new(state.clone(), AgentDecision::None))
            }
            
            // Shutdown - exit
            Some(InputEvent::Shutdown) => {
                Ok(Transition::exit(state.clone(), AgentExitReason::UserRequest))
            }

            // Runtime error - exit
            Some(InputEvent::RuntimeError { error, .. }) => {
                crate::info_log!("[LLM_ENGINE] RuntimeError received: {}. Exiting.", error);
                Ok(Transition::exit(
                    state.clone(),
                    AgentExitReason::Error(format!("Runtime error: {}", error))
                ))
            }
            
            // Default - no action
            _ => {
                Ok(Transition::new(state.clone(), AgentDecision::None))
            }
        }
    }
    
    fn build_prompt(&self, _state: &AgentState) -> String {
        self.system_prompt.clone()
    }
    
    fn requires_approval(&self, tool: &str, args: &str) -> bool {
        let dangerous_tools = ["shell", "write_file", "rm", "sudo"];
        let dangerous_patterns = ["rm -rf", "sudo", "curl | sh", "wget | sh"];
        
        if dangerous_tools.contains(&tool) {
            return true;
        }
        
        let command = format!("{} {}", tool, args);
        dangerous_patterns.iter().any(|p| command.contains(p))
    }
}


