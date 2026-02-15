//! Agent Memory Manager
//!
//! Unified memory interface for the agent system.
//! Handles hot memory (recent activity) and cold memory (vector search).

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, warn};

use crate::memory::store::{VectorStore, Memory, MemoryType};
use crate::memory::journal::{Journal, InteractionType};
use crate::config::agent::MemoryConfig;

/// Memory operation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMode {
    /// Legacy mode - strict schema matching, no auto-migration
    Legacy,
    /// Adaptive mode - auto-migrate on schema mismatch
    Adaptive,
}

impl Default for MemoryMode {
    fn default() -> Self {
        MemoryMode::Adaptive
    }
}

/// Unified memory manager for agent
pub struct AgentMemoryManager {
    vector_store: Arc<VectorStore>,
    #[allow(dead_code)]
    journal: Option<Arc<tokio::sync::Mutex<Journal>>>,
    config: MemoryConfig,
    mode: MemoryMode,
}

impl AgentMemoryManager {
    /// Create a new memory manager using config
    /// 
    /// Respects config.enabled, config.storage_path, and config.incognito
    pub async fn new(config: MemoryConfig) -> Result<Self> {
        // If memory is disabled or incognito, don't create persistent store
        if !config.enabled {
            info!("Memory is disabled in config, creating no-op manager");
            return Self::disabled();
        }
        
        if config.incognito {
            info!("Memory is in incognito mode, no persistence");
            return Self::incognito();
        }
        
        let storage_path = config.effective_storage_path();
        std::fs::create_dir_all(&storage_path)?;
        
        let path = storage_path.to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid storage path"))?;
        
        Self::with_path(path, config).await
    }
    
    /// Create a disabled memory manager (no-op)
    fn disabled() -> Result<Self> {
        // Create a temporary in-memory store that won't persist
        let temp_dir = std::env::temp_dir().join(format!("mylm_memory_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir)?;
        
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| anyhow::anyhow!("No Tokio runtime"))?;
        
        // This is a workaround - in disabled mode we create a store but won't use it
        let store = rt.block_on(async {
            VectorStore::new(temp_dir.to_str().unwrap()).await
        })?;
        
        Ok(Self {
            vector_store: Arc::new(store),
            journal: None,
            config: MemoryConfig {
                enabled: false,
                ..Default::default()
            },
            mode: MemoryMode::default(),
        })
    }
    
    /// Create an incognito memory manager (in-memory only)
    fn incognito() -> Result<Self> {
        Self::disabled() // Same as disabled - no persistence
    }
    
    /// Create a new memory manager with custom storage path
    pub async fn with_path(path: &str, config: MemoryConfig) -> Result<Self> {
        if !config.enabled {
            return Self::disabled();
        }
        
        info!("Initializing AgentMemoryManager at: {}", path);
        
        let vector_store = Arc::new(VectorStore::new(path).await?);
        
        // Journal is optional - can be None if not needed
        let journal = if config.enabled && !config.incognito {
            match Journal::new() {
                Ok(j) => Some(Arc::new(tokio::sync::Mutex::new(j))),
                Err(e) => {
                    warn!("Failed to initialize journal: {}", e);
                    None
                }
            }
        } else {
            None
        };
        
        Ok(Self {
            vector_store,
            journal,
            config,
            mode: MemoryMode::default(),
        })
    }
    
    /// Create with specific mode (legacy or adaptive)
    pub async fn with_mode(path: &str, config: MemoryConfig, mode: MemoryMode) -> Result<Self> {
        let mut manager = Self::with_path(path, config).await?;
        manager.mode = mode;
        info!("Memory manager initialized in {:?} mode", mode);
        Ok(manager)
    }
    
    /// Create from an existing VectorStore
    /// 
    /// This is useful when you want to share a VectorStore across multiple components
    /// or when the VectorStore was initialized elsewhere.
    pub fn from_store(store: Arc<VectorStore>) -> Self {
        Self {
            vector_store: store,
            journal: None, // Journal not available when using from_store
            config: MemoryConfig::default(),
            mode: MemoryMode::default(),
        }
    }
    
    /// Get a reference to the underlying VectorStore
    pub fn vector_store(&self) -> &Arc<VectorStore> {
        &self.vector_store
    }
    
    /// Check if memory is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
    
    /// Get the current memory mode
    pub fn mode(&self) -> MemoryMode {
        self.mode
    }
    
    /// Add a new memory entry
    pub async fn add_memory(
        &self,
        content: &str,
        memory_type: MemoryType,
    ) -> Result<i64> {
        if !self.config.enabled {
            debug!("Memory is disabled, skipping add_memory");
            return Ok(0);
        }
        
        debug!("Adding memory: type={:?}, content_len={}", memory_type, content.len());
        
        let id = chrono::Utc::now().timestamp_nanos_opt()
            .unwrap_or_else(|| chrono::Utc::now().timestamp());
        
        self.vector_store.add_memory_typed_with_id(
            id,
            content,
            memory_type,
            None, // session_id
            None, // metadata
            None, // category_id
            None, // summary
        ).await?;
        
        info!("Memory added with id: {}", id);
        Ok(id)
    }
    
    /// Add a user note memory
    pub async fn add_user_note(&self, content: &str) -> Result<i64> {
        self.add_memory(content, MemoryType::UserNote).await
    }
    
    /// Add a decision memory
    pub async fn add_decision(&self, content: &str) -> Result<i64> {
        self.add_memory(content, MemoryType::Decision).await
    }
    
    /// Add a discovery memory
    pub async fn add_discovery(&self, content: &str) -> Result<i64> {
        self.add_memory(content, MemoryType::Discovery).await
    }
    
    /// Search memories by semantic similarity
    pub async fn search_memories(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        if !self.config.enabled {
            debug!("Memory is disabled, returning empty search results");
            return Ok(Vec::new());
        }
        
        debug!("Searching memories: query='{}', limit={}", query, limit);
        
        let effective_limit = limit.min(self.config.max_memories);
        let results = self.vector_store.search_memory(query, effective_limit).await?;
        
        info!("Memory search returned {} results", results.len());
        Ok(results)
    }
    
    /// Search memories by type
    pub async fn search_by_type(
        &self,
        query: &str,
        memory_type: MemoryType,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }
        
        let results = self.vector_store.search_by_type(query, memory_type, limit).await?;
        Ok(results)
    }
    
