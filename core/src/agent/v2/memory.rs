//! Memory Integration
//!
//! Handles hot memory injection, memory context building, and auto-categorization.

use std::error::Error as StdError;
use std::sync::Arc;
use crate::llm::chat::{ChatMessage, MessageRole};
use crate::memory::{MemoryCategorizer, scribe::Scribe};
use crate::memory::store::VectorStore;

/// Manager for memory integration into agent context.
pub struct MemoryManager {
    scribe: Arc<Scribe>,
    memory_store: Option<Arc<VectorStore>>,
    categorizer: Option<Arc<MemoryCategorizer>>,
    disable_memory: bool,
}

impl MemoryManager {
    /// Create a new memory manager.
    pub fn new(
        scribe: Arc<Scribe>,
        memory_store: Option<Arc<VectorStore>>,
        categorizer: Option<Arc<MemoryCategorizer>>,
        disable_memory: bool,
    ) -> Self {
        Self {
            scribe,
            memory_store,
            categorizer,
            disable_memory,
        }
    }

    /// Check if memory is enabled.
    pub fn is_enabled(&self) -> bool {
        !self.disable_memory
    }

    /// Inject hot memory (recent journal entries) into the conversation context.
    ///
    /// This should be called after `reset()` to add recent activity awareness.
    /// Call this before starting the agent loop to ensure the model knows about
    /// recent interactions and can reference them proactively.
    pub async fn inject_hot_memory(&self, history: &mut Vec<ChatMessage>, limit: usize) {
        // Skip memory injection in incognito mode
        if self.disable_memory {
            return;
        }

        // Inject hot memory (recent journal entries) into context
        // This ensures the model is aware of recent activity and can use memory proactively
        if let Ok(hot_memory_context) = self.get_hot_memory_context(limit).await {
            if !hot_memory_context.is_empty() {
                // Insert hot memory after system prompt but before user messages
                let hot_memory_msg = ChatMessage::system(format!(
                    "## Recent Activity (Hot Memory)\n{}\n\nUse this context and proactively search memory for relevant information when needed.",
                    hot_memory_context
                ));
                if history.len() > 1 {
                    history.insert(1, hot_memory_msg);
                } else {
                    history.push(hot_memory_msg);
                }
            }
        }
    }

    /// Get recent journal entries (hot memory) as context.
    ///
    /// Fetches the last N entries from the journal to provide
    /// immediate context about recent activity.
    pub async fn get_hot_memory_context(&self, limit: usize) -> Result<String, Box<dyn StdError + Send + Sync>> {
        let journal = self.scribe.journal();
        let journal_guard = journal.lock().await;
        let entries = journal_guard.entries();

        if entries.is_empty() {
            return Ok(String::new());
        }

        // Get the last N entries
        let start = entries.len().saturating_sub(limit);
        let recent_entries = &entries[start..];

        let mut context = String::new();
        for entry in recent_entries {
            context.push_str(&format!("- [{}] {}: {}\n",
                entry.timestamp,
                entry.entry_type,
                entry.content.lines().next().unwrap_or(&entry.content) // First line only
            ));
        }

        Ok(context)
    }

    /// Inject relevant memories into the conversation history based on the last user message.
    pub async fn inject_memory_context(
        &self,
        history: &mut Vec<ChatMessage>,
        auto_context: bool,
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if !auto_context || self.disable_memory {
            return Ok(());
        }

        if let Some(store) = &self.memory_store {
            // Find the last user message to use as a search query
            if let Some(last_user_msg) = history.iter().rev().find(|m| m.role == MessageRole::User) {
                let memories = store.search_memory(&last_user_msg.content, 5).await.unwrap_or_default();
                if !memories.is_empty() {
                    let context = self.build_context_from_memories(&memories);
                    // Append context to the last user message
                    if let Some(user_idx) = history.iter().rposition(|m| m.role == MessageRole::User) {
                        history[user_idx].content.push_str("\n\n");
                        history[user_idx].content.push_str(&context);
                    }
                }
            }
        }
        Ok(())
    }

    /// Build context string from memories.
    pub fn build_context_from_memories(&self, memories: &[crate::memory::store::Memory]) -> String {
        if memories.is_empty() {
            return String::new();
        }

        let mut context = String::from("## Relevant Past Operations & Knowledge\n");
        for (i, mem) in memories.iter().enumerate() {
            let timestamp = chrono::DateTime::from_timestamp(mem.created_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "unknown time".to_string());

            context.push_str(&format!(
                "{}. [{}] {} ({})\n",
                i + 1,
                mem.r#type,
                mem.content,
                timestamp,
            ));
        }
        context.push_str("\nUse this context to inform your actions and avoid repeating mistakes.");
        context
    }

    /// Automatically categorize a newly added memory.
    pub async fn auto_categorize(&self, memory_id: i64, content: &str) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if let (Some(categorizer), Some(store)) = (&self.categorizer, &self.memory_store) {
            let category_id = categorizer.categorize_memory(content).await?;
            store.update_memory_category(memory_id, category_id.clone()).await?;
            // Update summary for the category
            let _ = categorizer.update_category_summary(&category_id).await;
        }
        Ok(())
    }

    /// Get a reference to the scribe.
    pub fn scribe(&self) -> &Arc<Scribe> {
        &self.scribe
    }

    /// Get a reference to the memory store.
    pub fn memory_store(&self) -> Option<&Arc<VectorStore>> {
        self.memory_store.as_ref()
    }
}
