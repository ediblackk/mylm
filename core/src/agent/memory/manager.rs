//! Agent Memory Manager
//!
//! Unified memory interface for the agent system.
//! Handles hot memory (recent activity), cold memory (vector search),
//! and user profile (personalized context).

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, warn};

use crate::memory::store::{VectorStore, Memory, MemoryType};
use crate::memory::journal::{Journal, InteractionType};
use crate::config::agent::{MemoryConfig, UserProfile};

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
    
    journal: Option<Arc<tokio::sync::Mutex<Journal>>>,
    config: MemoryConfig,
    mode: MemoryMode,
    /// User profile for personalized context
    profile: std::sync::Mutex<UserProfile>,
}

impl AgentMemoryManager {
    /// Create a new memory manager using config
    /// 
    /// Respects config.enabled, config.storage_path, and config.incognito
    pub async fn new(config: MemoryConfig) -> Result<Self> {
        // If memory is disabled or incognito, don't create persistent store
        if !config.enabled {
            info!("Memory is disabled in config, creating no-op manager");
            return Self::disabled().await;
        }
        
        if config.incognito {
            info!("Memory is in incognito mode, no persistence");
            return Self::incognito().await;
        }
        
        let storage_path = config.effective_storage_path();
        std::fs::create_dir_all(&storage_path)?;
        
        let path = storage_path.to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid storage path"))?;
        
        Self::with_path(path, config).await
    }
    
