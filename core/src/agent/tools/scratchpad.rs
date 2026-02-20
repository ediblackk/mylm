//! Scratchpad Tool - Shared workspace for worker coordination
//!
//! Provides a persistent, structured workspace for agents to store temporary notes,
//! coordinate between workers, and track task progress. Supports structured entries
//! with timestamps, TTL, tags, persistent flags, and automatic cleanup.
//!
//! # Coordination Protocol
//! Workers use the scratchpad for coordination:
//! - **CLAIM**: Before working on a file/resource
//! - **REPORT**: Progress updates
//! - **COMPLETE**: Task finished
//! - **SIGNAL**: Dependency ready

use crate::agent::runtime::core::{Capability, ToolCapability};
use crate::agent::runtime::core::RuntimeContext;
use crate::agent::runtime::core::ToolError;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Scratchpad size warning threshold
pub const SCRATCHPAD_WARNING_SIZE: usize = 8000;
/// Scratchpad critical size - force consolidation
pub const SCRATCHPAD_CRITICAL_SIZE: usize = 12000;
/// Default TTL for appended entries (1 hour)
pub const DEFAULT_ENTRY_TTL: Option<Duration> = Some(Duration::hours(1));
/// Age threshold for automatic cleanup (1 hour)
pub const CLEANUP_AGE_THRESHOLD: Duration = Duration::hours(1);

/// Type alias for entry ID
pub type EntryId = Uuid;

/// A structured scratchpad entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadEntry {
    /// Unique entry ID
    pub id: EntryId,
    /// Timestamp when entry was created
    pub timestamp: DateTime<Utc>,
    /// Entry content
    pub content: String,
    /// Time-to-live: if Some(duration), entry expires after that time
    pub ttl: Option<Duration>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Persistent flag: if true, entry must be explicitly deleted (not auto-removed)
    pub persistent: bool,
    /// Worker ID that created this entry (if from a worker)
    pub worker_id: Option<String>,
}

impl ScratchpadEntry {
    /// Create a new entry with default TTL, no tags, non-persistent
    pub fn new(content: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            content,
            ttl: DEFAULT_ENTRY_TTL,
            tags: Vec::new(),
            persistent: false,
            worker_id: None,
        }
    }

    /// Create a new entry with custom parameters
    pub fn with_params(
        content: String,
        ttl: Option<Duration>,
        tags: Vec<String>,
        persistent: bool,
        worker_id: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            content,
            ttl,
            tags,
            persistent,
            worker_id,
        }
    }

    /// Check if entry has expired based on TTL
    pub fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl {
            let age = Utc::now() - self.timestamp;
            age > ttl
        } else {
            false
        }
    }

    /// Check if entry is older than given duration
    pub fn is_older_than(&self, duration: Duration) -> bool {
        let age = Utc::now() - self.timestamp;
        age > duration
    }

    /// Format entry for display
    pub fn format(&self) -> String {
        let icon = if self.persistent { "🔒" } else { "📝" };
        let tag_str = if self.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", self.tags.join(", "))
        };
        let worker_str = self.worker_id.as_ref()
            .map(|w| format!(" <{}>", w))
            .unwrap_or_default();
        format!(
            "[{}] {}{}{}: {}",
            self.timestamp.format("%H:%M:%S"),
            icon,
            tag_str,
            worker_str,
            self.content.lines().next().unwrap_or(&self.content)
        )
    }
}

/// Structured scratchpad that maintains entries with full metadata
#[derive(Clone)]
pub struct StructuredScratchpad {
    entries: HashMap<EntryId, ScratchpadEntry>,
}

impl StructuredScratchpad {
    /// Create a new empty structured scratchpad
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Append new content as a structured entry
    /// Returns the entry ID
    pub fn append(
        &mut self,
        content: String,
        ttl: Option<Duration>,
        tags: Vec<String>,
        persistent: bool,
        worker_id: Option<String>,
    ) -> EntryId {
        let entry = ScratchpadEntry::with_params(content, ttl, tags, persistent, worker_id);
        let id = entry.id;
        self.entries.insert(id, entry);
        id
    }

    /// Remove an entry by ID
    /// Returns true if entry was removed, false if not found
    pub fn remove(&mut self, id: EntryId) -> bool {
        self.entries.remove(&id).is_some()
    }

    /// Get an entry by ID
    pub fn get(&self, id: EntryId) -> Option<&ScratchpadEntry> {
        self.entries.get(&id)
    }

    /// List entries older than given duration
    pub fn list_by_age(&self, max_age: Duration) -> Vec<&ScratchpadEntry> {
        self.entries
            .values()
            .filter(|e| e.is_older_than(max_age))
            .collect()
    }

