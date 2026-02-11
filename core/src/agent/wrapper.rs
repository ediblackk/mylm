//! Agent wrapper for UI compatibility between V1 and V2
//!
//! This module provides `AgentWrapper`, an enum that can hold either a V1 `Agent`
//! or a V2 `AgentV2`, allowing the TUI to work with either without dual instantiation.

use crate::llm::chat::ChatMessage;
use crate::agent::tools::StructuredScratchpad;
use crate::agent::v2::jobs::JobRegistry;
use crate::memory::scribe::Scribe;
use crate::config::AgentVersion;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Wrapper enum to hold either V1 or V2 agent
/// 
/// This eliminates the need for dual instantiation in the TUI.
/// The TUI creates ONE agent (V1 or V2) and uses this wrapper to
/// access common fields regardless of the underlying type.
/// 
/// Holds Arc<Mutex<>> internally so the same agent can be shared
/// between the UI (AppState) and the orchestrator.
#[derive(Clone)]
pub enum AgentWrapper {
    V1(Arc<Mutex<crate::agent::Agent>>),
    V2(Arc<Mutex<crate::agent::v2::AgentV2>>),
}

impl AgentWrapper {
    /// Create a new V1 wrapper
    pub fn new_v1(agent: crate::agent::Agent) -> Self {
        Self::V1(Arc::new(Mutex::new(agent)))
    }
    
    /// Create a new V2 wrapper
    pub fn new_v2(agent: crate::agent::v2::AgentV2) -> Self {
        Self::V2(Arc::new(Mutex::new(agent)))
    }
    
