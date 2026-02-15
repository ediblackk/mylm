//! Session Persistence
//!
//! Handles automatic, debounced, atomic session persistence for TUI sessions.
//!
//! # Features
//!
//! - **Autosave**: All TUI sessions are automatically saved by default
//! - **Debounced**: 500ms debounce to avoid excessive I/O
//! - **Atomic**: Write to temp file + rename for atomicity
//! - **Resume**: Load latest or specific session by ID
//!
//! # Storage Location
//!
//! Sessions are stored in:
//! - `$XDG_DATA_DIR/mylm/sessions/` or
//! - `$HOME/.local/share/mylm/sessions/`
//!
//! # File Format
//!
//! - `session_{id}.json` - Individual session files
//! - `latest.json` - Symlink to most recent session (for quick resume)

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use tracing::{info, warn, error};

use crate::agent::cognition::history::Message;
use crate::agent::types::events::TokenUsage;

/// Session data for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    /// Unique session ID
    pub id: String,
    /// Session creation timestamp
    pub timestamp: DateTime<Utc>,
    /// Session update timestamp
    pub updated_at: DateTime<Utc>,
    /// Chat/conversation history
    pub history: Vec<Message>,
    /// Session metadata (stats, preview, etc.)
    pub metadata: SessionMetadata,
    /// Agent state checkpoint (if available)
    #[serde(default)]
    pub agent_state: Option<AgentStateCheckpoint>,
    /// Incognito mode flag (sessions saved in incognito won't persist across restarts)
    #[serde(default)]
    pub incognito: bool,
}

/// Session metadata for display and statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Preview of last assistant message
    pub last_message_preview: String,
    /// Number of messages in session
    pub message_count: usize,
    /// Total tokens used
    pub total_tokens: u32,
    /// Input tokens used
    #[serde(default)]
    pub input_tokens: u32,
    /// Output tokens used
    #[serde(default)]
    pub output_tokens: u32,
    /// Estimated cost
    #[serde(default)]
    pub cost: f64,
    /// Session duration in seconds
    #[serde(default)]
    pub elapsed_seconds: u64,
    /// Session title (if named by user)
    #[serde(default)]
    pub title: Option<String>,
}

impl Default for SessionMetadata {
    fn default() -> Self {
        Self {
            last_message_preview: String::new(),
            message_count: 0,
            total_tokens: 0,
            input_tokens: 0,
            output_tokens: 0,
            cost: 0.0,
            elapsed_seconds: 0,
            title: None,
        }
    }
}

/// Agent state checkpoint for resuming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStateCheckpoint {
    /// Serialized agent state
    pub state_json: String,
    /// Step count at checkpoint
    pub step_count: usize,
    /// Session configuration
    pub config: crate::agent::SessionConfig,
}

impl Default for PersistedSession {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: now,
            updated_at: now,
            history: Vec::new(),
            metadata: SessionMetadata::default(),
            agent_state: None,
            incognito: false,
        }
    }
}

/// Session persistence manager
///
/// Handles automatic, debounced, atomic session persistence.
/// Spawns a background task that receives sessions via a channel and
/// writes them to disk with debouncing to avoid excessive I/O.
pub struct SessionPersistence {
    current_session: Arc<RwLock<Option<PersistedSession>>>,
    save_sender: mpsc::Sender<PersistedSession>,
    _save_task: tokio::task::JoinHandle<()>,
    autosave_enabled: bool,
}

impl SessionPersistence {
    /// Create a new SessionPersistence manager with default settings
    ///
    /// Autosave is enabled by default.
    pub fn new() -> Self {
        Self::with_autosave(true)
    }
    
    /// Create from memory config
    ///
    /// Respects config.autosave and config.incognito
    pub fn from_config(config: &crate::config::agent::MemoryConfig) -> Self {
        if config.incognito {
            // In incognito mode, disable autosave and use temp storage
            Self::without_autosave()
        } else {
            Self::with_autosave(config.autosave)
        }
    }
    
