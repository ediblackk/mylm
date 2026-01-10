use anyhow::Result;
use async_trait::async_trait;

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

    /// Execute the tool with the provided arguments
    async fn call(&self, args: &str) -> Result<String>;
}
