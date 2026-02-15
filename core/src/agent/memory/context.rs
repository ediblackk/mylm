//! Memory Context Injection
//!
//! Helpers for injecting memory context into LLM prompts.

use crate::agent::memory::manager::AgentMemoryManager;
use crate::memory::store::Memory;

/// Context injection strategies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionStrategy {
    /// Inject at the start of the conversation (after system prompt)
    AtStart,
    /// Inject before the last user message
    BeforeLastUser,
    /// Inject as a separate system message
    AsSystemMessage,
}

/// Builder for memory context injection
pub struct MemoryContextBuilder<'a> {
    manager: &'a AgentMemoryManager,
    strategy: InjectionStrategy,
    hot_memory_limit: usize,
    semantic_search_limit: usize,
    query: Option<String>,
}

impl<'a> MemoryContextBuilder<'a> {
    /// Create a new context builder
    pub fn new(manager: &'a AgentMemoryManager) -> Self {
        Self {
            manager,
            strategy: InjectionStrategy::AtStart,
            hot_memory_limit: 5,
            semantic_search_limit: 3,
            query: None,
        }
    }
    
    /// Set injection strategy
    pub fn with_strategy(mut self, strategy: InjectionStrategy) -> Self {
        self.strategy = strategy;
        self
    }
    
    /// Set hot memory limit
    pub fn with_hot_limit(mut self, limit: usize) -> Self {
        self.hot_memory_limit = limit;
        self
    }
    
    /// Set semantic search limit
    pub fn with_search_limit(mut self, limit: usize) -> Self {
        self.semantic_search_limit = limit;
        self
    }
    
    /// Set query for semantic search
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }
    
    /// Build the context string
    pub async fn build(self) -> String {
        if !self.manager.is_enabled() {
            return String::new();
        }
        
        let mut context = String::new();
        
        // Add hot memory (recent activity)
        match self.manager.get_hot_memories(self.hot_memory_limit).await {
            Ok(memories) if !memories.is_empty() => {
                context.push_str("## Recent Activity\n\n");
                for mem in &memories {
                    context.push_str(&format_context_line(mem));
                }
                context.push('\n');
            }
            _ => {}
        }
        
        // Add semantic search results if query provided
        if let Some(query) = self.query {
            match self.manager.search_memories(&query, self.semantic_search_limit).await {
                Ok(memories) if !memories.is_empty() => {
                    context.push_str("## Relevant Memories\n\n");
                    for mem in &memories {
                        context.push_str(&format_context_line(mem));
                    }
                    context.push('\n');
                }
                _ => {}
            }
        }
        
        context
    }
}

/// Format a single memory for context
fn format_context_line(mem: &Memory) -> String {
    let timestamp = chrono::DateTime::from_timestamp(mem.created_at, 0)
        .map(|dt| dt.format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "???".to_string());
    
    // Truncate content to first line or 100 chars
    let content = mem.content.lines().next().unwrap_or(&mem.content);
    let truncated = if content.len() > 100 {
        format!("{}...", &content[..100])
    } else {
        content.to_string()
    };
    
    format!("- [{} | {}] {}\n", timestamp, mem.r#type, truncated)
}

/// Inject memory context into a chat history (represented as list of strings for flexibility)
pub async fn inject_memory_context(
    manager: &AgentMemoryManager,
    messages: &mut Vec<(String, String)>, // (role, content) pairs
    query: Option<&str>,
) {
    if !manager.is_enabled() {
        return;
    }
    
    let context = MemoryContextBuilder::new(manager)
        .with_hot_limit(5)
        .with_search_limit(3)
        .with_query(query.unwrap_or_default())
        .build()
        .await;
    
    if context.is_empty() {
        return;
    }
    
    // Find position after system message (if exists)
    let insert_pos = messages
        .iter()
        .position(|(role, _)| role != "system")
        .unwrap_or(0);
    
    messages.insert(insert_pos, ("system".to_string(), context));
}

/// Quick helper to get context for a single query
pub async fn get_context_for_query(
    manager: &AgentMemoryManager,
    query: &str,
) -> String {
    MemoryContextBuilder::new(manager)
        .with_query(query)
        .with_hot_limit(3)
        .with_search_limit(5)
        .build()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_format_context_line() {
        let mem = Memory {
            id: 1,
            content: "Test memory content".to_string(),
            summary: None,
            created_at: 1700000000,
            r#type: crate::memory::store::MemoryType::UserNote,
            session_id: None,
            metadata: None,
            category_id: None,
            embedding: None,
        };
        
        let line = format_context_line(&mem);
        assert!(line.contains("UserNote"));
        assert!(line.contains("Test memory content"));
    }
}