    /// Create with specific autosave setting
    pub fn with_autosave(_autosave: bool) -> Self {
        let (save_sender, mut save_receiver) = mpsc::channel::<PersistedSession>(1);
        let sessions_dir = Self::resolve_sessions_dir();
        
        // Create directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&sessions_dir) {
            error!("Failed to create sessions directory: {}", e);
        }
        
        let current_session = Arc::new(RwLock::new(None));
        let current_session_clone = current_session.clone();
        let sessions_dir_clone = sessions_dir.clone();
        
        // Spawn background save task
        let save_task = tokio::spawn(async move {
            let mut pending_session: Option<PersistedSession> = None;
            let mut debounce_interval = interval(Duration::from_millis(500));
            
            loop {
                tokio::select! {
                    // Receive new session from channel
                    received_session = save_receiver.recv() => {
                        match received_session {
                            Some(session) => {
                                pending_session = Some(session);
                            }
                            None => {
                                info!("[SessionPersistence] Save channel closed, exiting background task");
                                break;
                            }
                        }
                    }
                    
                    // Debounce timer tick
                    _ = debounce_interval.tick() => {
                        if let Some(session) = pending_session.take() {
                            // Skip saving incognito sessions
                            if session.incognito {
                                continue;
                            }
                            
                            // Save session to disk atomically
                            if let Err(e) = Self::save_session_atomic(&sessions_dir_clone, &session).await {
                                error!("[SessionPersistence] Failed to save session {}: {}", session.id, e);
                            } else {
                                // Update latest.json atomically
                                if let Err(e) = Self::update_latest_atomic(&sessions_dir_clone, &session).await {
                                    error!("[SessionPersistence] Failed to update latest.json: {}", e);
                                } else {
                                    info!("[SessionPersistence] Saved session {}", session.id);
                                }
                            }
                        }
                    }
                }
            }
        });
        
