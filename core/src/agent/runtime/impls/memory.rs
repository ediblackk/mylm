//! Memory Capability
//!
//! Provides long-term memory storage and retrieval with semantic search.

use crate::agent::runtime::{
    capability::{Capability, TelemetryCapability},
    context::RuntimeContext,
    impls::vector_store::{InMemoryVectorStore, SimpleEmbedder},
};
use crate::agent::cognition::{AgentDecision, InputEvent};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

/// Memory entry
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub category: MemoryCategory,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub importance: f32, // 0.0 to 1.0
}

#[derive(Debug, Clone)]
pub enum MemoryCategory {
    Fact,
    UserPreference,
    TaskResult,
    Conversation,
    Code,
}

/// Memory capability - stores and retrieves memories
pub struct MemoryCapability {
    memories: Arc<RwLock<Vec<MemoryEntry>>>,
    vector_store: Arc<InMemoryVectorStore>,
}

/// Vector store trait for semantic search
#[async_trait::async_trait]
pub trait VectorStore: Send + Sync {
    async fn store(&self, id: &str, embedding: Vec<f32>, content: &str);
    async fn search(&self, query_embedding: Vec<f32>, top_k: usize) -> Vec<String>;
}

impl MemoryCapability {
    pub fn new() -> Self {
        Self {
            memories: Arc::new(RwLock::new(Vec::new())),
            vector_store: Arc::new(InMemoryVectorStore::new()),
        }
    }
    
    /// Store a memory with embedding
    pub async fn store(&self, content: impl Into<String>, category: MemoryCategory) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let content = content.into();
        
        let entry = MemoryEntry {
            id: id.clone(),
            content: content.clone(),
            category: category.clone(),
            timestamp: chrono::Utc::now(),
            importance: 0.5,
        };
        
        // Store in vector store with embedding
        let embedding = SimpleEmbedder::embed(&content);
        let mut metadata = HashMap::new();
        metadata.insert("category".to_string(), format!("{:?}", category));
        
        self.vector_store.store(&id, embedding, &content, metadata).await;
        
        // Also store in memory list
        self.memories.write().await.push(entry);
        
        id
    }
    
    /// Retrieve memories by category
    pub async fn retrieve_by_category(&self, category: MemoryCategory, limit: usize) -> Vec<MemoryEntry> {
        let memories = self.memories.read().await;
        memories.iter()
            .filter(|m| std::mem::discriminant(&m.category) == std::mem::discriminant(&category))
            .take(limit)
            .cloned()
            .collect()
    }
    
    /// Search memories semantically
    pub async fn search_semantic(&self, query: &str, limit: usize) -> Vec<MemoryEntry> {
        let query_embedding = SimpleEmbedder::embed(query);
        let results = self.vector_store.search(&query_embedding, limit).await;
        
        // Map search results back to memory entries
        let memories = self.memories.read().await;
        results
            .into_iter()
            .filter_map(|result| {
                memories.iter().find(|m| m.id == result.id).cloned()
            })
            .collect()
    }
    
    /// Search memories by keyword
    pub async fn search(&self, query: &str, limit: usize) -> Vec<MemoryEntry> {
        let memories = self.memories.read().await;
        memories.iter()
            .filter(|m| m.content.to_lowercase().contains(&query.to_lowercase()))
            .take(limit)
            .cloned()
            .collect()
    }
    
    /// Get recent memories
    pub async fn recent(&self, limit: usize) -> Vec<MemoryEntry> {
        let memories = self.memories.read().await;
        memories.iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
    
    /// Format memories for inclusion in prompt
    pub async fn format_for_prompt(&self, limit: usize) -> String {
        let memories = self.recent(limit).await;
        if memories.is_empty() {
            return "No relevant memories.".to_string();
        }
        
        memories.iter()
            .map(|m| format!("- [{}] {}", format_category(&m.category), m.content))
            .collect::<Vec<_>>()
            .join("\n")
    }
    
    /// Format semantic search results for prompt
    pub async fn format_semantic_for_prompt(&self, query: &str, limit: usize) -> String {
        let memories = self.search_semantic(query, limit).await;
        if memories.is_empty() {
            return "No relevant memories found.".to_string();
        }
        
        memories.iter()
            .map(|m| format!("- [{}] {}", format_category(&m.category), m.content))
            .collect::<Vec<_>>()
            .join("\n")
    }
    
    /// Get memory count
    pub async fn count(&self) -> usize {
        self.memories.read().await.len()
    }
}

impl Default for MemoryCapability {
    fn default() -> Self {
        Self::new()
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

fn format_category(category: &MemoryCategory) -> &'static str {
    match category {
        MemoryCategory::Fact => "FACT",
        MemoryCategory::UserPreference => "PREF",
        MemoryCategory::TaskResult => "TASK",
        MemoryCategory::Conversation => "CONV",
        MemoryCategory::Code => "CODE",
    }
}

/// Stub vector store for testing
pub struct StubVectorStore;

#[async_trait::async_trait]
impl VectorStore for StubVectorStore {
    async fn store(&self, _id: &str, _embedding: Vec<f32>, _content: &str) {
        // No-op
    }
    
    async fn search(&self, _query_embedding: Vec<f32>, _top_k: usize) -> Vec<String> {
        vec![]
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

impl<E: crate::agent::CognitiveEngine> crate::agent::CognitiveEngine for MemoryAugmentedEngine<E> {
    fn step(
        &mut self,
        state: &crate::agent::cognition::state::AgentState,
        input: Option<InputEvent>,
    ) -> Result<crate::agent::cognition::decision::Transition, crate::agent::cognition::error::CognitiveError> {
        // Could augment state with relevant memories here
        self.inner.step(state, input)
    }
    
    fn build_prompt(&self, state: &crate::agent::cognition::state::AgentState) -> String {
        self.inner.build_prompt(state)
    }
    
    fn requires_approval(&self, tool: &str, args: &str) -> bool {
        self.inner.requires_approval(tool, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_memory_store_and_retrieve() {
        let memory = MemoryCapability::new();
        
        let id = memory.store("User likes Python", MemoryCategory::UserPreference).await;
        assert!(!id.is_empty());
        
        let prefs = memory.retrieve_by_category(MemoryCategory::UserPreference, 10).await;
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0].content, "User likes Python");
    }
    
    #[tokio::test]
    async fn test_memory_search() {
        let memory = MemoryCapability::new();
        
        memory.store("Python is great", MemoryCategory::Fact).await;
        memory.store("Rust is fast", MemoryCategory::Fact).await;
        memory.store("I love coding", MemoryCategory::UserPreference).await;
        
        let results = memory.search("Python", 10).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Python"));
    }
    
    #[tokio::test]
    async fn test_memory_semantic_search() {
        let memory = MemoryCapability::new();
        
        memory.store("The quick brown fox", MemoryCategory::Fact).await;
        memory.store("jumps over the lazy dog", MemoryCategory::Fact).await;
        memory.store("Hello world", MemoryCategory::Fact).await;
        
        // Search should find relevant memories
        let results = memory.search_semantic("quick fox", 2).await;
        assert!(!results.is_empty());
    }
}