    /// List entries with given tag
    pub fn list_by_tag(&self, tag: &str) -> Vec<&ScratchpadEntry> {
        self.entries
            .values()
            .filter(|e| e.tags.iter().any(|t| t == tag))
            .collect()
    }

    /// List entries by worker ID
    pub fn list_by_worker(&self, worker_id: &str) -> Vec<&ScratchpadEntry> {
        self.entries
            .values()
            .filter(|e| e.worker_id.as_ref() == Some(&worker_id.to_string()))
            .collect()
    }

    /// Summarize non-persistent entries older than threshold
    pub fn summarize_old_entries(&self, older_than: Duration) -> String {
        let old_entries: Vec<_> = self.entries.values()
            .filter(|e| !e.persistent && e.is_older_than(older_than))
            .collect();

        if old_entries.is_empty() {
            return String::from("No old non-persistent entries found.");
        }

        let mut summary = format!("Found {} old non-persistent entries:\n\n", old_entries.len());
        for entry in old_entries {
            let preview = entry.content.lines().next()
                .map(|s| &s[..s.len().min(80)])
                .unwrap_or(&entry.content[..entry.content.len().min(80)]);
            summary.push_str(&format!(
                "- ID: {} | Age: {:?} | Tags: {:?}\n  Content: {}...\n",
                entry.id,
                Utc::now() - entry.timestamp,
                entry.tags,
                preview
            ));
        }
        summary
    }

    /// Get total character count across all entries
    pub fn get_size(&self) -> usize {
        self.entries.values().map(|e| e.content.len()).sum()
    }

    /// Clear all entries (except persistent ones by default)
    pub fn clear(&mut self, keep_persistent: bool) {
        if keep_persistent {
            self.entries.retain(|_, e| e.persistent);
        } else {
            self.entries.clear();
        }
    }

    /// Automatic cleanup: remove expired entries and old non-persistent entries
    /// Returns the number of entries removed
    pub fn cleanup(&mut self) -> usize {
        let threshold = Utc::now() - CLEANUP_AGE_THRESHOLD;

        let to_remove: Vec<_> = self.entries.iter()
            .filter(|(_, e)| {
                e.is_expired() || (!e.persistent && e.timestamp < threshold)
            })
            .map(|(id, _)| *id)
            .collect();

        let removed = to_remove.len();
        for id in to_remove {
            self.entries.remove(&id);
        }

        removed
    }

    /// Get all entry IDs
    pub fn get_all_ids(&self) -> Vec<EntryId> {
        self.entries.keys().cloned().collect()
    }

    /// Get entry count
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Convert to concatenated string (sorted by timestamp)
    pub fn to_string(&self) -> String {
        let mut sorted: Vec<_> = self.entries.values().collect();
        sorted.sort_by_key(|e| e.timestamp);

        sorted.iter()
            .map(|e| e.format())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get entries as JSON value
    pub fn to_json(&self) -> serde_json::Value {
        let mut sorted: Vec<_> = self.entries.values().collect();
        sorted.sort_by_key(|e| e.timestamp);

        serde_json::json!({
            "entries": sorted.iter().map(|e| serde_json::json!({
                "id": e.id.to_string(),
                "timestamp": e.timestamp.to_rfc3339(),
                "content": e.content,
                "tags": e.tags,
                "persistent": e.persistent,
                "worker_id": e.worker_id,
            })).collect::<Vec<_>>(),
            "total_entries": self.len(),
            "total_size": self.get_size(),
        })
    }
}

impl Default for StructuredScratchpad {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared scratchpad type for cross-worker coordination
pub type SharedScratchpad = Arc<RwLock<StructuredScratchpad>>;

/// Create a new shared scratchpad
pub fn create_shared_scratchpad() -> SharedScratchpad {
    Arc::new(RwLock::new(StructuredScratchpad::new()))
}

/// The scratchpad tool for agent use
#[derive(Clone)]
pub struct ScratchpadTool {
    scratchpad: SharedScratchpad,
}

impl ScratchpadTool {
    /// Create a new scratchpad tool with a shared scratchpad
    pub fn new(scratchpad: SharedScratchpad) -> Self {
        Self { scratchpad }
    }

    /// Create a new scratchpad tool with a fresh scratchpad
    pub fn new_standalone() -> Self {
        Self::new(create_shared_scratchpad())
    }

    /// Get scratchpad content as string
    pub async fn get_full_content(&self) -> String {
        self.scratchpad.read().await.to_string()
    }

    /// Check if scratchpad needs consolidation
    pub async fn check_size(&self) -> (usize, bool, bool) {
        let size = self.scratchpad.read().await.get_size();
        let warning = size > SCRATCHPAD_WARNING_SIZE;
        let critical = size > SCRATCHPAD_CRITICAL_SIZE;
        (size, warning, critical)
    }

