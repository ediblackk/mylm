//! Planner - Decision-making component for the agent
//!
//! Takes state + events, produces an intent graph of what to do next.
//! Supports parallel tool calls from a single LLM response.
//!
//! This planner is PURE - it doesn't make actual LLM calls.
//! Instead, it emits Intent::RequestLLM with the prompt,
//! and the Session/runtime layer fulfills it.

use crate::agent::cognition::kernel::{GraphEngine, AgentState, KernelError, PendingApproval};
use crate::agent::types::{
    graph::IntentGraph,
    intents::IntentNode,
    ids::IntentId,
    intents::{Intent, ExitReason, Context, LLMRequest, Message, Role},
    events::KernelEvent,
    config::KernelConfig,
    parser::{ShortKeyParser, ParsedResponse},
};

use super::prompts::system::{ToolDescription, build_tool_defs, build_system_prompt};
use super::policy::approval::ApprovalPolicy;

/// Planner implementation
///
/// Processes events and produces intent graphs.
/// Maintains state and uses an LLM (via emitted intents) to make decisions.
/// Supports parallel tool execution from single LLM response.
pub struct Planner {
    /// Internal state
    state: AgentState,
    /// System prompt
    system_prompt: String,
    /// Tool descriptions for prompt generation
    tool_descriptions: Vec<ToolDescription>,
    /// Response parser
    parser: ShortKeyParser,
    /// Approval policy
    approval_policy: ApprovalPolicy,
}

impl Planner {
    /// Create a new kernel
    pub fn new() -> Self {
        Self {
            state: AgentState::default(),
            system_prompt: build_system_prompt(),
            tool_descriptions: Vec::new(),
            parser: ShortKeyParser::new(),
            approval_policy: ApprovalPolicy::default(),
        }
    }
    
    /// Set custom system prompt
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }
    
    /// Set tool descriptions for prompt generation
    pub fn with_tool_descriptions(mut self, descriptions: Vec<ToolDescription>) -> Self {
        self.tool_descriptions = descriptions;
        self
    }
    
    /// Set approval policy
    pub fn with_approval_policy(mut self, policy: ApprovalPolicy) -> Self {
        self.approval_policy = policy;
        self
    }
}

impl Default for Planner {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphEngine for Planner {
    fn init(&mut self, config: KernelConfig) -> Result<(), KernelError> {
        self.state.max_steps = config.max_steps;
        Ok(())
    }
    
    fn process(&mut self, events: &[KernelEvent]) -> Result<IntentGraph, KernelError> {
        let mut graph = IntentGraph::new();
        crate::info_log!("[PLANNER] process called with {} events", events.len());
        
        for event in events {
            crate::debug_log!("[PLANNER] Handling event: {:?}", std::mem::discriminant(event));
            self.handle_event(event, &mut graph)?;
        }
        
        crate::info_log!("[PLANNER] process returning graph with {} nodes", graph.len());
        Ok(graph)
    }
    
    fn state(&self) -> &AgentState {
        &self.state
    }
}

impl Planner {
    /// Handle a single event and add intents to graph
    fn handle_event(&mut self, event: &KernelEvent, graph: &mut IntentGraph) -> Result<(), KernelError> {
        crate::info_log!("[LLM_KERNEL] handle_event: {:?}", std::mem::discriminant(event));
        match event {
            KernelEvent::UserMessage { content } => {
                self.handle_user_message(content, graph)
            }
            KernelEvent::LLMCompleted { intent_id, response } => {
                self.handle_llm_response(&response.content, *intent_id, graph)
            }
            KernelEvent::ToolCompleted { tool, result, .. } => {
                self.handle_tool_result(tool, result, graph)
            }
            KernelEvent::ApprovalGiven { outcome, .. } => {
                self.handle_approval_result(outcome, graph)
            }
            KernelEvent::WorkerCompleted { worker_id, result } => {
                self.handle_worker_result(&worker_id.0.to_string(), result.as_ref().ok(), graph)
            }
            KernelEvent::WorkerFailed { worker_id, error: _, .. } => {
                self.handle_worker_result(&worker_id.0.to_string(), None, graph)
            }
            KernelEvent::RuntimeError { error, .. } => {
                crate::error_log!("[LLM_KERNEL] Runtime error: {}", error);
                // Emit halt on runtime error
                graph.add(IntentNode::new(
                    self.next_intent_id(),
                    Intent::Halt(ExitReason::Error(error.clone())),
                ));
                Ok(())
            }
            _ => Ok(()), // Ignore other events
        }
    }
    
