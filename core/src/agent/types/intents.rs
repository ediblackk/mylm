//! Intent types - what the kernel wants to do
//!
//! Intents represent what the kernel WANTS to do, not what it did.
//! They are side-effect requests that the runtime will execute.

use serde::{Deserialize, Serialize};
use super::ids::IntentId;

/// An intent - a request to perform a side effect
/// 
/// The kernel emits these but does not execute them.
/// The runtime executes them and returns observations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Intent {
    /// Call a tool with arguments
    CallTool(ToolCall),

    /// Request an LLM completion
    RequestLLM(LLMRequest),

    /// Request user approval for an action
    RequestApproval(ApprovalRequest),

    /// Spawn a worker with an objective
    SpawnWorker(WorkerSpec),

    /// Emit a response to the user
    EmitResponse(String),

    /// Halt execution with a reason
    Halt(ExitReason),
}

/// A tool call intent
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Name of the tool to call
    pub name: String,
    /// Arguments as JSON
    pub arguments: serde_json::Value,
    /// Working directory for the tool
    pub working_dir: Option<String>,
    /// Timeout in seconds
    pub timeout_secs: Option<u64>,
}

impl ToolCall {
    /// Create a simple tool call
    pub fn new(name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            arguments,
            working_dir: None,
            timeout_secs: None,
        }
    }

    /// Set working directory
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }
}

/// An LLM request intent
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LLMRequest {
    /// The prompt/context
    pub context: Context,
    /// Maximum tokens to generate
    pub max_tokens: Option<u32>,
    /// Temperature (0.0 - 2.0)
    pub temperature: Option<f32>,
    /// Specific model to use (optional override)
    pub model: Option<String>,
    /// Request structured output
    pub response_format: Option<ResponseFormat>,
    /// Whether to enable streaming
    pub stream: bool,
}

impl LLMRequest {
    /// Create a basic LLM request
    pub fn new(context: Context) -> Self {
        Self {
            context,
            max_tokens: None,
            temperature: None,
            model: None,
            response_format: None,
            stream: false,
        }
    }

    /// Enable streaming
    pub fn with_streaming(mut self) -> Self {
        self.stream = true;
        self
    }

    /// Request JSON output
    pub fn with_json(mut self) -> Self {
        self.response_format = Some(ResponseFormat::JsonObject);
        self
    }

    /// Set temperature
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }
}

/// Context for LLM request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context {
    /// Conversation history
    pub history: Vec<Message>,
    /// System prompt
    pub system_prompt: String,
    /// Available tools
    pub available_tools: Vec<ToolDef>,
    /// Current scratchpad/thoughts
    pub scratchpad: String,
    /// Token budget
    pub token_budget: TokenBudget,
}

impl Context {
    /// Create minimal context
    pub fn new(scratchpad: impl Into<String>) -> Self {
        Self {
            history: Vec::new(),
            system_prompt: String::new(),
            available_tools: Vec::new(),
            scratchpad: scratchpad.into(),
            token_budget: TokenBudget::default(),
        }
    }

    /// With system prompt
    pub fn with_system(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }
}

/// Message in conversation history
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// Message role
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

/// Tool definition for available tools
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Tool schema for describing available tools (alias for ToolDef)
pub type ToolSchema = ToolDef;

/// Token budget
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenBudget {
    pub max_tokens: u32,
    pub reserved_for_output: u32,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            max_tokens: 4096,
            reserved_for_output: 1024,
        }
    }
}

/// Response format specification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ResponseFormat {
    /// Free-form text
    Text,
    /// JSON object
    JsonObject,
    /// JSON with specific schema
    JsonSchema { schema: serde_json::Value },
    /// XML
    Xml,
}

/// Worker specification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerSpec {
    /// Objective for the worker
    pub objective: String,
    /// Context to pass to worker
    pub context: String,
    /// Maximum iterations for the worker
    pub max_iterations: Option<usize>,
    /// Whether worker can spawn sub-workers
    pub can_delegate: bool,
    /// Specific tools available to worker
    pub allowed_tools: Option<Vec<String>>,
    /// Model override for worker
    pub model: Option<String>,
}

