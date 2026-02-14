//! Configuration for the Agency Kernel
//!
//! IMPORTANT: KernelConfig contains ONLY descriptors and metadata.
//! NO executors, NO channels, NO runtime resources.
//!
//! The kernel is pure. It only needs to know:
//! - What tools exist (schemas, not implementations)
//! - What policies apply (rules, not enforcers)
//! - What limits exist (numbers, not counters)

use serde::{Deserialize, Serialize};

use super::events::ToolSchema;

/// Configuration for the kernel
/// 
/// This is pure data - no executors, no async resources.
/// The kernel uses this to make decisions, not to execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelConfig {
    /// Maximum steps before halting (safety limit)
    pub max_steps: usize,

    /// Maximum depth of nested workers
    pub max_worker_depth: usize,

    /// Maximum workers active at once
    pub max_concurrent_workers: usize,

    /// Tool schemas - what tools exist and their signatures
    /// The kernel uses these to validate tool calls, not to execute them
    pub tool_schemas: Vec<ToolSchema>,

    /// Policies for approval and execution
    pub policies: PolicySet,

    /// Worker limits and defaults
    pub worker_limits: WorkerLimits,

    /// Prompt configuration
    pub prompt_config: PromptConfig,

    /// Feature flags
    pub features: FeatureFlags,
}

impl KernelConfig {
    /// Create a default configuration
    pub fn new() -> Self {
        Self {
            max_steps: 50,
            max_worker_depth: 3,
            max_concurrent_workers: 5,
            tool_schemas: Vec::new(),
            policies: PolicySet::default(),
            worker_limits: WorkerLimits::default(),
            prompt_config: PromptConfig::default(),
            features: FeatureFlags::default(),
        }
    }

    /// Set max steps
    pub fn with_max_steps(mut self, steps: usize) -> Self {
        self.max_steps = steps;
        self
    }

    /// Add a tool schema
    pub fn with_tool(mut self, schema: ToolSchema) -> Self {
        self.tool_schemas.push(schema);
        self
    }

    /// Set policies
    pub fn with_policies(mut self, policies: PolicySet) -> Self {
        self.policies = policies;
        self
    }

    /// Find a tool schema by name
    pub fn find_tool(&self, name: &str) -> Option<&ToolSchema> {
        self.tool_schemas.iter().find(|t| t.name == name)
    }

    /// Check if a tool exists
    pub fn has_tool(&self, name: &str) -> bool {
        self.find_tool(name).is_some()
    }
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Set of policies for the kernel
/// 
/// Policies are pure rules - they describe WHAT should happen,
/// not HOW to enforce it. The runtime enforces these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicySet {
    /// Approval policy - when to request user approval
    pub approval: ApprovalPolicy,

    /// Tool policy - which tools are allowed
    pub tools: ToolPolicy,

    /// Worker policy - worker spawning rules
    pub workers: WorkerPolicy,

    /// Safety policy - content filtering, etc.
    pub safety: SafetyPolicy,
}

impl PolicySet {
    /// Create default policies
    pub fn new() -> Self {
        Self {
            approval: ApprovalPolicy::default(),
            tools: ToolPolicy::default(),
            workers: WorkerPolicy::default(),
            safety: SafetyPolicy::default(),
        }
    }

    /// Check if a tool requires approval based on policy
    pub fn requires_approval(&self, tool: &str, args: &str) -> bool {
        self.approval.requires_approval(tool, args)
    }

    /// Check if a tool is allowed
    pub fn is_tool_allowed(&self, tool: &str) -> bool {
        self.tools.is_allowed(tool)
    }
}

impl Default for PolicySet {
    fn default() -> Self {
        Self::new()
    }
}

/// Policy for when to request user approval
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalPolicy {
    /// Require approval for all destructive operations
    pub require_approval_for_destructive: bool,

    /// Require approval for network operations
    pub require_approval_for_network: bool,

    /// Require approval for specific tools
    pub require_approval_for_tools: Vec<String>,

    /// Don't require approval for these specific tools
    pub auto_approve_tools: Vec<String>,

    /// Require approval if estimated cost exceeds this (in USD)
    pub cost_threshold_usd: Option<f64>,
}

impl ApprovalPolicy {
    /// Check if a tool call requires approval
    pub fn requires_approval(&self, tool: &str, _args: &str) -> bool {
        // Auto-approved tools never require approval
        if self.auto_approve_tools.contains(&tool.to_string()) {
            return false;
        }

        // Specific tools always require approval
        if self.require_approval_for_tools.contains(&tool.to_string()) {
            return true;
        }

        // Default policies based on tool name patterns
        if self.require_approval_for_destructive {
            let destructive = ["write", "delete", "modify", "exec", "shell"];
            if destructive.iter().any(|d| tool.contains(d)) {
                return true;
            }
        }

        if self.require_approval_for_network {
            let network = ["web_search", "fetch", "curl", "wget", "http"];
            if network.iter().any(|n| tool.contains(n)) {
                return true;
            }
        }

        false
    }
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            require_approval_for_destructive: true,
            require_approval_for_network: true,
            require_approval_for_tools: Vec::new(),
            auto_approve_tools: vec!["read_file".to_string(), "list_files".to_string()],
            cost_threshold_usd: None,
        }
    }
}