    /// Get the agent version
    pub async fn version(&self) -> AgentVersion {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.version
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.version
            }
        }
    }
    
    /// Check if this is a V2 agent
    pub fn is_v2(&self) -> bool {
        matches!(self, AgentWrapper::V2(_))
    }
    
    /// Get the inner V1 Arc (if this is V1)
    pub fn as_v1_arc(&self) -> Option<Arc<Mutex<crate::agent::Agent>>> {
        match self {
            AgentWrapper::V1(a) => Some(a.clone()),
            _ => None,
        }
    }
    
    /// Get the inner V2 Arc (if this is V2)
    pub fn as_v2_arc(&self) -> Option<Arc<Mutex<crate::agent::v2::AgentV2>>> {
        match self {
            AgentWrapper::V2(a) => Some(a.clone()),
            _ => None,
        }
    }
    
    // ============================================================================
    // Common field accessors (async due to Mutex)
    // ============================================================================
    
    /// Get the conversation history
    pub async fn history(&self) -> Vec<ChatMessage> {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.history.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.history.clone()
            }
        }
    }
    
    /// Set the conversation history (for session restore)
    pub async fn set_history(&self, history: Vec<ChatMessage>) {
        match self {
            AgentWrapper::V1(a) => {
                let mut guard = a.lock().await;
                guard.history = history;
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.history = history;
            }
        }
    }
    
    /// Get the scratchpad for structured data
    pub async fn scratchpad(&self) -> Option<Arc<RwLock<StructuredScratchpad>>> {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.scratchpad.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                Some(guard.scratchpad.clone())
            }
        }
    }
    
    /// Set the scratchpad
    pub async fn set_scratchpad(&self, scratchpad: Arc<RwLock<StructuredScratchpad>>) {
        match self {
            AgentWrapper::V1(a) => {
                let mut guard = a.lock().await;
                guard.scratchpad = Some(scratchpad);
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.scratchpad = scratchpad;
            }
        }
    }
    
    /// Get the session ID
    pub async fn session_id(&self) -> String {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.session_id.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.session_id.clone()
            }
        }
    }
    
    /// Set the session ID (for session restore)
    pub async fn set_session_id(&self, session_id: String) {
        match self {
            AgentWrapper::V1(a) => {
                let mut guard = a.lock().await;
                guard.session_id = session_id;
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.session_id = session_id;
            }
        }
    }
    
    /// Reset the iteration counter (for chat session mode)
    /// This allows the agent to continue chatting without hitting iteration limits
    pub async fn reset_iteration_counter(&self) {
        match self {
            AgentWrapper::V1(_) => {
                // V1 doesn't have this counter
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.reset_iteration_counter();
            }
        }
    }
    
    /// Set iteration limit dynamically (for chat session mode)
    pub async fn set_iteration_limit(&self, limit: usize) {
        match self {
            AgentWrapper::V1(_) => {
                // V1 doesn't have this counter
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.set_iteration_limit(limit);
            }
        }
    }
    
    /// Get the scribe for memory operations
    pub async fn scribe(&self) -> Option<Arc<Scribe>> {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.scribe.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                Some(guard.scribe.clone())
            }
        }
    }
    
    /// Set the scribe
    pub async fn set_scribe(&self, scribe: Arc<Scribe>) {
        match self {
            AgentWrapper::V1(a) => {
                let mut guard = a.lock().await;
                guard.scribe = Some(scribe);
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.scribe = scribe;
            }
        }
    }
    
    /// Get the job registry
    pub async fn job_registry(&self) -> JobRegistry {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.job_registry.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.job_registry.clone()
            }
        }
    }
    
    /// Get the memory store
    pub async fn memory_store(&self) -> Option<Arc<crate::memory::VectorStore>> {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.memory_store.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.memory_store.clone()
            }
        }
    }
    
    /// Get the categorizer
    pub async fn categorizer(&self) -> Option<Arc<crate::memory::MemoryCategorizer>> {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.categorizer.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.categorizer.clone()
            }
        }
    }
    
    /// Get permissions
    pub async fn permissions(&self) -> Option<crate::config::v2::types::AgentPermissions> {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.permissions.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.permissions.clone()
            }
        }
    }
    
    /// Get the LLM client
    pub async fn llm_client(&self) -> Arc<crate::llm::LlmClient> {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.llm_client.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.llm_client.clone()
            }
        }
    }
    
    /// Set the LLM client
    pub async fn set_llm_client(&self, client: Arc<crate::llm::LlmClient>) {
        match self {
            AgentWrapper::V1(a) => {
                let mut guard = a.lock().await;
                guard.llm_client = client;
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.llm_client = client;
            }
        }
    }
    
    /// Get the system prompt prefix
    pub async fn system_prompt_prefix(&self) -> String {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.system_prompt_prefix.clone()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.system_prompt_prefix.clone()
            }
        }
    }
    
    /// Set the system prompt prefix
    pub async fn set_system_prompt_prefix(&self, prefix: String) {
        match self {
            AgentWrapper::V1(a) => {
                let mut guard = a.lock().await;
                guard.system_prompt_prefix = prefix;
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.system_prompt_prefix = prefix;
            }
        }
    }
    
    /// Check if memory is disabled
    pub async fn disable_memory(&self) -> bool {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.disable_memory
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.disable_memory
            }
        }
    }
    
    /// Set memory disabled flag
    pub async fn set_disable_memory(&self, disabled: bool) {
        match self {
            AgentWrapper::V1(a) => {
                let mut guard = a.lock().await;
                guard.disable_memory = disabled;
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.disable_memory = disabled;
            }
        }
    }
    
    /// Get max iterations
    pub async fn max_iterations(&self) -> usize {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.max_iterations
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.max_iterations
            }
        }
    }
    
    /// Get tool registry (V1 only, returns None for V2)
    pub async fn tool_registry(&self) -> Option<crate::agent::ToolRegistry> {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                Some(guard.tool_registry.clone())
            }
            AgentWrapper::V2(_) => None,
        }
    }
    
    /// Get tools HashMap (V2 only, returns None for V1)
    pub async fn tools(&self) -> Option<std::collections::HashMap<String, Arc<dyn crate::agent::Tool>>> {
        match self {
            AgentWrapper::V1(_) => None,
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                Some(guard.tools.clone())
            }
        }
    }
    
    /// Condense history - delegates to appropriate implementation
    pub async fn condense_history(&self, history: &[ChatMessage]) -> anyhow::Result<Vec<ChatMessage>> {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.condense_history(history).await
                    .map_err(|e| anyhow::anyhow!("Condensation failed: {}", e))
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.condense_history(history).await
                    .map_err(|e| anyhow::anyhow!("Condensation failed: {}", e))
            }
        }
    }
    
    /// Get tools description (for debugging)
    pub async fn get_tools_description(&self) -> String {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.get_tools_description().await
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.get_tools_description()
            }
        }
    }
    
    /// Get system prompt (for debugging)
    pub async fn get_system_prompt(&self) -> String {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.get_system_prompt().await
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.get_system_prompt().await
            }
        }
    }
    
    /// Execute a single agent step (for direct chat execution)
    /// Only works with V2 agents
    pub async fn step_v2(&self, observation: Option<String>) -> anyhow::Result<crate::agent::v2::protocol::AgentDecision> {
        match self {
            AgentWrapper::V1(_) => {
                Err(anyhow::anyhow!("step_v2() not available for V1 agent"))
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.step(observation).await
                    .map_err(|e| anyhow::anyhow!("Agent step failed: {}", e))
            }
        }
    }
    
    /// Get tools for direct execution (V2 only)
    pub async fn get_tools(&self) -> Option<std::collections::HashMap<String, Arc<dyn crate::agent::Tool>>> {
        match self {
            AgentWrapper::V1(_) => None,
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                Some(guard.tools.clone())
            }
        }
    }
    
    /// Reset the agent with new history (for starting fresh conversations)
    pub async fn reset(&self, history: Vec<ChatMessage>) {
        match self {
            AgentWrapper::V1(a) => {
                let mut guard = a.lock().await;
                guard.reset(history).await;
            }
            AgentWrapper::V2(a) => {
                let mut guard = a.lock().await;
                guard.reset(history).await;
            }
        }
    }
    
    /// Check if the agent has a pending decision
    pub async fn has_pending_decision(&self) -> bool {
        match self {
            AgentWrapper::V1(a) => {
                let guard = a.lock().await;
                guard.has_pending_decision()
            }
            AgentWrapper::V2(a) => {
                let guard = a.lock().await;
                guard.has_pending_decision()
            }
        }
    }
}

// Implement Send + Sync since both inner types are Send + Sync
unsafe impl Send for AgentWrapper {}
unsafe impl Sync for AgentWrapper {}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_wrapper_enum_size() {
        let size = std::mem::size_of::<AgentWrapper>();
        assert!(size > 0);
    }
}