    /// Get the underlying shared scratchpad
    pub fn shared(&self) -> SharedScratchpad {
        self.scratchpad.clone()
    }
}

impl Capability for ScratchpadTool {
    fn name(&self) -> &'static str {
        "scratchpad"
    }
}

#[async_trait::async_trait]
impl ToolCapability for ScratchpadTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        let action = call.arguments.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");

        match action {
            "append" => {
                let text = call.arguments.get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Missing 'text' for append action"))?;
                
                let ttl_seconds = call.arguments.get("ttl_seconds")
                    .and_then(|v| v.as_i64());
                let ttl = ttl_seconds.map(|s| Duration::seconds(s));
                
                let tags = call.arguments.get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect())
                    .unwrap_or_default();
                
                let persistent = call.arguments.get("persistent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                
                let worker_id = call.arguments.get("worker_id")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let mut scratchpad = self.scratchpad.write().await;
                let id = scratchpad.append(
                    text.to_string(),
                    ttl,
                    tags,
                    persistent,
                    worker_id,
                );
                let size = scratchpad.get_size();
                
                Ok(ToolResult::Success {
                    output: format!("Appended entry {}. Total size: {} chars, entries: {}", 
                        id, size, scratchpad.len()),
                    structured: Some(serde_json::json!({
                        "entry_id": id.to_string(),
                        "total_size": size,
                        "total_entries": scratchpad.len(),
                    })),
                })
            }
            
            "overwrite" => {
                let text = call.arguments.get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Missing 'text' for overwrite action"))?;
                
                let ttl_seconds = call.arguments.get("ttl_seconds")
                    .and_then(|v| v.as_i64());
                let ttl = ttl_seconds.map(|s| Duration::seconds(s));
                
                let tags = call.arguments.get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect())
                    .unwrap_or_default();
                
                let persistent = call.arguments.get("persistent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                
                let worker_id = call.arguments.get("worker_id")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let mut scratchpad = self.scratchpad.write().await;
                // Clear non-persistent entries
                scratchpad.clear(true);
                // Add new entry
                let id = scratchpad.append(
                    text.to_string(),
                    ttl,
                    tags,
                    persistent,
                    worker_id,
                );
                let size = scratchpad.get_size();
                
                Ok(ToolResult::Success {
                    output: format!("Scratchpad overwritten with entry {}. Size: {} chars, entries: {}",
                        id, size, scratchpad.len()),
                    structured: Some(serde_json::json!({
                        "entry_id": id.to_string(),
                        "total_size": size,
                        "total_entries": scratchpad.len(),
                    })),
                })
            }
            
            "clear" => {
                let mut scratchpad = self.scratchpad.write().await;
                let before = scratchpad.len();
                let kept = scratchpad.entries.values().filter(|e| e.persistent).count();
                scratchpad.clear(true);
                
                Ok(ToolResult::Success {
                    output: format!("Cleared {} non-persistent entries. {} persistent entries remain.",
                        before - kept, kept),
                    structured: Some(serde_json::json!({
                        "cleared": before - kept,
                        "persistent_remaining": kept,
                    })),
                })
            }
            
            "list" => {
                let scratchpad = self.scratchpad.read().await;
                let json = scratchpad.to_json();
                
                Ok(ToolResult::Success {
                    output: format!("Scratchpad: {} entries, {} chars", 
                        scratchpad.len(), scratchpad.get_size()),
                    structured: Some(json),
                })
            }
            
            "delete" => {
                let entry_id = call.arguments.get("entry_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Missing 'entry_id' for delete action"))?;
                
                let id = Uuid::parse_str(entry_id)
                    .map_err(|e| ToolError::new(format!("Invalid entry ID: {}", e)))?;
                
                let force = call.arguments.get("force")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                
                let mut scratchpad = self.scratchpad.write().await;
                
                if let Some(entry) = scratchpad.get(id) {
                    if entry.persistent && !force {
                        return Ok(ToolResult::Error {
                            message: format!("Cannot delete persistent entry {}. Use force=true to override.", id),
                            code: Some("PERSISTENT_ENTRY".to_string()),
                            retryable: false,
                        });
                    }
                    
                    let removed = scratchpad.remove(id);
                    if removed {
                        Ok(ToolResult::Success {
                            output: format!("Entry {} deleted.", id),
                            structured: None,
                        })
                    } else {
                        Ok(ToolResult::Error {
                            message: format!("Entry {} not found.", id),
                            code: Some("NOT_FOUND".to_string()),
                            retryable: false,
                        })
                    }
                } else {
                    Ok(ToolResult::Error {
                        message: format!("Entry {} not found.", id),
                        code: Some("NOT_FOUND".to_string()),
                        retryable: false,
                    })
                }
            }
            