    /// Handle user message - requests LLM
    fn handle_user_message(&mut self, content: &str, graph: &mut IntentGraph) -> Result<(), KernelError> {
        if self.check_limits(graph)? {
            return Ok(());
        }
        
        // Increment step BEFORE getting intent ID to avoid collision with completed intents
        self.state.increment_step();
        
        // Add message to history
        self.state.history.push(Message {
            role: Role::User,
            content: content.to_string(),
        });
        
        let context = self.build_context(&format!("User: {}\n\nWhat should I do?", content));
        
        let llm_intent_id = self.next_intent_id();
        crate::info_log!("[PLANNER] Adding LLM intent {} to graph for tool result interpretation", llm_intent_id.0);
        // Track this as a new LLM request (retry count = 0)
        self.state.llm_retry_counts.insert(llm_intent_id, 0);
        
        graph.add(IntentNode::new(
            llm_intent_id,
            Intent::RequestLLM(LLMRequest {
                context,
                max_tokens: None,
                temperature: None,
                model: None,
                response_format: None,
                stream: false,
                retry_attempt: 0,
                extra_system_messages: Vec::new(),
            }),
        ));
        crate::info_log!("[PLANNER] Graph now has {} nodes, step_count={}", graph.len(), self.state.step_count);
        
        Ok(())
    }
    
    /// Maximum retry attempts for format correction
    const MAX_FORMAT_RETRIES: u32 = 2;

    /// Handle LLM response - parse and act
    /// 
    /// Implements format validation and retry logic:
    /// 1. Parse response BEFORE adding to history
    /// 2. If valid JSON format: add to history and proceed
    /// 3. If XML/invalid and retries left: emit retry intent with corrective message
    /// 4. If max retries reached: emit error response
    fn handle_llm_response(&mut self, content: &str, intent_id: IntentId, graph: &mut IntentGraph) -> Result<(), KernelError> {
        if self.check_limits(graph)? {
            return Ok(());
        }

        // Check for XML format (common LLM error)
        let contains_xml = content.contains("<tool_call") 
            || content.contains("<function=")
            || content.contains("<parameter=");
        
        // Try to parse the response
        let parse_result = self.parser.parse_to_response(content);
        let is_valid = !contains_xml && parse_result.is_ok();

        // Get retry count for this request chain
        let retry_count = self.state.llm_retry_counts.get(&intent_id).copied().unwrap_or(0);

        if !is_valid && retry_count < Self::MAX_FORMAT_RETRIES {
            // Format invalid - trigger retry WITHOUT adding to history
            crate::warn_log!("[PLANNER] LLM output invalid format (XML={}, retry={}/{}), requesting retry", 
                contains_xml, retry_count + 1, Self::MAX_FORMAT_RETRIES);

            // Build corrective system message
            let correction = if contains_xml {
                "⚠️ CORRECTION: You output XML format. Use ONLY Short-Key JSON like: {\"t\": \"reasoning\", \"a\": \"tool\", \"i\": {...}}. NEVER use <tool_call> tags."
            } else {
                "⚠️ CORRECTION: Your response was not valid Short-Key JSON. Use ONLY JSON format like: {\"t\": \"reasoning\", \"f\": \"response\"}"
            };

            // Create retry request with same context but extra system message
            let retry_context = self.build_context("Please provide your response in the correct Short-Key JSON format.");
            
            // Increment retry count for tracking
            let new_intent_id = self.next_intent_id();
            self.state.llm_retry_counts.insert(new_intent_id, retry_count + 1);

            // Build retry request with corrective message
            let retry_request = LLMRequest {
                context: retry_context,
                max_tokens: None,
                temperature: Some(0.7), // Slightly lower temp for more deterministic output
                model: None,
                response_format: None,
                stream: false, // Don't stream retries
                retry_attempt: retry_count + 1,
                extra_system_messages: vec![correction.to_string()],
            };

            graph.add(IntentNode::new(
                new_intent_id,
                Intent::RequestLLM(retry_request),
            ));

            crate::info_log!("[PLANNER] Added retry intent {} with corrective message", new_intent_id.0);
            return Ok(());
        }

        // Add assistant message to history (only for valid responses or after max retries)
        self.state.history.push(Message {
            role: Role::Assistant,
            content: content.to_string(),
        });

        // Clean up retry tracking for this intent
        self.state.llm_retry_counts.remove(&intent_id);
        
        match parse_result {
            Ok(ParsedResponse::ToolCalls(calls)) => {
                if let Some(call) = calls.into_iter().next() {
                    let args_str = call.arguments.to_string();
                    
                    // Check approval
                    if self.approval_policy.check(&call.name, &args_str) {
                        crate::info_log!("[LLM_KERNEL] Adding pending approval for tool: {}, args: {}", call.name, args_str);
                        
                        // Generate intent ID once and reuse (step already incremented at function start)
                        let approval_intent_id = self.next_intent_id();
                        
                        self.state.pending_approvals.push(PendingApproval {
                            intent_id: approval_intent_id,
                            tool: call.name.clone(),
                            args: args_str.clone(),
                            requested_at: std::time::SystemTime::now(),
                        });
                        
                        let tool_name = call.name.clone();
                        graph.add(IntentNode::new(
                            approval_intent_id,
                            Intent::RequestApproval(crate::agent::types::intents::ApprovalRequest {
                                tool: call.name,
                                args: args_str,
                                reason: format!("Tool '{}' requires approval", tool_name),
                            }),
                        ));
                    } else {
                        graph.add(IntentNode::new(
                            self.next_intent_id(),
                            Intent::CallTool(call),
                        ));
                    }
                }
            }
            Ok(ParsedResponse::FinalAnswer(answer)) => {
                graph.add(IntentNode::new(
                    self.next_intent_id(),
                    Intent::EmitResponse(answer),
                ));
            }
            Ok(ParsedResponse::Remember { content: memory, .. }) => {
                graph.add(IntentNode::new(
                    self.next_intent_id(),
                    Intent::Remember { content: memory },
                ));
            }
            Ok(ParsedResponse::RememberAndCall { content: _, tool }) => {
                // For now, prioritize the tool call
                let args_str = tool.arguments.to_string();
                
                if self.approval_policy.check(&tool.name, &args_str) {
                    let tool_name = tool.name.clone();
                    graph.add(IntentNode::new(
                        self.next_intent_id(),
                        Intent::RequestApproval(crate::agent::types::intents::ApprovalRequest {
                            tool: tool.name,
                            args: args_str,
                            reason: format!("Tool '{}' requires approval", tool_name),
                        }),
                    ));
                } else {
                    graph.add(IntentNode::new(
                        self.next_intent_id(),
                        Intent::CallTool(tool),
                    ));
                }
            }
            Ok(ParsedResponse::ConfirmRequest { tool, .. }) => {
                graph.add(IntentNode::new(
                    self.next_intent_id(),
                    Intent::RequestApproval(crate::agent::types::intents::ApprovalRequest {
                        tool: tool.name,
                        args: tool.arguments.to_string(),
                        reason: "Tool requires confirmation".to_string(),
                    }),
                ));
            }
            Ok(ParsedResponse::Malformed { error, .. }) => {
                graph.add(IntentNode::new(
                    self.next_intent_id(),
                    Intent::EmitResponse(format!("Parse error: {}", error)),
                ));
            }
            Err(_) => {
                // Fallback: emit raw response
                graph.add(IntentNode::new(
                    self.next_intent_id(),
                    Intent::EmitResponse(content.trim().to_string()),
                ));
            }
        }
        
        Ok(())
    }
    
