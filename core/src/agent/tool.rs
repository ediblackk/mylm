use anyhow::Result;
use async_trait::async_trait;

/// Categorizes tools based on how they should be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// Execute silently/internally (e.g., Memory, WebSearch).
    Internal,
    /// Execute visibly in Terminal (e.g., Shell).
    Terminal,
    /// Web-based tools (search, crawl)
    Web,
}

/// A trait for tools that can be executed by the agent.
///
/// Tools are the primary way the agent interacts with the world.
/// Each tool must implement this trait and be `Send + Sync` to be used in the agentic loop.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The name of the tool (e.g., "execute_command")
    fn name(&self) -> &str;

    /// A brief description of what the tool does
    fn description(&self) -> &str;

    /// A description of how to use the tool, including parameter format
    fn usage(&self) -> &str;

    /// Optional JSON schema for tool parameters
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "args": {
                    "type": "string",
                    "description": self.usage()
                }
            },
            "required": ["args"]
        })
    }

    /// Execute the tool with the provided arguments
    async fn call(&self, args: &str) -> Result<String>;

    /// The kind of tool (Internal or Terminal)
    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}