            "tag" => {
                let entry_id = call.arguments.get("entry_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Missing 'entry_id' for tag action"))?;
                
                let id = Uuid::parse_str(entry_id)
                    .map_err(|e| ToolError::new(format!("Invalid entry ID: {}", e)))?;
                
                let new_tags = call.arguments.get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>())
                    .ok_or_else(|| ToolError::new("Missing 'tags' for tag action"))?;
                
                let mut scratchpad = self.scratchpad.write().await;
                
                if let Some(entry) = scratchpad.entries.get_mut(&id) {
                    // Add new tags (avoid duplicates)
                    for tag in new_tags {
                        if !entry.tags.contains(&tag) {
                            entry.tags.push(tag);
                        }
                    }
                    
                    Ok(ToolResult::Success {
                        output: format!("Added tags to entry {}. Current tags: {:?}",
                            id, entry.tags),
                        structured: Some(serde_json::json!({
                            "entry_id": id.to_string(),
                            "tags": entry.tags.clone(),
                        })),
                    })
                } else {
                    Ok(ToolResult::Error {
                        message: format!("Entry {} not found.", id),
                        code: Some("NOT_FOUND".to_string()),
                        retryable: false,
                    })
                }
            }
            
            "cleanup" => {
                let mut scratchpad = self.scratchpad.write().await;
                let before = scratchpad.len();
                let removed = scratchpad.cleanup();
                let after = scratchpad.len();
                
                Ok(ToolResult::Success {
                    output: format!("Cleanup complete. Removed {} entries. {} entries remain.",
                        removed, after),
                    structured: Some(serde_json::json!({
                        "removed": removed,
                        "remaining": after,
                        "before": before,
                    })),
                })
            }
            
            _ => {
                Ok(ToolResult::Error {
                    message: format!("Unknown scratchpad action: {}. Valid: append, overwrite, clear, list, delete, tag, cleanup", action),
                    code: Some("INVALID_ACTION".to_string()),
                    retryable: false,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scratchpad_entry() {
        let entry = ScratchpadEntry::new("Test content".to_string());
        assert!(!entry.persistent);
        assert!(entry.tags.is_empty());
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_scratchpad_append_and_get() {
        let mut scratchpad = StructuredScratchpad::new();
        let id = scratchpad.append(
            "Test entry".to_string(),
            None,
            vec!["test".to_string()],
            false,
            Some("worker-1".to_string()),
        );
        
        assert_eq!(scratchpad.len(), 1);
        
        let entry = scratchpad.get(id).unwrap();
        assert_eq!(entry.content, "Test entry");
        assert_eq!(entry.tags, vec!["test"]);
        assert_eq!(entry.worker_id, Some("worker-1".to_string()));
    }

    #[test]
    fn test_scratchpad_clear() {
        let mut scratchpad = StructuredScratchpad::new();
        
        // Add persistent entry
        scratchpad.append(
            "Persistent".to_string(),
            None,
            Vec::new(),
            true,
            None,
        );
        
        // Add non-persistent entry
        scratchpad.append(
            "Temporary".to_string(),
            None,
            Vec::new(),
            false,
            None,
        );
        
        assert_eq!(scratchpad.len(), 2);
        
        // Clear non-persistent
        scratchpad.clear(true);
        assert_eq!(scratchpad.len(), 1);
        
        // Clear all
        scratchpad.clear(false);
        assert_eq!(scratchpad.len(), 0);
    }

    #[test]
    fn test_scratchpad_cleanup() {
        let mut scratchpad = StructuredScratchpad::new();
        
        // Add expired entry
        let mut expired = ScratchpadEntry::new("Expired".to_string());
        expired.ttl = Some(Duration::seconds(1));
        expired.timestamp = Utc::now() - Duration::seconds(10);
        let expired_id = expired.id;
        scratchpad.entries.insert(expired_id, expired);
        
        // Add old non-persistent entry
        let mut old = ScratchpadEntry::new("Old".to_string());
        old.persistent = false;
        old.timestamp = Utc::now() - Duration::hours(2);
        let old_id = old.id;
        scratchpad.entries.insert(old_id, old);
        
        // Add recent persistent entry
        scratchpad.append(
            "Recent persistent".to_string(),
            None,
            Vec::new(),
            true,
            None,
        );
        
        assert_eq!(scratchpad.len(), 3);
        
        let removed = scratchpad.cleanup();
        assert_eq!(removed, 2);
        assert_eq!(scratchpad.len(), 1);
    }
}