    /// Get recent memories (hot memory) from journal or vector store
    pub async fn get_hot_memories(&self, limit: usize) -> Result<Vec<Memory>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }
        
        // Try to get from journal first (faster, more recent)
        if let Some(ref journal) = self.journal {
            let journal_guard = journal.lock().await;
            let entries = journal_guard.entries();
            
            if !entries.is_empty() {
                let start = entries.len().saturating_sub(limit);
                let recent_entries: Vec<Memory> = entries[start..]
                    .iter()
                    .enumerate()
                    .map(|(i, entry)| {
                        let memory_type = match entry.entry_type {
                            InteractionType::Thought => MemoryType::Decision,
                            InteractionType::Tool => MemoryType::Command,
                            InteractionType::Output => MemoryType::Discovery,
                            InteractionType::Chat => MemoryType::UserNote,
                        };
                        
                        Memory {
                            id: i as i64,
                            content: entry.content.clone(),
                            summary: None,
                            created_at: chrono::Utc::now().timestamp(),
                            r#type: memory_type,
                            session_id: None,
                            metadata: None,
                            category_id: None,
                            embedding: None,
                        }
                    })
                    .collect();
                
                return Ok(recent_entries);
            }
        }
        
        // Fallback: get most recent from vector store
        // Use get_recent_memories to get the newest memories by created_at
        let results = self.vector_store.get_recent_memories(limit).await?;
        Ok(results)
    }
    
    /// Format memories for inclusion in prompt context
    pub fn format_memories_for_prompt(memories: &[Memory]) -> String {
        if memories.is_empty() {
            return "No relevant memories.".to_string();
        }
        
        let mut context = String::from("## Relevant Past Context\n\n");
        
        for (i, mem) in memories.iter().enumerate() {
            let timestamp = chrono::DateTime::from_timestamp(mem.created_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            
            context.push_str(&format!(
                "{}. [{} | {}] {}\n",
                i + 1,
                mem.r#type,
                timestamp,
                mem.content.lines().next().unwrap_or(&mem.content)
            ));
        }
        
        context.push_str("\n");
        context
    }
    
    /// Format hot memories for prompt
    pub async fn format_hot_memory_for_prompt(&self, limit: usize) -> String {
        match self.get_hot_memories(limit).await {
            Ok(memories) => Self::format_memories_for_prompt(&memories),
            Err(e) => {
                warn!("Failed to get hot memories: {}", e);
                "## Recent Activity\n(No recent activity recorded)\n\n".to_string()
            }
        }
    }
    
    /// Search and format for prompt
    pub async fn search_and_format(&self, query: &str, limit: usize) -> String {
        match self.search_memories(query, limit).await {
            Ok(memories) => Self::format_memories_for_prompt(&memories),
            Err(e) => {
                warn!("Failed to search memories: {}", e);
                String::new()
            }
        }
    }
    
    /// Get memory statistics
    pub async fn stats(&self) -> Result<MemoryStats> {
        let count = if self.config.enabled {
            self.vector_store.count_memories().await.unwrap_or(0)
        } else {
            0
        };
        
        Ok(MemoryStats {
            total_memories: count,
            recent_memories: self.config.context_window,
            mode: self.mode,
            enabled: self.config.enabled,
        })
    }
    
    /// Get recent memories ordered by created_at (newest first)
    pub async fn get_recent_memories(&self, limit: usize) -> Result<Vec<Memory>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }
        
        self.vector_store.get_recent_memories(limit).await
    }
    
    /// Get a single memory by ID
    pub async fn get_memory_by_id(&self, id: i64) -> Result<Option<Memory>> {
        if !self.config.enabled {
            return Ok(None);
        }
        
        self.vector_store.get_memory_by_id(id).await
    }
    
    /// Delete a memory by ID
    pub async fn delete_memory(&self, id: i64) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }
        
        self.vector_store.delete_memory(id).await
    }
    
    /// Update memory content
    pub async fn update_memory(&self, id: i64, content: &str) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }
        
        self.vector_store.update_memory(id, content).await
    }
    
    /// Repair database (adaptive mode only)
    pub async fn repair(&self) -> Result<String> {
        if self.mode == MemoryMode::Legacy {
            return Ok("Repair not available in Legacy mode".to_string());
        }
        
        self.vector_store.repair_database().await
    }
    
    // ===== Config-aware convenience methods =====
    
    /// Get hot memories using configured context_window
    /// 
    /// This respects the config.context_window setting
    pub async fn get_hot_memories_configured(&self) -> Result<Vec<Memory>> {
        self.get_hot_memories(self.config.context_window).await
    }
    
    /// Format hot memories for prompt using configured context_window
    pub async fn format_hot_memory_for_prompt_configured(&self) -> String {
        self.format_hot_memory_for_prompt(self.config.context_window).await
    }
    
    /// Get the configured context window size
    pub fn context_window(&self) -> usize {
        self.config.context_window
    }
    
    /// Get the configured max memories limit
    pub fn max_memories(&self) -> usize {
        self.config.max_memories
    }
    
    /// Check if semantic search is enabled
    pub fn is_semantic_search_enabled(&self) -> bool {
        self.config.semantic_search
    }
    
    /// Check if this is incognito mode
    pub fn is_incognito(&self) -> bool {
        self.config.incognito
    }
    
    /// Get the memory configuration
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }
}