    /// Create a disabled memory manager (no-op)
    /// 
    /// Uses a temporary in-memory store. This is async to avoid blocking issues.
    async fn disabled() -> Result<Self> {
        // Create a temporary in-memory store that won't persist
        let temp_dir = std::env::temp_dir().join(format!("mylm_memory_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir)?;
        
        // Create the store asynchronously (no block_on needed)
        let store = VectorStore::new(temp_dir.to_str().unwrap()).await?;
        
        Ok(Self {
            vector_store: Arc::new(store),
            journal: None,
            config: MemoryConfig {
                enabled: false,
                ..Default::default()
            },
            mode: MemoryMode::default(),
            profile: std::sync::Mutex::new(UserProfile::default()),
        })
    }
    
    /// Create an incognito memory manager (in-memory only)
    async fn incognito() -> Result<Self> {
        Self::disabled().await // Same as disabled - no persistence
    }
    
    /// Create a new memory manager with custom storage path
    pub async fn with_path(path: &str, config: MemoryConfig) -> Result<Self> {
        if !config.enabled {
            return Self::disabled().await;
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
        
        // Load user profile
        let profile = UserProfile::load().unwrap_or_default();
        
        Ok(Self {
            vector_store,
            journal,
            config,
            mode: MemoryMode::default(),
            profile: std::sync::Mutex::new(profile),
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
        let profile = UserProfile::load().unwrap_or_default();
        
        Self {
            vector_store: store,
            journal: None, // Journal not available when using from_store
            config: MemoryConfig::default(),
            mode: MemoryMode::default(),
            profile: std::sync::Mutex::new(profile),
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
    /// 
    /// Content is sanitized in VectorStore::add_memory_typed_with_id before storage.
    pub async fn add_memory(
        &self,
        content: &str,
        memory_type: MemoryType,
    ) -> Result<i64> {
        self.add_memory_full(content, memory_type, None, None, None, None).await
    }
    
    /// Add a memory with all fields (for import/export)
    pub async fn add_memory_full(
        &self,
        content: &str,
        memory_type: MemoryType,
        session_id: Option<String>,
        metadata: Option<serde_json::Value>,
        category_id: Option<String>,
        summary: Option<String>,
    ) -> Result<i64> {
        if !self.config.enabled {
            debug!("Memory is disabled, skipping add_memory");
            return Ok(0);
        }
        
        debug!("Adding memory: type={:?}, content_len={}", memory_type, content.len());
        
        let id = chrono::Utc::now().timestamp_nanos_opt()
            .unwrap_or_else(|| chrono::Utc::now().timestamp());
        
        // Note: sanitization happens in VectorStore::add_memory_typed_with_id
        self.vector_store.add_memory_typed_with_id(
            id,
            content,
            memory_type,
            session_id,
            metadata,
            category_id,
            summary,
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
                        
                        // Sanitize journal entry content before creating Memory
                        let sanitized_content = sanitize_memory_content(&entry.content);
                        
                        Memory {
                            id: i as i64,
                            content: sanitized_content,
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
    /// Content is already sanitized when stored in VectorStore or retrieved from journal.
    pub fn format_memories_for_prompt(memories: &[Memory]) -> String {
        if memories.is_empty() {
            return "No relevant memories.".to_string();
        }
        
        let mut context = String::from("## Relevant Past Context\n\n");
        
        for (i, mem) in memories.iter().enumerate() {
            let timestamp = chrono::DateTime::from_timestamp(mem.created_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            
            // Content is already sanitized when stored
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
        self.get_recent_memories_with_offset(limit, 0).await
    }
    
    /// Get recent memories with offset pagination support
    /// 
    /// # Arguments
    /// * `limit` - Maximum number of memories to return
    /// * `offset` - Number of memories to skip (for pagination)
    pub async fn get_recent_memories_with_offset(&self, limit: usize, offset: usize) -> Result<Vec<Memory>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }
        
        self.vector_store.get_recent_memories_with_offset(limit, offset).await
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
    
    // ===== User Profile Methods =====
    
    /// Get user profile
    pub fn get_profile(&self) -> UserProfile {
        self.profile.lock().map(|p| p.clone()).unwrap_or_default()
    }
    
    /// Update user profile
    pub fn update_profile(&self, f: impl FnOnce(&mut UserProfile)) -> Result<()> {
        let mut profile = self.profile.lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock profile: {}", e))?;
        f(&mut profile);
        profile.save()?;
        Ok(())
    }
    
    /// Format user profile for prompt injection
    pub fn format_profile_for_prompt(&self) -> String {
        let profile = self.get_profile();
        if profile.is_empty() {
            String::new()
        } else {
            profile.format_for_prompt()
        }
    }
    
    /// Set a user preference
    pub fn set_preference(&self, key: impl Into<String>, value: impl Into<String>) -> Result<()> {
        self.update_profile(|p| p.set_preference(key, value))
    }
    
    /// Set a user fact
    pub fn set_fact(&self, key: impl Into<String>, value: impl Into<String>) -> Result<()> {
        self.update_profile(|p| p.set_fact(key, value))
    }
    
    /// Add a user pattern
    pub fn add_pattern(&self, pattern: impl Into<String>) -> Result<()> {
        self.update_profile(|p| p.add_pattern(pattern))
    }
    
    /// Add an active goal
    pub fn add_goal(&self, goal: impl Into<String>) -> Result<()> {
        self.update_profile(|p| p.add_goal(goal))
    }
    
    /// Complete/remove a goal
    pub fn complete_goal(&self, goal: &str) -> Result<()> {
        self.update_profile(|p| p.complete_goal(goal))
    }
    
    /// Extract profile updates from conversation text
    /// 
    /// This is a simple heuristic-based extraction.
    /// For production, consider using LLM-based extraction.
    pub fn extract_profile_from_text(&self, text: &str) -> Result<()> {
        let text_lower = text.to_lowercase();
        
        // Detect preferences
        if text_lower.contains("i prefer") {
            if let Some(start) = text_lower.find("i prefer") {
                let rest = &text[start + 8..];
                if let Some(end) = rest.find(|c: char| c == '.' || c == '!' || c == '?') {
                    let pref = rest[..end].trim();
                    if !pref.is_empty() {
                        self.set_fact(format!("preference_{}", pref.replace(' ', "_")), pref)?;
                    }
                }
            }
        }
        
        // Detect birthday
        if text_lower.contains("my birthday") || text_lower.contains("birthday is") {
            // Simple pattern: look for month names and dates
            let months = ["january", "february", "march", "april", "may", "june",
                         "july", "august", "september", "october", "november", "december"];
            for (i, month) in months.iter().enumerate() {
                if text_lower.contains(month) {
                    // Try to extract day
                    if let Some(pos) = text_lower.find(month) {
                        let after = &text[pos + month.len()..pos + month.len() + 10];
                        if let Some(day) = after.trim().split_whitespace().next() {
                            if let Ok(d) = day.parse::<u32>() {
                                self.set_fact("birthday", format!("{:02}-{:02}", i + 1, d))?;
                                break;
                            }
                        }
                    }
                }
            }
        }
        
        // Detect work patterns
        if text_lower.contains("i usually work") || text_lower.contains("i often work") {
            self.add_pattern("has_work_schedule")?;
        }
        if text_lower.contains("evening") && text_lower.contains("work") {
            self.add_pattern("evening_worker")?;
        }
        if text_lower.contains("morning") && text_lower.contains("work") {
            self.add_pattern("morning_person")?;
        }
        
        // Detect tools/preferences
        if text_lower.contains("i use vim") || text_lower.contains("i prefer vim") {
            self.set_preference("editor", "vim")?;
        }
        if text_lower.contains("i use vscode") || text_lower.contains("i use vs code") {
            self.set_preference("editor", "vscode")?;
        }
        if text_lower.contains("dark mode") || text_lower.contains("dark theme") {
            self.set_preference("theme", "dark")?;
        }
        if text_lower.contains("light mode") || text_lower.contains("light theme") {
            self.set_preference("theme", "light")?;
        }
        
        Ok(())
    }
    
    /// Extract profile using LLM for better accuracy
    /// 
    /// This is more accurate than heuristic extraction but requires an LLM call.
    /// Recommended for batch processing or important conversations.
    /// 
    /// # Arguments
    /// * `text` - The conversation text to analyze
    /// * `llm_client` - The LLM client to use for extraction
    /// 
    /// # Example
    /// ```rust
    /// let extracted = manager.extract_profile_with_llm(
    ///     "I prefer dark mode and use vim. My birthday is April 5.",
    ///     &llm_client
    /// ).await?;
    /// ```
    pub async fn extract_profile_with_llm(
        &self,
        text: &str,
        llm_client: &crate::provider::LlmClient,
    ) -> Result<crate::config::agent::UserProfile> {
        use crate::provider::chat::{ChatMessage, ChatRequest};
        
        let prompt = format!(
            "Analyze the following text and extract user profile information. \
            Return ONLY a JSON object with this structure:\n\
            {{\n\
              \"preferences\": {{\"key\": \"value\"}},\n\
              \"facts\": {{\"key\": \"value\"}},\n\
              \"patterns\": [\"pattern1\", \"pattern2\"],\n\
              \"goals\": [\"goal1\", \"goal2\"]\n\
            }}\n\n\
            If no profile information is found, return empty objects/arrays.\n\n\
            Text to analyze:\n{}\n\n\
            JSON:",
            text
        );
        
        let request = ChatRequest::new(
            llm_client.model().to_string(),
            vec![
                ChatMessage::system("You are a profile extraction assistant. Extract user preferences, facts, patterns, and goals from text."),
                ChatMessage::user(&prompt),
            ],
        );
        
        let response = llm_client.chat(&request).await?;
        let content = response.content();
        
        // Extract JSON from response
        let json_str = if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                &content[start..=end]
            } else {
                content.as_str()
            }
        } else {
            content.as_str()
        };
        
        // Parse the extracted profile
        let extracted: crate::config::agent::UserProfile = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse LLM response as profile: {}", e))?;
        
        // Merge with existing profile
        self.update_profile(|profile| {
            // Merge preferences
            for (k, v) in extracted.preferences {
                profile.set_preference(k, v);
            }
            // Merge facts
            for (k, v) in extracted.facts {
                profile.set_fact(k, v);
            }
            // Merge patterns
            for p in extracted.patterns {
                profile.add_pattern(p);
            }
            // Merge goals
            for g in extracted.active_goals {
                profile.add_goal(g);
            }
        })?;
        
        Ok(self.get_profile())
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
// ===== MemoryProvider Implementation =====

use crate::agent::memory::MemoryProvider;
use std::sync::Weak;

/// A MemoryProvider implementation that wraps AgentMemoryManager
/// 
/// This allows the LLM engine to proactively inject memory context into prompts.
pub struct AgentMemoryProvider {
    manager: Weak<AgentMemoryManager>,
}

impl AgentMemoryProvider {
    /// Create a new memory provider from an AgentMemoryManager
    pub fn new(manager: Arc<AgentMemoryManager>) -> Self {
        Self {
            manager: Arc::downgrade(&manager),
        }
    }
}

#[async_trait::async_trait]
impl MemoryProvider for AgentMemoryProvider {
    async fn get_context(&self, user_message: &str) -> String {
        crate::info_log!("[MEMORY_PROVIDER] get_context called");
        
        // Try to get the manager
        let Some(manager) = self.manager.upgrade() else {
            crate::warn_log!("[MEMORY_PROVIDER] Memory manager dropped, cannot get context");
            return String::new();
        };
        
        // Check if memory is enabled
        if !manager.is_enabled() {
            crate::warn_log!("[MEMORY_PROVIDER] Memory manager is disabled");
            return String::new();
        }
        
        crate::info_log!("[MEMORY_PROVIDER] Getting context for message: '{}'", 
            &user_message[..user_message.len().min(50)]);
        
        // Async implementation - no block_in_place needed
        crate::info_log!("[MEMORY_PROVIDER] Fetching hot memories and semantic search...");
        
        // 1. Hot memory: recent activity from this session
        let hot_memories = manager.get_hot_memories_configured().await.unwrap_or_default();
        crate::info_log!("[MEMORY_PROVIDER] Got {} hot memories", hot_memories.len());
        
        // 2. Semantic memory: relevant memories based on user query
        let semantic_limit = manager.config().semantic_search_limit;
        let semantic_memories = manager.search_memories(user_message, semantic_limit).await.unwrap_or_default();
        crate::info_log!("[MEMORY_PROVIDER] Got {} semantic matches", semantic_memories.len());
        
        crate::debug_log!("[MEMORY_PROVIDER] Hot memories: {}, Semantic matches: {}", 
            hot_memories.len(), semantic_memories.len());
        
        // Combine and deduplicate (by memory ID)
        let mut combined: std::collections::HashMap<i64, Memory> = std::collections::HashMap::new();
        
        // Add hot memories first (they're more recent/relevant to current session)
        for mem in hot_memories {
            combined.insert(mem.id, mem);
        }
        
        // Add semantic memories (may overlap with hot)
        for mem in semantic_memories {
            combined.insert(mem.id, mem);
        }
        
        let memories: Vec<Memory> = combined.into_values().collect();
        
        // 3. User profile for personalized context
        let profile_context = manager.format_profile_for_prompt();
        if !profile_context.is_empty() {
            crate::info_log!("[MEMORY_PROVIDER] Injecting user profile ({} chars)", profile_context.len());
        }
        
        if memories.is_empty() && profile_context.is_empty() {
            crate::warn_log!("[MEMORY_PROVIDER] No memories or profile found to inject - VectorStore may be empty");
            return String::new();
        }
        
        crate::info_log!("[MEMORY_PROVIDER] Injecting {} unique memories into context", memories.len());
        
        // Combine: Profile first, then memories
        let mut full_context = String::new();
        if !profile_context.is_empty() {
            full_context.push_str(&profile_context);
            full_context.push('\n');
        }
        if !memories.is_empty() {
            full_context.push_str(&AgentMemoryManager::format_memories_for_prompt(&memories));
        }
        
        crate::info_log!("[MEMORY_PROVIDER] Formatted context ({} chars): {}", 
            full_context.len(), &full_context[..full_context.len().min(200)]);
        full_context
    }
    
    fn remember(&self, content: &str) {
        let Some(manager) = self.manager.upgrade() else {
            crate::warn_log!("[MEMORY_PROVIDER] Memory manager dropped, cannot remember");
            return;
        };
        
        if !manager.is_enabled() {
            return;
        }
        
        // Auto-extract profile from content using heuristics
        let content_str = content.to_string();
        if let Err(e) = manager.extract_profile_from_text(&content_str) {
            crate::debug_log!("[MEMORY_PROVIDER] Profile extraction failed: {}", e);
        }
        
        // Fire-and-forget: spawn a task to save the memory
        let content = content.to_string();
        tokio::spawn(async move {
            use crate::memory::store::MemoryType;
            if let Err(e) = manager.add_memory(&content, MemoryType::UserNote).await {
                crate::warn_log!("[MEMORY_PROVIDER] Failed to save memory: {}", e);
            } else {
                crate::info_log!("[MEMORY_PROVIDER] Saved memory: '{}'", 
                    &content[..content.len().min(50)]);
            }
        });
    }
    
    async fn build_context(
        &self, 
        history: &[crate::conversation::manager::Message], 
        _scratchpad: &str, 
        _system_prompt: &str
    ) -> String {
        // Get manager for profile extraction
        let Some(manager) = self.manager.upgrade() else {
            return String::new();
        };
        
        // 1. Extract profile from recent user messages
        for msg in history.iter().filter(|m| m.role == "user").rev().take(3) {
            if let Err(e) = manager.extract_profile_from_text(&msg.content) {
                crate::debug_log!("[MEMORY_PROVIDER] Profile extraction from history failed: {}", e);
            }
        }
        
        // 2. Derive query from recent user messages in history
        let query = history
            .iter()
            .filter(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        
        // Use existing get_context with the derived query
        // Truncate to avoid overly long queries
        let truncated_query = if query.len() > 500 {
            &query[..500]
        } else {
            &query
        };
        
        self.get_context(truncated_query).await
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_memory_manager_disabled() {
        let _config = MemoryConfig {
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

// Re-export from store for backward compatibility
pub use crate::memory::store::sanitize_memory_content;