        Self {
            current_session: current_session_clone,
            save_sender,
            _save_task: save_task,
            autosave_enabled: true,
        }
    }
    
    /// Create with autosave disabled
    /// 
    /// This creates a minimal persistence manager that doesn't spawn a background task.
    pub fn without_autosave() -> Self {
        let (save_sender, _save_receiver) = mpsc::channel::<PersistedSession>(1);
        
        // Create a dummy task that does nothing
        let save_task = tokio::spawn(async move {
            // No-op - channel is dropped immediately
        });
        
        Self {
            current_session: Arc::new(RwLock::new(None)),
            save_sender,
            _save_task: save_task,
            autosave_enabled: false,
        }
    }
    
    /// Check if autosave is enabled
    pub fn is_autosave_enabled(&self) -> bool {
        self.autosave_enabled
    }
    
    /// Enable/disable autosave
    pub fn set_autosave(&mut self, enabled: bool) {
        self.autosave_enabled = enabled;
    }
    
    /// Save a session asynchronously (fire-and-forget)
    ///
    /// Non-blocking - the session is cloned and sent to the background task
    pub async fn save(&self, session: &PersistedSession) {
        if !self.autosave_enabled || session.incognito {
            return;
        }
        
        let session_clone = session.clone();
        if let Err(e) = self.save_sender.send(session_clone).await {
            error!("[SessionPersistence] Failed to send session to background task: {}", e);
        }
    }
    
    /// Set the current session and trigger immediate save
    ///
    /// Stores in self.current_session and triggers an immediate save via the channel
    pub fn set_current_session(&self, session: PersistedSession) {
        // Update current session
        if let Ok(mut guard) = self.current_session.try_write() {
            *guard = Some(session.clone());
        } else {
            warn!("[SessionPersistence] Failed to acquire write lock for current_session");
        }
        
        // Trigger immediate save (spawn a task to send async)
        if self.autosave_enabled && !session.incognito {
            let sender = self.save_sender.clone();
            tokio::spawn(async move {
                if let Err(e) = sender.send(session).await {
                    error!("[SessionPersistence] Failed to send session in set_current_session: {}", e);
                }
            });
        }
    }
    
    /// Get the current session (cloned)
    pub async fn current_session(&self) -> Option<PersistedSession> {
        let guard = self.current_session.read().await;
        guard.clone()
    }
    
    /// Load the latest session from latest.json
    pub async fn load_latest() -> Option<PersistedSession> {
        let sessions_dir = Self::resolve_sessions_dir();
        let latest_path = sessions_dir.join("latest.json");
        
        if !latest_path.exists() {
            return None;
        }
        
        match tokio::fs::read_to_string(&latest_path).await {
            Ok(content) => {
                match serde_json::from_str(&content) {
                    Ok(session) => Some(session),
                    Err(e) => {
                        error!("[SessionPersistence] Failed to deserialize latest.json: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                error!("[SessionPersistence] Failed to read latest.json: {}", e);
                None
            }
        }
    }
    
    /// Load all sessions from the sessions directory
    ///
    /// Returns sessions sorted by timestamp (most recent first)
    pub fn load_all() -> Vec<PersistedSession> {
        let sessions_dir = Self::resolve_sessions_dir();
        let mut sessions: Vec<PersistedSession> = Vec::new();
        
        let entries = match std::fs::read_dir(&sessions_dir) {
            Ok(entries) => entries,
            Err(e) => {
                error!("[SessionPersistence] Failed to read sessions directory: {}", e);
                return Vec::new();
            }
        };
        
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                // Skip latest.json and temp files
                if file_name == "latest.json" || file_name.ends_with(".tmp") {
                    continue;
                }
                
                // Match session_*.json pattern
                if file_name.starts_with("session_") && file_name.ends_with(".json") {
                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            match serde_json::from_str(&content) {
                                Ok(session) => sessions.push(session),
                                Err(e) => {
                                    error!("[SessionPersistence] Failed to deserialize {}: {}", file_name, e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("[SessionPersistence] Failed to read {}: {}", file_name, e);
                        }
                    }
                }
            }
        }
        
        // Sort by timestamp descending (most recent first)
        sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        sessions
    }
    
    /// Load a specific session by ID
    pub async fn load(id: &str) -> Option<PersistedSession> {
        // Try loading latest if id is "latest" or empty
        if id == "latest" || id.is_empty() {
            return Self::load_latest().await;
        }
        
        let sessions_dir = Self::resolve_sessions_dir();
        
        // Try direct file first
        let direct_path = sessions_dir.join(format!("session_{}.json", id));
        if direct_path.exists() {
            match tokio::fs::read_to_string(&direct_path).await {
                Ok(content) => {
                    return serde_json::from_str(&content).ok();
                }
                Err(e) => {
                    error!("[SessionPersistence] Failed to read session {}: {}", id, e);
                }
            }
        }
        
        // Search through all sessions for partial match
        let all_sessions = Self::load_all();
        for session in all_sessions {
            if session.id == id || 
               session.id.ends_with(&format!("_{}", id)) ||
               session.id.contains(id) {
                return Some(session);
            }
        }
        
        None
    }
    
    /// Delete a session by ID
    pub async fn delete(id: &str) -> Result<(), std::io::Error> {
        let sessions_dir = Self::resolve_sessions_dir();
        let session_path = sessions_dir.join(format!("session_{}.json", id));
        let temp_path = sessions_dir.join(format!("session_{}.tmp", id));
        
        // Delete main session file if it exists
        if session_path.exists() {
            tokio::fs::remove_file(&session_path).await?;
        }
        
        // Delete temp file if it exists
        if temp_path.exists() {
            tokio::fs::remove_file(&temp_path).await?;
        }
        
        info!("[SessionPersistence] Deleted session {}", id);
        Ok(())
    }
    
    /// Resolve the sessions directory path
    ///
    /// Priority: $XDG_DATA_DIR/mylm/sessions/ or $HOME/.local/share/mylm/sessions/
    fn resolve_sessions_dir() -> PathBuf {
        // Try XDG_DATA_DIR first via dirs crate
        if let Some(mut data_dir) = dirs::data_dir() {
            data_dir.push("mylm/sessions");
            return data_dir;
        }
        
        // Fallback to HOME/.local/share/mylm/sessions
        if let Some(home) = dirs::home_dir() {
            let mut fallback = home;
            fallback.push(".local/share/mylm/sessions");
            return fallback;
        }
        
        // Last resort: current directory (shouldn't happen in practice)
        PathBuf::from("./sessions")
    }
    
    /// Save session to disk atomically: write to temp file, then rename
    async fn save_session_atomic(dir: &PathBuf, session: &PersistedSession) -> Result<(), std::io::Error> {
        let temp_path = dir.join(format!("session_{}.tmp", session.id));
        let final_path = dir.join(format!("session_{}.json", session.id));
        
        // Serialize session to JSON
        let json = serde_json::to_string_pretty(session)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        
        // Write to temp file
        tokio::fs::write(&temp_path, json).await?;
        
        // Atomic rename
        tokio::fs::rename(&temp_path, &final_path).await?;
        
        Ok(())
    }
    
    /// Update latest.json atomically: write to temp file, then rename
    async fn update_latest_atomic(dir: &PathBuf, session: &PersistedSession) -> Result<(), std::io::Error> {
        let temp_path = dir.join("latest.tmp");
        let final_path = dir.join("latest.json");
        
        // Serialize session to JSON
        let json = serde_json::to_string_pretty(session)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        
        // Write to temp file
        tokio::fs::write(&temp_path, json).await?;
        
        // Atomic rename
        tokio::fs::rename(&temp_path, &final_path).await?;
        
        Ok(())
    }
}

impl Default for SessionPersistence {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating persisted sessions
pub struct SessionBuilder {
    id: String,
    history: Vec<Message>,
    metadata: SessionMetadata,
    incognito: bool,
}

impl SessionBuilder {
    /// Create a new session builder
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            history: Vec::new(),
            metadata: SessionMetadata::default(),
            incognito: false,
        }
    }
    
    /// Set session ID
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }
    
    /// Set chat history
    pub fn with_history(mut self, history: Vec<Message>) -> Self {
        self.metadata.message_count = history.len();
        self.history = history;
        self
    }
    
    /// Set incognito mode
    pub fn with_incognito(mut self, incognito: bool) -> Self {
        self.incognito = incognito;
        self
    }
    
    /// Set session title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.metadata.title = Some(title.into());
        self
    }
    
    /// Add token usage
    pub fn with_usage(mut self, usage: &TokenUsage) -> Self {
        self.metadata.total_tokens = usage.total_tokens;
        self.metadata.input_tokens = usage.prompt_tokens;
        self.metadata.output_tokens = usage.completion_tokens;
        self
    }
    
    /// Build the persisted session
    pub fn build(self) -> PersistedSession {
        let now = Utc::now();
        PersistedSession {
            id: self.id,
            timestamp: now,
            updated_at: now,
            history: self.history,
            metadata: self.metadata,
            agent_state: None,
            incognito: self.incognito,
        }
    }
}

impl Default for SessionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_session_builder() {
        let session = SessionBuilder::new()
            .with_title("Test Session")
            .with_incognito(false)
            .build();
        
        assert_eq!(session.metadata.title, Some("Test Session".to_string()));
        assert!(!session.incognito);
    }
    
    #[test]
    fn test_session_metadata_default() {
        let meta = SessionMetadata::default();
        assert_eq!(meta.message_count, 0);
        assert_eq!(meta.total_tokens, 0);
    }
}