/// Policy for tool usage
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolPolicy {
    /// Allowed tools (empty = all allowed)
    pub allowed_tools: Vec<String>,

    /// Blocked tools
    pub blocked_tools: Vec<String>,

    /// Maximum tool execution time (seconds)
    pub max_execution_time_secs: u64,

    /// Maximum tool output size (bytes)
    pub max_output_size_bytes: usize,
}

impl ToolPolicy {
    /// Check if a tool is allowed
    pub fn is_allowed(&self, tool: &str) -> bool {
        // If blocked, not allowed
        if self.blocked_tools.contains(&tool.to_string()) {
            return false;
        }

        // If allowed list is empty, all non-blocked are allowed
        if self.allowed_tools.is_empty() {
            return true;
        }

        // Otherwise must be in allowed list
        self.allowed_tools.contains(&tool.to_string())
    }
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            allowed_tools: Vec::new(),
            blocked_tools: vec!["rm".to_string(), "dd".to_string(), "mkfs".to_string()],
            max_execution_time_secs: 60,
            max_output_size_bytes: 1024 * 1024, // 1MB
        }
    }
}

/// Policy for worker management
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerPolicy {
    /// Whether workers can spawn sub-workers
    pub allow_nested_workers: bool,

    /// Maximum iterations per worker
    pub max_iterations: usize,

    /// Whether workers can use all tools or limited set
    pub workers_limited_tools: bool,

    /// Tools available to workers (if limited)
    pub worker_allowed_tools: Vec<String>,
}

impl Default for WorkerPolicy {
    fn default() -> Self {
        Self {
            allow_nested_workers: true,
            max_iterations: 30,
            workers_limited_tools: true,
            worker_allowed_tools: vec![
                "read_file".to_string(),
                "write_file".to_string(),
                "shell".to_string(),
                "search".to_string(),
            ],
        }
    }
}

/// Safety/content policy
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyPolicy {
    /// Enable content filtering
    pub enable_content_filter: bool,

    /// Blocked keywords/patterns
    pub blocked_patterns: Vec<String>,

    /// Log all tool calls for audit
    pub audit_log: bool,
}

impl Default for SafetyPolicy {
    fn default() -> Self {
        Self {
            enable_content_filter: false,
            blocked_patterns: Vec::new(),
            audit_log: true,
        }
    }
}

/// Worker limits and defaults
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerLimits {
    /// Default max iterations for new workers
    pub default_max_iterations: usize,

    /// Default whether workers can delegate
    pub default_can_delegate: bool,

    /// Maximum total workers per session
    pub max_total_workers: usize,
}

impl Default for WorkerLimits {
    fn default() -> Self {
        Self {
            default_max_iterations: 30,
            default_can_delegate: false,
            max_total_workers: 10,
        }
    }
}

/// Prompt configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptConfig {
    /// System prompt prefix
    pub system_prefix: String,

    /// Whether to include tool descriptions in prompts
    pub include_tool_descriptions: bool,

    /// Whether to include example tool calls
    pub include_examples: bool,

    /// Format for tool calls (XML, JSON, etc.)
    pub tool_format: ToolFormat,

    /// Maximum context length to include
    pub max_context_length: usize,
}

/// Format for tool calls in prompts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolFormat {
    /// XML-style <tool> tags
    Xml,
    /// JSON format
    Json,
    /// Function calling format (OpenAI style)
    Function,
    /// Custom format
    Custom(String),
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            system_prefix: "You are a helpful AI assistant.".to_string(),
            include_tool_descriptions: true,
            include_examples: true,
            tool_format: ToolFormat::Xml,
            max_context_length: 8000,
        }
    }
}

/// Feature flags
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureFlags {
    /// Enable PaCoRe reasoning
    pub enable_pacore: bool,

    /// Enable memory/recall
    pub enable_memory: bool,

    /// Enable web search
    pub enable_web_search: bool,

    /// Enable worker spawning
    pub enable_workers: bool,

    /// Enable streaming responses
    pub enable_streaming: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            enable_pacore: false,
            enable_memory: true,
            enable_web_search: true,
            enable_workers: true,
            enable_streaming: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approval_policy() {
        let policy = ApprovalPolicy::default();
        
        // Auto-approved tools
        assert!(!policy.requires_approval("read_file", ""));
        
        // Destructive tools require approval
        assert!(policy.requires_approval("write_file", ""));
        assert!(policy.requires_approval("shell", ""));
        
        // Network tools require approval
        assert!(policy.requires_approval("web_search", ""));
    }

    #[test]
    fn test_tool_policy() {
        let policy = ToolPolicy::default();
        
        // Blocked tools
        assert!(!policy.is_allowed("rm"));
        
        // Non-blocked tools allowed
        assert!(policy.is_allowed("read_file"));
    }

    #[test]
    fn test_kernel_config_builder() {
        let config = KernelConfig::new()
            .with_max_steps(100)
            .with_tool(ToolSchema {
                name: "test".to_string(),
                description: "Test tool".to_string(),
                parameters: serde_json::json!({}),
            });

        assert_eq!(config.max_steps, 100);
        assert_eq!(config.tool_schemas.len(), 1);
        assert!(config.has_tool("test"));
    }
}