    /// Handle tool result - request LLM interpretation
    fn handle_tool_result(
        &mut self,
        tool: &str,
        result: &crate::agent::types::events::ToolResult,
        graph: &mut IntentGraph,
    ) -> Result<(), KernelError> {
        crate::info_log!("[PLANNER] handle_tool_result called for tool: {} with result type", tool);
        if self.check_limits(graph)? {
            crate::warn_log!("[PLANNER] check_limits returned true, step_count={} max_steps={}, not adding LLM intent", self.state.step_count, self.state.max_steps);
            return Ok(());
        }
        
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
        
        // Add tool message to history
        self.state.history.push(Message {
            role: Role::Tool,
            content: format!("Tool '{}' {}: {}", tool, status, output),
        });
        
        // Don't include tool output in scratchpad - it's already in history
        // This avoids duplicate content that can trigger WAF
        let scratchpad = format!(
            "The {} tool {}. What should I do next?",
            tool, status
        );
        let context = self.build_context(&scratchpad);
        
        let llm_intent_id = self.next_intent_id();
        crate::info_log!("[PLANNER] Adding LLM intent {} (step_count={}) to graph for tool result interpretation", llm_intent_id.0, self.state.step_count);
        
        // Track this as a new LLM request (retry count = 0)
        self.state.llm_retry_counts.insert(llm_intent_id, 0);
        
        graph.add(IntentNode::new(
            llm_intent_id,
            Intent::RequestLLM(LLMRequest {
                context,
                max_tokens: None,
                temperature: None,
                model: None,
                response_format: None,
                stream: false,
                retry_attempt: 0,
                extra_system_messages: Vec::new(),
            }),
        ));
        
        Ok(())
    }
    
