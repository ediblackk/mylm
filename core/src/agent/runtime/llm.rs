//! LLM runtime

use crate::agent::cognition::input::InputEvent;
use crate::agent::runtime::RuntimeError;
use crate::agent::types::common::TokenUsage;

/// LLM client interface
#[async_trait::async_trait]
pub trait LlmRuntime: Send + Sync {
    /// Send prompt, get response
    async fn complete(
        &self,
        prompt: &str,
        system: Option<&str>,
    ) -> Result<(String, TokenUsage), RuntimeError>;
}

/// Stub implementation
pub struct StubLlmRuntime;

#[async_trait::async_trait]
impl LlmRuntime for StubLlmRuntime {
    async fn complete(
        &self,
        _prompt: &str,
        _system: Option<&str>,
    ) -> Result<(String, TokenUsage), RuntimeError> {
        Ok(("stub response".to_string(), TokenUsage::default()))
    }
}
