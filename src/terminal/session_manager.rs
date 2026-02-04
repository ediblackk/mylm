use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration};
use serde_json;
use crate::terminal::session::Session;

/// SessionManager handles automatic, debounced, atomic session persistence.
/// 
/// It spawns a background task that receives sessions via a channel and
/// writes them to disk with debouncing to avoid excessive I/O. All writes
/// are atomic using temp file + rename pattern.
pub struct SessionManager {
    current_session: Arc<RwLock<Option<Session>>>,
    save_sender: mpsc::Sender<Session>,
    _save_task: tokio::task::JoinHandle<()>,
}

impl SessionManager {
    /// Create a new SessionManager.
    /// 
    /// 1. Determines sessions directory: $XDG_DATA_DIR/mylm/sessions/ or $HOME/.local/share/mylm/sessions/
    /// 2. Creates the directory if it doesn't exist
    /// 3. Creates an mpsc channel (buffer size 1, we only care about latest)
    /// 4. Spawns background task that:
    ///    - Receives sessions from save_sender
    ///    - Maintains a pending session Option
    ///    - Uses a 500ms debounce interval
    ///    - When debounce expires, writes session to disk atomically:
    ///      a) Write to temp file: sessions_dir/join(format!("session_{}.tmp", session.id))
    ///      b) Rename to final: sessions_dir/join(format!("session_{}.json", session.id))
    ///      c) Also update latest.json (atomic rename as well)
    ///    - Handles errors gracefully (logs but doesn't panic)
    pub fn new() -> Self {
        let (save_sender, mut save_receiver) = mpsc::channel::<Session>(1);
        let sessions_dir = Self::resolve_sessions_dir();
        
        // Create directory if it doesn't exist
        std::fs::create_dir_all(&sessions_dir).expect("Failed to create sessions directory");
        
        let current_session = Arc::new(RwLock::new(None));
        let current_session_clone = current_session.clone();
        
        // Clone sessions_dir for use in the async task
        let sessions_dir_clone = sessions_dir.clone();
        
        // Spawn background save task
        let save_task = tokio::spawn(async move {
            let mut pending_session: Option<Session> = None;
            let mut debounce_interval = interval(Duration::from_millis(500));
            
            loop {
                tokio::select! {
                    // Receive new session from channel
                    received_session = save_receiver.recv() => {
                        match received_session {
                            Some(session) => {
                                pending_session = Some(session);
                                // Reset debounce timer by dropping and recreating interval
                                // Actually, we just update pending and let the interval tick
                            }
                            None => {
                                // Channel closed, exit task
                                mylm_core::debug_log!("[SessionManager] Save channel closed, exiting background task");
                                break;
                            }
                        }
                    }
                    
                    // Debounce timer tick
                    _ = debounce_interval.tick() => {
                        if let Some(session) = pending_session.take() {
                            // Save session to disk atomically
                            if let Err(e) = Self::save_session_atomic(&sessions_dir_clone, &session).await {
                                mylm_core::error_log!("[SessionManager] Failed to save session {}: {}", session.id, e);
                            } else {
                                // Update latest.json atomically
                                if let Err(e) = Self::update_latest_atomic(&sessions_dir_clone, &session).await {
                                    mylm_core::error_log!("[SessionManager] Failed to update latest.json: {}", e);
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
        }
    }
    
    /// Save a session asynchronously.
    /// Non-blocking, fire-and-forget. The session is cloned and sent to the
    /// background task via channel.
    #[allow(dead_code)]
    pub async fn save_session_async(&self, session: &Session) {
        let session_clone = session.clone();
        if let Err(e) = self.save_sender.send(session_clone).await {
            mylm_core::error_log!("[SessionManager] Failed to send session to background task: {}", e);
        }
    }
    
    /// Set the current session.
    /// Stores in self.current_session (using RwLock for interior mutability)
    /// and triggers an immediate save via the channel.
    pub fn set_current_session(&self, session: Session) {
        // Update current session
        if let Ok(mut guard) = self.current_session.try_write() {
            *guard = Some(session.clone());
        } else {
            mylm_core::error_log!("[SessionManager] Failed to acquire write lock for current_session");
        }
        
        // Trigger immediate save (spawn a task to send async)
        let sender = self.save_sender.clone();
        tokio::spawn(async move {
            if let Err(e) = sender.send(session).await {
                mylm_core::error_log!("[SessionManager] Failed to send session in set_current_session: {}", e);
            }
        });
    }
    
    /// Get the current session (cloned).
    #[allow(dead_code)]
    pub async fn get_current_session(&self) -> Option<Session> {
        let guard = self.current_session.read().await;
        guard.clone()
    }
    
    /// Load the latest session from latest.json.
    /// Async method - can be called from sync context via tokio::runtime::Handle::current().block_on()
    pub async fn load_latest() -> Option<Session> {
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
                        mylm_core::error_log!("[SessionManager] Failed to deserialize latest.json: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                mylm_core::error_log!("[SessionManager] Failed to read latest.json: {}", e);
                None
            }
        }
    }
    
    /// Load all sessions from the sessions directory.
    /// Scans for files matching "session_*.json", excludes "latest.json".
    /// Deserializes each and returns sorted by timestamp descending.
    pub fn load_sessions() -> Vec<Session> {
        let sessions_dir = Self::resolve_sessions_dir();
        let mut sessions: Vec<Session> = Vec::new();
        
        // Read directory entries
        let entries = match std::fs::read_dir(&sessions_dir) {
            Ok(entries) => entries,
            Err(e) => {
                mylm_core::error_log!("[SessionManager] Failed to read sessions directory: {}", e);
                return Vec::new();
            }
        };
        
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                // Skip latest.json
                if file_name == "latest.json" {
                    continue;
                }
                
                // Match session_*.json pattern
                if file_name.starts_with("session_") && file_name.ends_with(".json") {
                    // Skip temp files
                    if file_name.ends_with(".tmp") {
                        continue;
                    }
                    
                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            match serde_json::from_str(&content) {
                                Ok(session) => sessions.push(session),
                                Err(e) => {
                                    mylm_core::error_log!("[SessionManager] Failed to deserialize {}: {}", file_name, e);
                                }
                            }
                        }
                        Err(e) => {
                            mylm_core::error_log!("[SessionManager] Failed to read {}: {}", file_name, e);
                        }
                    }
                }
            }
        }
        
        // Sort by timestamp descending (most recent first)
        sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        sessions
    }
    
    /// Delete a session by ID.
    /// Deletes session_{id}.json and any associated temp files.
    /// Returns Result<(), std::io::Error> for caller to handle.
    #[allow(dead_code)]
    pub async fn delete_session(id: &str) -> Result<(), std::io::Error> {
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
        
        Ok(())
    }
    
    /// Resolve the sessions directory path.
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
    
    /// Save session to disk atomically: write to temp file, then rename.
    async fn save_session_atomic(dir: &PathBuf, session: &Session) -> Result<(), std::io::Error> {
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
    
    /// Update latest.json atomically: write to temp file, then rename.
    async fn update_latest_atomic(dir: &PathBuf, session: &Session) -> Result<(), std::io::Error> {
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