/// Memory statistics
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub total_memories: usize,
    pub recent_memories: usize,
    pub mode: MemoryMode,
    pub enabled: bool,
}


// ===== MemoryProvider Implementation (TEMPORARILY DISABLED) =====
// TODO: restore with new architecture
/*
use crate::agent::MemoryProvider;
use std::sync::Weak;

pub struct AgentMemoryProvider {
    manager: Weak<AgentMemoryManager>,
}

impl AgentMemoryProvider {
    pub fn new(manager: Arc<AgentMemoryManager>) -> Self {
        Self {
            manager: Arc::downgrade(&manager),
        }
    }
}

impl MemoryProvider for AgentMemoryProvider {
    fn get_context(&self, user_message: &str) -> String {
        // ... implementation ...
        String::new()
    }
    
    fn remember(&self, _content: &str) {
        // ... implementation ...
    }
}
*/


#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_memory_manager_disabled() {
        let config = MemoryConfig {
            enabled: false,
            ..Default::default()
        };
        
        // This will fail if no data dir, but we test the enabled check
        // In practice, you'd use a temp dir for testing
    }
    
    #[test]
    fn test_format_memories_empty() {
        let memories: Vec<Memory> = vec![];
        let result = AgentMemoryManager::format_memories_for_prompt(&memories);
        assert_eq!(result, "No relevant memories.");
    }
}
