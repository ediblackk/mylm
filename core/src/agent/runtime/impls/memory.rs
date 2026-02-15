//! Memory Capability
//!
//! Provides long-term memory storage and retrieval with semantic search.
//!
//! This module provides two implementations:
//! - `MemoryCapability`: Simple in-memory implementation for testing
//! - `AgentMemoryManager`: Full-featured implementation with VectorStore backend

use crate::agent::runtime::{
    capability::{Capability, TelemetryCapability},
    context::RuntimeContext,
    impls::vector_store::{InMemoryVectorStore, SimpleEmbedder},
};
use crate::agent::cognition::{AgentDecision, InputEvent};
use crate::agent::memory::AgentMemoryManager;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

/// Memory entry (legacy format)
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
/// 
/// Note: For production use, prefer `AgentMemoryManager` from `crate::agent::memory`
/// which provides full VectorStore integration.
pub struct MemoryCapability {
    memories: Arc<RwLock<Vec<MemoryEntry>>>,
    vector_store: Arc<InMemoryVectorStore>,
    /// Optional reference to the full memory manager
    memory_manager: Option<Arc<AgentMemoryManager>>,
}

/// Vector store trait for semantic search (legacy)
#[async_trait::async_trait]
pub trait VectorStore: Send + Sync {
    async fn store(&self, id: &str, embedding: Vec<f32>, content: &str);
    async fn search(&self, query_embedding: Vec<f32>, top_k: usize) -> Vec<SearchResult>;
}

/// Search result from vector store
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub score: f32,
}

impl MemoryCapability {
    /// Create a new memory capability (in-memory only)
    pub fn new() -> Self {
        Self {
            memories: Arc::new(RwLock::new(Vec::new())),
            vector_store: Arc::new(InMemoryVectorStore::new()),
            memory_manager: None,
        }
    }
    
    /// Create with a full AgentMemoryManager backend
    pub fn with_manager(manager: Arc<AgentMemoryManager>) -> Self {
        Self {
            memories: Arc::new(RwLock::new(Vec::new())),
            vector_store: Arc::new(InMemoryVectorStore::new()),
            memory_manager: Some(manager),
        }
    }
    
    /// Store a memory with embedding
    pub async fn store(&self, content: impl Into<String>, category: MemoryCategory) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let content = content.into();
        
        // Also store in memory manager if available
        if let Some(ref manager) = self.memory_manager {
            let memory_type = match category {
                MemoryCategory::Fact => crate::memory::store::MemoryType::Discovery,
                MemoryCategory::UserPreference => crate::memory::store::MemoryType::UserNote,
                MemoryCategory::TaskResult => crate::memory::store::MemoryType::Decision,
                MemoryCategory::Conversation => crate::memory::store::MemoryType::UserNote,
                MemoryCategory::Code => crate::memory::store::MemoryType::Discovery,
            };
            let _ = manager.add_memory(&content, memory_type).await;
        }
        
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
        // If we have a memory manager, try to get from there first
        if let Some(ref manager) = self.memory_manager {
            if let Ok(memories) = manager.get_hot_memories(limit).await {
                if !memories.is_empty() {
                    return memories.into_iter().map(|m| MemoryEntry {
                        id: m.id.to_string(),
                        content: m.content,
                        category: MemoryCategory::Fact, // Default
                        timestamp: chrono::DateTime::from_timestamp(m.created_at, 0)
                            .unwrap_or_else(|| chrono::Utc::now()),
                        importance: 0.5,
                    }).collect();
                }
            }
        }
        
        // Fallback to local memories
        let memories = self.memories.read().await;
        memories.iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
    
    /// Format memories for inclusion in prompt
    pub async fn format_for_prompt(&self, limit: usize) -> String {
        // Try memory manager first
        if let Some(ref manager) = self.memory_manager {
            return manager.format_hot_memory_for_prompt(limit).await;
        }
        
        // Fallback to local
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
        // Try memory manager first
        if let Some(ref manager) = self.memory_manager {
            return manager.search_and_format(query, limit).await;
        }
        
        // Fallback to local
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
    
    /// Get the underlying memory manager if available
    pub fn memory_manager(&self) -> Option<&Arc<AgentMemoryManager>> {
        self.memory_manager.as_ref()
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
    
    async fn search(&self, _query_embedding: Vec<f32>, _top_k: usize) -> Vec<SearchResult> {
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
