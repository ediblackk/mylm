//! Tool execution runtime

use crate::agent::runtime::RuntimeError;
use crate::agent::types::common::ToolResult;

/// Tool executor - async side effects
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute tool with args
    async fn execute(&self, tool: &str, args: &str) -> Result<ToolResult, RuntimeError>;
    
    /// Check if tool is available
    fn has_tool(&self, tool: &str) -> bool;
}

/// Stub implementation
pub struct StubToolExecutor;

#[async_trait::async_trait]
impl ToolExecutor for StubToolExecutor {
    async fn execute(&self, _tool: &str, _args: &str) -> Result<ToolResult, RuntimeError> {
        Ok(ToolResult::Success("stub".to_string()))
    }
    
    fn has_tool(&self, _tool: &str) -> bool {
        true
    }
}