impl WorkerSpec {
    /// Create a worker with an objective
    pub fn new(objective: impl Into<String>) -> Self {
        Self {
            objective: objective.into(),
            context: String::new(),
            max_iterations: None,
            can_delegate: false,
            allowed_tools: None,
            model: None,
        }
    }

    /// Set context for the worker
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = context.into();
        self
    }

    /// Allow delegation
    pub fn with_delegation(mut self) -> Self {
        self.can_delegate = true;
        self
    }

    /// Limit allowed tools
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = Some(tools);
        self
    }
}

/// Request for user approval
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Tool being requested
    pub tool: String,
    /// Arguments to the tool
    pub args: String,
    /// Reason for the request
    pub reason: String,
}

/// Exit reason for halting
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExitReason {
    /// Completed successfully
    Completed,
    /// User requested exit
    UserRequest,
    /// Hit step limit
    StepLimit,
    /// Error occurred
    Error(String),
    /// Interrupted
    Interrupted,
}

/// Priority for intent execution
/// 
/// This is a hint to the runtime about execution order
/// when multiple intents are ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    /// Execute immediately - user-facing response
    Critical = 0,
    /// Execute ASAP - important tool calls
    High = 1,
    /// Default priority
    Normal = 2,
    /// Can be delayed - background tasks
    Background = 3,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

/// An intent node in a DAG
/// 
/// Combines an intent with dependency information and metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentNode {
    /// Unique ID for this intent
    pub id: IntentId,
    /// The intent itself
    pub intent: Intent,
    /// IDs of intents that must complete before this one
    pub dependencies: Vec<IntentId>,
    /// Execution priority
    pub priority: Priority,
    /// Optional timeout for this intent
    pub timeout_secs: Option<u64>,
    /// Whether this intent can be retried on failure
    pub retryable: bool,
    /// Maximum retry attempts
    pub max_retries: u32,
}

impl IntentNode {
    /// Create a new intent node
    pub fn new(id: IntentId, intent: Intent) -> Self {
        Self {
            id,
            intent,
            dependencies: Vec::new(),
            priority: Priority::Normal,
            timeout_secs: None,
            retryable: false,
            max_retries: 0,
        }
    }

    /// Add a dependency
    pub fn depends_on(mut self, id: IntentId) -> Self {
        if !self.dependencies.contains(&id) {
            self.dependencies.push(id);
        }
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Make retryable
    pub fn with_retry(mut self, max_retries: u32) -> Self {
        self.retryable = true;
        self.max_retries = max_retries;
        self
    }

    /// Check if this node is ready given completed intents
    pub fn is_ready(&self, completed: &[IntentId]) -> bool {
        self.dependencies.iter().all(|dep| completed.contains(dep))
    }

    /// Check if this node has no dependencies
    pub fn has_no_dependencies(&self) -> bool {
        self.dependencies.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_node_builder() {
        let node = IntentNode::new(IntentId::new(1), Intent::EmitResponse("hello".to_string()))
            .with_priority(Priority::High)
            .with_timeout(30);

        assert_eq!(node.id.0, 1);
        assert_eq!(node.priority, Priority::High);
        assert_eq!(node.timeout_secs, Some(30));
        assert!(node.is_ready(&[]));
    }

    #[test]
    fn test_intent_node_dependencies() {
        let node = IntentNode::new(IntentId::new(2), Intent::EmitResponse("world".to_string()))
            .depends_on(IntentId::new(1));

        assert!(!node.is_ready(&[]));
        assert!(node.is_ready(&[IntentId::new(1)]));
        assert!(node.is_ready(&[IntentId::new(1), IntentId::new(3)]));
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical < Priority::High);
        assert!(Priority::High < Priority::Normal);
        assert!(Priority::Normal < Priority::Background);
    }

    #[test]
    fn test_tool_call_builder() {
        let call = ToolCall::new("read_file", serde_json::json!({"path": "/tmp/test"}))
            .with_timeout(60);

        assert_eq!(call.name, "read_file");
        assert_eq!(call.timeout_secs, Some(60));
    }
}