    /// Handle approval result
    fn handle_approval_result(
        &mut self,
        outcome: &crate::agent::types::events::ApprovalOutcome,
        graph: &mut IntentGraph,
    ) -> Result<(), KernelError> {
        crate::info_log!("[LLM_KERNEL] handle_approval_result called with outcome: {:?}", outcome);
        crate::info_log!("[LLM_KERNEL] pending_approvals count: {}", self.state.pending_approvals.len());
        match outcome {
            crate::agent::types::events::ApprovalOutcome::Granted => {
                // Execute the pending tool that was approved
                if let Some(pending) = self.state.pending_approvals.pop() {
                    crate::info_log!("[LLM_KERNEL] Approval granted for tool: {}, args: {}", pending.tool, pending.args);
                    
                    // We need to re-parse or store the original tool call
                    // For now, emit a shell intent as a placeholder
                    // TODO: Store original ToolCall in PendingApproval
                    graph.add(IntentNode::new(
                        self.next_intent_id(),
                        Intent::CallTool(crate::agent::types::intents::ToolCall {
                            name: pending.tool,
                            arguments: serde_json::from_str(&pending.args).unwrap_or_default(),
                            working_dir: None,
                            timeout_secs: None,
                        }),
                    ));
                } else {
                    crate::warn_log!("[LLM_KERNEL] Approval granted but no pending tool found");
                }
            }
            crate::agent::types::events::ApprovalOutcome::Denied { .. } => {
                self.state.pending_approvals.clear();
                
                let scratchpad = "Tool execution was denied by user. What should I do instead?";
                let context = self.build_context(scratchpad);
                
                let llm_intent_id = self.next_intent_id();
                // Track this as a new LLM request (retry count = 0)
                self.state.llm_retry_counts.insert(llm_intent_id, 0);
                
                graph.add(IntentNode::new(
                    llm_intent_id,
                    Intent::RequestLLM(LLMRequest {
                        context,
                        max_tokens: None,
                        temperature: None,
                        model: None,
                        response_format: None,
                        stream: false,
                        retry_attempt: 0,
                        extra_system_messages: Vec::new(),
                    }),
                ));
            }
        }
        
        Ok(())
    }
    
    /// Handle worker result
    fn handle_worker_result(
        &mut self,
        worker_id: &str,
        result: Option<&String>,
        graph: &mut IntentGraph,
    ) -> Result<(), KernelError> {
        if self.check_limits(graph)? {
            return Ok(());
        }
        
        let output = match result {
            Some(output) => format!("Worker {} completed: {}", worker_id, output),
            None => format!("Worker {} failed", worker_id),
        };
        
        let scratchpad = format!("{}\n\nWhat should I do next?", output);
        let context = self.build_context(&scratchpad);
        
        let llm_intent_id = self.next_intent_id();
        // Track this as a new LLM request (retry count = 0)
        self.state.llm_retry_counts.insert(llm_intent_id, 0);
        
        graph.add(IntentNode::new(
            llm_intent_id,
            Intent::RequestLLM(LLMRequest {
                context,
                max_tokens: None,
                temperature: None,
                model: None,
                response_format: None,
                stream: false,
                retry_attempt: 0,
                extra_system_messages: Vec::new(),
            }),
        ));
        
        Ok(())
    }
    
    /// Check step limits and emit halt if exceeded
    fn check_limits(&mut self, graph: &mut IntentGraph) -> Result<bool, KernelError> {
        if self.state.at_limit() {
            graph.add(IntentNode::new(
                self.next_intent_id(),
                Intent::Halt(ExitReason::StepLimit),
            ));
            Ok(true)
        } else {
            Ok(false)
        }
    }
    
    /// Build LLM context with current state
    fn build_context(&self, scratchpad: &str) -> Context {
        Context::new(scratchpad.to_string())
            .with_system(self.system_prompt.clone())
            .with_history(self.state.history.clone())
            .with_tools(build_tool_defs(&self.tool_descriptions))
    }
    
    /// Generate next intent ID
    fn next_intent_id(&mut self) -> IntentId {
        let seq = self.state.next_intent_seq();
        IntentId::from_seq(seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_planner_init() {
        let mut planner = Planner::new();
        let config = KernelConfig::default().with_max_steps(10);
        planner.init(config).unwrap();
        assert_eq!(planner.state().max_steps, 10);
    }
    
    #[test]
    fn test_process_user_message() {
        let mut planner = Planner::new();
        planner.init(KernelConfig::default()).unwrap();
        
        let events = vec![KernelEvent::UserMessage {
            content: "hello".to_string(),
        }];
        
        let graph = planner.process(&events).unwrap();
        assert!(!graph.is_empty());
    }
}
