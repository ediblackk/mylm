//! Memory Capability
//!
//! Provides long-term memory storage and retrieval.
//! This is a thin wrapper around `AgentMemoryManager`.

use crate::agent::runtime::core::{
    Capability, TelemetryCapability, RuntimeContext,
};
use crate::agent::cognition::{AgentDecision, InputEvent};
use crate::agent::memory::AgentMemoryManager;
use std::sync::Arc;

/// Memory entry category
#[derive(Debug, Clone)]
pub enum MemoryCategory {
    Fact,
    UserPreference,
    TaskResult,
    Conversation,
    Code,
}

/// Memory capability - stores and retrieves memories
/// 
/// This is a wrapper around `AgentMemoryManager` that implements
/// the `Capability` and `TelemetryCapability` traits.
pub struct MemoryCapability {
    manager: Arc<AgentMemoryManager>,
}

impl MemoryCapability {
    /// Create with a full AgentMemoryManager backend
    pub fn with_manager(manager: Arc<AgentMemoryManager>) -> Self {
        Self { manager }
    }
    
    /// Store a memory
    pub async fn store(&self, content: impl Into<String>, category: MemoryCategory) -> String {
        let content = content.into();
        
        let memory_type = match category {
            MemoryCategory::Fact => crate::memory::store::MemoryType::Discovery,
            MemoryCategory::UserPreference => crate::memory::store::MemoryType::UserNote,
            MemoryCategory::TaskResult => crate::memory::store::MemoryType::Decision,
            MemoryCategory::Conversation => crate::memory::store::MemoryType::UserNote,
            MemoryCategory::Code => crate::memory::store::MemoryType::Discovery,
        };
        
        match self.manager.add_memory(&content, memory_type).await {
            Ok(id) => id.to_string(),
            Err(_) => String::new(),
        }
    }
    
    /// Search memories by keyword
    pub async fn search(&self, query: &str, limit: usize) -> Vec<String> {
        match self.manager.search_memories(query, limit).await {
            Ok(memories) => memories.into_iter().map(|m| m.content).collect(),
            Err(_) => Vec::new(),
        }
    }
    
    /// Format memories for inclusion in prompt
    pub async fn format_for_prompt(&self, limit: usize) -> String {
        self.manager.format_hot_memory_for_prompt(limit).await
    }
    
    /// Get memory count
    pub async fn count(&self) -> usize {
        // Note: This is a workaround since AgentMemoryManager doesn't expose count directly
        match self.manager.get_hot_memories(10000).await {
            Ok(memories) => memories.len(),
            Err(_) => 0,
        }
    }
    
    /// Get reference to the underlying memory manager
    pub fn manager(&self) -> &Arc<AgentMemoryManager> {
        &self.manager
    }
}

impl Capability for MemoryCapability {
    fn name(&self) -> &'static str {
        "memory"
    }
}

#[async_trait::async_trait]
impl TelemetryCapability for MemoryCapability {
    async fn record_decision(&self, _ctx: &RuntimeContext, _decision: &AgentDecision) {
        // Could extract memories from decisions
    }
    
    async fn record_result(&self, _ctx: &RuntimeContext, event: &InputEvent) {
        // Extract memories from events
        if let InputEvent::UserMessage(msg) = event {
            // Simple extraction - in production, use LLM to extract facts
            if msg.contains("my name is") || msg.contains("I am") || msg.contains("I like") {
                self.store(msg.clone(), MemoryCategory::UserPreference).await;
            }
        }
    }
}

/// Memory-augmented engine wrapper
pub struct MemoryAugmentedEngine<E> {
    inner: E,
    #[allow(dead_code)]
    memory: Arc<MemoryCapability>,
}

impl<E> MemoryAugmentedEngine<E> {
    pub fn new(inner: E, memory: Arc<MemoryCapability>) -> Self {
        Self { inner, memory }
    }
}

impl<E: crate::agent::StepEngine> crate::agent::StepEngine for MemoryAugmentedEngine<E> {
    fn step(
        &mut self,
        state: &crate::agent::cognition::kernel::AgentState,
        input: Option<InputEvent>,
    ) -> Result<crate::agent::cognition::decision::Transition, crate::agent::cognition::error::CognitiveError> {
        self.inner.step(state, input)
    }
    
    fn build_prompt(&self, state: &crate::agent::cognition::kernel::AgentState) -> String {
        self.inner.build_prompt(state)
    }
    
    fn requires_approval(&self, tool: &str, args: &str) -> bool {
        self.inner.requires_approval(tool, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    // Note: These tests require a properly initialized AgentMemoryManager
    // which needs async runtime and temp directories. For unit tests,
    // we would typically mock the AgentMemoryManager.
    
    #[tokio::test]
    async fn test_memory_capability_with_manager() {
        // Create a temporary memory manager
        use crate::config::agent::{AgentConfig, MemoryConfig};
        
        let config = AgentConfig {
            memory: MemoryConfig {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        };
        
        // In disabled mode, we can create a manager without persistence
        let manager = AgentMemoryManager::new(config.memory)
            .await
            .expect("Failed to create memory manager");
        
        let memory = MemoryCapability::with_manager(Arc::new(manager));
        
        // Should not panic
        let count = memory.count().await;
        assert_eq!(count, 0);
    }
}
