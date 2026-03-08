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
    intents::{Intent, ExitReason, Context, LLMRequest},
    events::KernelEvent,
    config::KernelConfig,
    parser::{ShortKeyParser, ParsedResponse, ShortKeyExtracted},
};
use crate::conversation::manager::Message;

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
    
    /// Initialize the planner with conversation history
    /// 
    /// This is used when restoring a session from persisted state.
    /// The history is added to the agent's state without triggering any responses.
    pub fn with_history(mut self, history: Vec<Message>) -> Self {
        self.state.history = history;
        crate::info_log!("[PLANNER] Initialized with {} messages from history", self.state.history.len());
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
    
    /// Check if message is casual chitchat (greeting, thanks, etc.)
    /// 
    /// Returns true for messages that don't need tool-based investigation
    fn is_chitchat(content: &str) -> bool {
        let lower = content.trim().to_lowercase();
        
        // Greetings
        if lower.starts_with("hi") || lower.starts_with("hello") || lower.starts_with("hey") {
            return true;
        }
        
        // Short acknowledgments/thanks
        if ["ok", "okay", "got it", "thanks", "thank you", "nice", "great", "cool"]
            .iter()
            .any(|&s| lower == s || lower.starts_with(s))
        {
            return true;
        }
        
        // Simple questions that don't need investigation
        if lower.starts_with("how are you") || lower.starts_with("what's up") || lower.starts_with("sup") {
            return true;
        }
        
        false
    }

    /// Handle user message - requests LLM
    fn handle_user_message(&mut self, content: &str, graph: &mut IntentGraph) -> Result<(), KernelError> {
        if self.check_limits(graph)? {
            return Ok(());
        }
        
        // Increment step BEFORE getting intent ID to avoid collision with completed intents
        self.state.increment_step();
        
        // Add message to history
        self.state.history.push(Message::new("user", content));
        
        // Use different prompt based on message type:
        // - Chitchat: Just respond conversationally
        // - Task: Be proactive with "What should I do?"
        let prompt = if Self::is_chitchat(content) {
            format!("User: {}\n\nRespond conversationally. Do NOT use tools for greetings or casual chat.", content)
        } else {
            format!("User: {}\n\nWhat should I do?", content)
        };
        
        let context = self.build_context(&prompt);
        
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
    
    /// Y-SWITCH: Handle Short-Key extracted fields
    /// 
    /// The planner acts as a traffic controller, routing to different "tracks" based on
    /// what fields are present in the extracted response. This is the heart of the Y-switch design.
    /// 
    /// Track 1 (Remember): If "r" field present → Intent::Remember (fire-and-forget)
    /// Track 2 (Tool): If "a" field present → Intent::CallTool or Intent::RequestApproval
    /// Track 3 (Final): If "f" field present → Intent::EmitResponse
    /// 
    /// Tracks can run in parallel (no dependencies) or sequentially based on semantics.
    fn handle_short_key_response(
        &mut self,
        extracted: ShortKeyExtracted,
        graph: &mut IntentGraph,
    ) -> Result<(), KernelError> {
        crate::info_log!("[PLANNER] Y-SWITCH processing: r={}, a={}, f={}, c={}",
            extracted.remember.is_some(),
            extracted.tool_call.is_some(),
            extracted.final_answer.is_some(),
            extracted.confirm
        );

        // Track 1: REMEMBER (fire-and-forget, no dependencies)
        // The "r" field is saved to memory asynchronously - doesn't block other tracks
        let has_remember = extracted.remember.is_some();
        let _remember_node = if let Some(content) = extracted.remember {
            let node_id = self.next_intent_id();
            crate::info_log!("[PLANNER] Track 1: Creating Remember intent {}", node_id.0);
            graph.add(IntentNode::new(
                node_id,
                Intent::Remember { content },
            ));
            Some(node_id)
        } else {
            None
        };

        // Track 2: TOOL CALL (may need approval)
        // If "c" flag is set, we create RequestApproval instead of CallTool
        let has_tool = extracted.tool_call.is_some();
        let tool_node = if let Some(tool) = extracted.tool_call {
            let args_str = tool.arguments.to_string();
            let node_id = self.next_intent_id();

            // Check if this is a suggestion (no approval needed)
            let is_suggestion = tool.arguments.get("mode")
                .and_then(|v| v.as_str())
                .map(|s| s == "suggest")
                .unwrap_or(false);

            let tool_name = tool.name.clone();
            if !is_suggestion && (extracted.confirm || self.approval_policy.check(&tool_name, &args_str)) {
                crate::info_log!("[PLANNER] Track 2: Creating RequestApproval intent {} for tool '{}'",
                    node_id.0, tool_name);
                
                self.state.pending_approvals.push(PendingApproval {
                    intent_id: node_id,
                    tool: tool_name.clone(),
                    args: args_str.clone(),
                    requested_at: std::time::SystemTime::now(),
                });

                graph.add(IntentNode::new(
                    node_id,
                    Intent::RequestApproval(crate::agent::types::intents::ApprovalRequest {
                        tool: tool_name.clone(),
                        args: args_str,
                        reason: format!("Tool '{}' requires approval", tool_name),
                    }),
                ));
            } else {
                crate::info_log!("[PLANNER] Track 2: Creating CallTool intent {} for tool '{}'",
                    node_id.0, tool_name);
                graph.add(IntentNode::new(
                    node_id,
                    Intent::CallTool(tool),
                ));
            }
            Some(node_id)
        } else {
            None
        };

        // Track 3: FINAL ANSWER (user-facing response)
        // If there's a tool call, the final answer should come AFTER tool completion
        // So we add a dependency edge: tool_node -> emit_node
        if let Some(answer) = extracted.final_answer {
            let node_id = self.next_intent_id();
            crate::info_log!("[PLANNER] Track 3: Creating EmitResponse intent {}", node_id.0);
            
            let mut emit_node = IntentNode::new(
                node_id,
                Intent::EmitResponse(answer),
            );

            // If there's a tool call, emit response depends on tool completion
            if let Some(tool_id) = tool_node {
                emit_node = emit_node.depends_on(tool_id);
                crate::info_log!("[PLANNER] Track 3: Adding dependency: emit {} depends on tool {}",
                    node_id.0, tool_id.0);
            }

            graph.add(emit_node);
        } else if extracted.thought.is_empty() && !has_remember && !has_tool {
            // Nothing to do - malformed response
            crate::warn_log!("[PLANNER] Y-SWITCH: No actionable fields found in response");
            graph.add(IntentNode::new(
                self.next_intent_id(),
                Intent::EmitResponse("I received your message but I'm not sure how to respond.".to_string()),
            ));
        }

        crate::info_log!("[PLANNER] Y-SWITCH complete: {} nodes in graph", graph.len());
        Ok(())
    }
    
    /// Maximum retry attempts for format correction
    const MAX_FORMAT_RETRIES: u32 = 2;

    /// Check if content appears to be a plain text/markdown response (not a tool attempt)
    /// 
    /// Returns true if the content looks like natural language text that should be
    /// emitted directly to the user, rather than a malformed tool call.
    fn is_plain_text_response(content: &str) -> bool {
        let trimmed = content.trim();
        
        // Empty check
        if trimmed.is_empty() {
            return false;
        }
        
        // If it contains JSON-like structures, it's likely a tool attempt
        // Look for patterns like {"key": or {"t": or <function= etc.
        let has_json_object = trimmed.contains("{") && trimmed.contains("}") && 
            (trimmed.contains("\"") || trimmed.contains("'"));
        
        // Check for XML-style tool calls
        let has_xml_tool_call = trimmed.contains("<function=") || trimmed.contains("<tool_call>");
        
        // If it has JSON or XML structures, treat as a tool attempt (not plain text)
        if has_json_object || has_xml_tool_call {
            return false;
        }
        
        // It's plain text if it looks like natural language
        // - Contains words/sentences
        // - Has common punctuation
        // - No code-like structures
        let word_count = trimmed.split_whitespace().count();
        let has_sentences = trimmed.contains('.') || trimmed.contains('!') || trimmed.contains('?');
        let has_markdown = trimmed.contains("##") || trimmed.contains("**") || trimmed.contains("- ");
        
        (word_count > 3 && has_sentences) || has_markdown
    }

    /// Handle LLM response - parse and act
    /// 
    /// Implements format validation and retry logic:
    /// 1. Parse response BEFORE adding to history
    /// 2. If valid JSON format: add to history and proceed (Y-SWITCH routing)
    /// 3. If plain text/markdown: accept as final answer (no retry needed)
    /// 4. If XML/invalid tool format and retries left: emit retry intent
    /// 5. If max retries reached: emit error response
    fn handle_llm_response(&mut self, content: &str, intent_id: IntentId, graph: &mut IntentGraph) -> Result<(), KernelError> {
        if self.check_limits(graph)? {
            return Ok(());
        }

        // Try to parse the response (parser handles both JSON and XML formats)
        let parse_result = self.parser.parse_to_response(content);
        let is_valid = parse_result.is_ok();

        // Get retry count for this request chain
        let retry_count = self.state.llm_retry_counts.get(&intent_id).copied().unwrap_or(0);

        if !is_valid && retry_count < Self::MAX_FORMAT_RETRIES {
            // Check if this is plain text/markdown - if so, accept as final answer
            if Self::is_plain_text_response(content) {
                crate::info_log!("[PLANNER] Plain text/markdown response detected, accepting as final answer");
                
                // Add to history and emit directly
                self.state.history.push(Message::new("assistant", content));
                self.state.llm_retry_counts.remove(&intent_id);
                
                graph.add(IntentNode::new(
                    self.next_intent_id(),
                    Intent::EmitResponse(content.trim().to_string()),
                ));
                return Ok(());
            }
            
            // Format invalid (likely a malformed tool call) - trigger retry WITHOUT adding to history
            crate::warn_log!("[PLANNER] LLM output invalid format (retry={}/{}), requesting retry", 
                retry_count + 1, Self::MAX_FORMAT_RETRIES);

            // Build corrective system message
            let correction = "⚠️ CORRECTION: Your response was not valid Short-Key JSON. Use ONLY JSON format like: {\"t\": \"reasoning\", \"a\": \"tool\", \"i\": {...}} or {\"t\": \"reasoning\", \"f\": \"response\"}";

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
        self.state.history.push(Message::new("assistant", content));

        // Clean up retry tracking for this intent
        self.state.llm_retry_counts.remove(&intent_id);
        
        // Y-SWITCH: The planner is the traffic controller
        // It takes the extracted Short-Key fields and creates appropriate intents
        match parse_result {
            Ok(ParsedResponse::ShortKey(extracted)) => {
                self.handle_short_key_response(extracted, graph)?;
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
        
        // Skip follow-up LLM request for suggestions - command is in terminal, user handles it
        if output.starts_with("SUGGESTED_COMMAND: ") {
            crate::info_log!("[PLANNER] Tool result is a suggestion, skipping LLM follow-up");
            // Still add to history but don't request interpretation
            self.state.history.push(Message::new("tool", output.clone()));
            return Ok(());
        }
        
        // Add tool message to history
        self.state.history.push(Message::new("tool", format!("Tool '{}' {}: {}", tool, status, output)));
        
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
    
    #[test]
    fn test_is_plain_text_response_markdown() {
        // Markdown responses should be accepted as plain text
        let markdown = "## Next Steps\n\n1. **Install** the package\n2. Run the command";
        assert!(Planner::is_plain_text_response(markdown));
    }
    
    #[test]
    fn test_is_plain_text_response_natural_language() {
        // Natural language should be accepted
        let text = "That's a great question! Let me help you with that.";
        assert!(Planner::is_plain_text_response(text));
    }
    
    #[test]
    fn test_is_plain_text_response_short_message() {
        // Very short messages without punctuation might not be plain text
        let short = "hi there";
        assert!(!Planner::is_plain_text_response(short));
    }
    
    #[test]
    fn test_is_plain_text_response_json_tool_call() {
        // JSON should NOT be plain text (it's a tool attempt)
        let json = r#"{"t": "Thinking", "a": "shell", "i": {"command": "ls"}}"#;
        assert!(!Planner::is_plain_text_response(json));
    }
    
    #[test]
    fn test_is_plain_text_response_json_final_answer() {
        // JSON with final answer should NOT be plain text
        let json = r#"{"t": "Done", "f": "Here is the result"}"#;
        assert!(!Planner::is_plain_text_response(json));
    }
    
    #[test]
    fn test_is_plain_text_response_xml_tool_call() {
        // XML-style tool calls should NOT be plain text
        let xml = r#"<function=shell><parameter=command>ls</parameter></function>"#;
        assert!(!Planner::is_plain_text_response(xml));
    }
    
    #[test]
    fn test_is_plain_text_response_empty() {
        assert!(!Planner::is_plain_text_response(""));
        assert!(!Planner::is_plain_text_response("   "));
    }
    
    #[test]
    fn test_is_plain_text_response_bullet_list() {
        // Markdown bullet lists should be plain text
        let bullets = "- Item one\n- Item two\n- Item three";
        assert!(Planner::is_plain_text_response(bullets));
    }
}
