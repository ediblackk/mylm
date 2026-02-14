//! Scratchpad tool for short-term agent memory.
//!
//! Provides a persistent workspace for agents to store temporary notes,
//! plans, and todo lists during task execution. Supports structured entries
//! with timestamps, TTL, tags, persistent flag, and automatic cleanup.
//!
//! # Main Types
//! - `ScratchpadTool`: Tool implementation for scratchpad operations
//! - `StructuredScratchpad`: Internal structured storage with backward compatibility

use anyhow::Result;
use async_trait::async_trait;
use crate::agent_old::tool::{Tool, ToolOutput, ToolKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock; // For internal caches (short-lived operations)
use chrono::{DateTime, Utc, Duration};
use uuid::Uuid;

/// Scratchpad size warning threshold (increased from 4000)
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
    /// Tags for categorization and filtering
    pub tags: Vec<String>,
    /// Persistent flag: if true, entry must be explicitly deleted (not auto-removed)
    pub persistent: bool,
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
        }
    }

    /// Create a new entry with custom parameters
    pub fn with_params(
        content: String,
        ttl: Option<Duration>,
        tags: Vec<String>,
        persistent: bool,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            content,
            ttl,
            tags,
            persistent,
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
}

/// Structured scratchpad that maintains entries with full metadata
/// Provides backward-compatible string interface for legacy code
#[derive(Clone)]
pub struct StructuredScratchpad {
    entries: HashMap<EntryId, ScratchpadEntry>,
    /// Cached concatenated string for efficient to_string() calls
    cached_string: Arc<RwLock<String>>,
    /// Cache invalidation flag
    cache_valid: Arc<RwLock<bool>>,
}

impl StructuredScratchpad {
    /// Create a new empty structured scratchpad
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            cached_string: Arc::new(RwLock::new(String::new())),
            cache_valid: Arc::new(RwLock::new(true)),
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
    ) -> EntryId {
        let entry = ScratchpadEntry::with_params(content, ttl, tags, persistent);
        let id = entry.id;
        self.entries.insert(id, entry);
        self.invalidate_cache();
        id
    }

    /// Remove an entry by ID
    /// Returns true if entry was removed, false if not found
    pub fn remove(&mut self, id: EntryId) -> bool {
        let removed = self.entries.remove(&id).is_some();
        if removed {
            self.invalidate_cache();
        }
        removed
    }

    /// Get an entry by ID
    pub fn get(&self, id: EntryId) -> Option<&ScratchpadEntry> {
        self.entries.get(&id)
    }

    /// List entries older than given duration
    pub fn list_by_age(&self, max_age: Duration) -> Vec<ScratchpadEntry> {
        self.entries
            .values()
            .filter(|e| e.is_older_than(max_age))
            .cloned()
            .collect()
    }

    /// List entries with given tag
    pub fn list_by_tag(&self, tag: &str) -> Vec<ScratchpadEntry> {
        self.entries
            .values()
            .filter(|e| e.tags.iter().any(|t| t == tag))
            .cloned()
            .collect()
    }

    /// Summarize non-persistent entries older than threshold
    /// Returns a formatted string with entry IDs, timestamps, and content previews
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

    /// Invalidate the cached string representation
    fn invalidate_cache(&self) {
        if let Ok(mut valid) = self.cache_valid.write() {
            *valid = false;
        }
    }

    /// Rebuild the cached string from all entries
    fn rebuild_cache(&self) {
        if let Ok(valid) = self.cache_valid.read() {
            if *valid {
                return;
            }
        }

        let mut full = String::new();
        // Sort entries by timestamp for consistent ordering
        let mut sorted: Vec<_> = self.entries.values().collect();
        sorted.sort_by_key(|e| e.timestamp);

        for entry in sorted {
            let icon = if entry.persistent { "🔒" } else { "📝" };
            let tag_str = if entry.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", entry.tags.join(", "))
            };
            let ttl_str = match entry.ttl {
                Some(d) => format!(" (TTL: {:?})", d),
                None => String::new(),
            };
            full.push_str(&format!(
                "[{}] {}{}{}: {}\n",
                entry.timestamp.format("%H:%M:%S"),
                icon,
                tag_str,
                ttl_str,
                entry.content.lines().next().unwrap_or(&entry.content)
            ));
        }

        if let Ok(mut cached) = self.cached_string.write() {
            *cached = full;
        }
        if let Ok(mut valid) = self.cache_valid.write() {
            *valid = true;
        }
    }

    /// Convert to concatenated string (backward compatibility)
    pub fn to_string(&self) -> String {
        self.rebuild_cache();
        match self.cached_string.read() {
            Ok(cached) => cached.clone(),
            Err(_) => String::new(),
        }
    }

    /// Clear all entries (except persistent ones by default)
    /// If `keep_persistent` is true, persistent entries are preserved
    pub fn clear(&mut self, keep_persistent: bool) {
        if keep_persistent {
            self.entries.retain(|_, e| e.persistent);
        } else {
            self.entries.clear();
        }
        self.invalidate_cache();
    }

    /// Automatic cleanup: remove expired entries and old non-persistent entries
    /// Returns the number of entries removed
    pub fn cleanup(&mut self) -> usize {
        let _now = Utc::now();
        let mut removed = 0;
        let threshold = Utc::now() - CLEANUP_AGE_THRESHOLD;

        // Collect IDs to remove
        let to_remove: Vec<_> = self.entries.iter()
            .filter(|(_, e)| {
                // Remove if expired OR (non-persistent AND older than threshold)
                e.is_expired() || (!e.persistent && e.timestamp < threshold)
            })
            .map(|(id, _)| *id)
            .collect();

        for id in to_remove {
            if self.entries.remove(&id).is_some() {
                removed += 1;
            }
        }

        if removed > 0 {
            self.invalidate_cache();
        }

        removed
    }

    /// Get all entry IDs (for listing/debugging)
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

    /// Legacy: push_str appends as a non-persistent entry with default TTL
    pub fn push_str(&mut self, content: &str) {
        self.append(content.to_string(), DEFAULT_ENTRY_TTL, Vec::new(), false);
    }

    /// Legacy: clear everything (including persistent)
    pub fn clear_all(&mut self) {
        self.clear(false);
    }

    /// Legacy: get as string (same as to_string)
    pub fn as_str(&self) -> String {
        self.to_string()
    }
}

/// The enhanced scratchpad tool with structured entries support
/// Uses tokio::sync::RwLock for async compatibility
#[derive(Clone)]
pub struct ScratchpadTool {
    scratchpad: Arc<tokio::sync::RwLock<StructuredScratchpad>>,
}

impl ScratchpadTool {
    pub fn new(scratchpad: Arc<tokio::sync::RwLock<StructuredScratchpad>>) -> Self {
        Self { scratchpad }
    }

    /// Get scratchpad content as string
    pub async fn get_full_content(&self) -> String {
        let scratchpad = self.scratchpad.read().await.to_string();
        scratchpad
    }

    /// Check if scratchpad needs consolidation
    pub async fn check_size(&self) -> (usize, bool, bool) {
        let scratchpad = self.scratchpad.read().await.get_size();

        let warning = scratchpad > SCRATCHPAD_WARNING_SIZE;
        let critical = scratchpad > SCRATCHPAD_CRITICAL_SIZE;

        (scratchpad, warning, critical)
    }
}

#[derive(Debug, Deserialize)]
struct ScratchpadArgs {
    text: Option<String>,
    #[serde(default = "default_action")]
    action: String,
    /// TTL in seconds for entries (overrides default)
    #[serde(default)]
    ttl_seconds: Option<i64>,
    /// Tags for categorization
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// Persistent flag: if true, entry won't be auto-cleaned
    #[serde(default)]
    persistent: Option<bool>,
    /// Entry ID for list/delete actions
    #[serde(default)]
    entry_id: Option<String>,
    /// Force flag for delete (allows deleting persistent entries)
    #[serde(default)]
    force: Option<bool>,
}

fn default_action() -> String {
    "overwrite".to_string()
}

#[async_trait]
impl Tool for ScratchpadTool {
    fn name(&self) -> &str {
        "scratchpad"
    }

    fn description(&self) -> &str {
        "Manage a persistent scratchpad with structured entries. Supports append, overwrite, clear, list, delete, and tagging. Entries have TTL and persistent flags."
    }

    fn usage(&self) -> &str {
        r#"
        {
            "text": "content to store",
            "action": "overwrite" | "append" | "clear" | "list" | "delete" | "tag",
            "ttl_seconds": 3600,
            "tags": ["tag1", "tag2"],
            "persistent": false,
            "entry_id": "uuid",
            "force": false
        }
        "#
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Content to store in the scratchpad"
                },
                "action": {
                    "type": "string",
                    "enum": ["overwrite", "append", "clear", "list", "delete", "tag"],
                    "description": "Action to perform"
                },
                "ttl_seconds": {
                    "type": "integer",
                    "description": "Time-to-live in seconds (default: 3600 = 1 hour)"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tags for categorization"
                },
                "persistent": {
                    "type": "boolean",
                    "description": "If true, entry won't be auto-cleaned (default: false)"
                },
                "entry_id": {
                    "type": "string",
                    "description": "Entry UUID for delete/tag actions"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force delete persistent entries (use with caution)"
                }
            },
            "required": ["action"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn std::error::Error + Send + Sync>> {
        let args: ScratchpadArgs = serde_json::from_str(args)?;

        let mut scratchpad = self.scratchpad.write().await;

        match args.action.as_str() {
            "overwrite" => {
                if let Some(text) = args.text {
                    // Clear all non-persistent entries
                    scratchpad.clear(true);
                    // Add new entry
                    let ttl = args.ttl_seconds.map(Duration::seconds);
                    let tags = args.tags.unwrap_or_default();
                    let persistent = args.persistent.unwrap_or(false);
                    scratchpad.append(text, ttl, tags, persistent);
                    let size = scratchpad.get_size();
                    Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                        "Scratchpad overwritten. Size: {} chars, entries: {}",
                        size,
                        scratchpad.len()
                    ))))
                } else {
                    Ok(ToolOutput::Immediate(serde_json::Value::String("Error: 'text' is required for overwrite action.".to_string())))
                }
            },
            "append" => {
                if let Some(text) = args.text {
                    let ttl = args.ttl_seconds.map(Duration::seconds);
                    let tags = args.tags.unwrap_or_default();
                    let persistent = args.persistent.unwrap_or(false);
                    scratchpad.append(text, ttl, tags, persistent);
                    let size = scratchpad.get_size();
                    Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                        "Appended to scratchpad. Size: {} chars, entries: {}",
                        size,
                        scratchpad.len()
                    ))))
                } else {
                    Ok(ToolOutput::Immediate(serde_json::Value::String("Error: 'text' is required for append action.".to_string())))
                }
            },
            "clear" => {
                // Clear non-persistent entries only
                let kept = scratchpad.entries.values().filter(|e| e.persistent).count();
                scratchpad.clear(true);
                Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                    "Cleared non-persistent entries. {} persistent entries remain.",
                    kept
                ))))
            },
            "list" => {
                // List all entries with metadata
                let entries: Vec<_> = scratchpad.entries.values()
                    .map(|e| serde_json::json!({
                        "id": e.id.to_string(),
                        "timestamp": e.timestamp.to_rfc3339(),
                        "content_preview": &e.content[..e.content.len().min(100)],
                        "content_length": e.content.len(),
                        "ttl": e.ttl.map(|d| d.num_seconds()),
                        "tags": e.tags,
                        "persistent": e.persistent
                    }))
                    .collect();

                let output = serde_json::json!({
                    "entries": entries,
                    "total_entries": scratchpad.len(),
                    "total_size": scratchpad.get_size()
                });
                Ok(ToolOutput::Immediate(output))
            },
            "delete" => {
                if let Some(entry_id_str) = args.entry_id {
                    if let Ok(entry_id) = Uuid::parse_str(&entry_id_str) {
                        let force = args.force.unwrap_or(false);
                        if let Some(entry) = scratchpad.entries.get(&entry_id) {
                            if entry.persistent && !force {
                                Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                                    "Cannot delete persistent entry {}. Use force=true to override.",
                                    entry_id
                                ))))
                            } else {
                                let removed = scratchpad.remove(entry_id);
                                if removed {
                                    Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                                        "Entry {} deleted.",
                                        entry_id
                                    ))))
                                } else {
                                    Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                                        "Entry {} not found.",
                                        entry_id
                                    ))))
                                }
                            }
                        } else {
                            Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                                "Entry {} not found.",
                                entry_id
                            ))))
                        }
                    } else {
                        Ok(ToolOutput::Immediate(serde_json::Value::String("Invalid entry ID format.".to_string())))
                    }
                } else {
                    Ok(ToolOutput::Immediate(serde_json::Value::String("Error: 'entry_id' is required for delete action.".to_string())))
                }
            },
            "tag" => {
                if let Some(entry_id_str) = args.entry_id {
                    if let Ok(entry_id) = Uuid::parse_str(&entry_id_str) {
                        if let Some(entry) = scratchpad.entries.get_mut(&entry_id) {
                            if let Some(tags) = args.tags {
                                // Add new tags (avoid duplicates)
                                for tag in tags {
                                    if !entry.tags.contains(&tag) {
                                        entry.tags.push(tag);
                                    }
                                }
                                Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                                    "Added tags to entry {}. Current tags: {:?}",
                                    entry_id, entry.tags
                                ))))
                            } else {
                                Ok(ToolOutput::Immediate(serde_json::Value::String("Error: 'tags' is required for tag action.".to_string())))
                            }
                        } else {
                            Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                                "Entry {} not found.",
                                entry_id
                            ))))
                        }
                    } else {
                        Ok(ToolOutput::Immediate(serde_json::Value::String("Invalid entry ID format.".to_string())))
                    }
                } else {
                    Ok(ToolOutput::Immediate(serde_json::Value::String("Error: 'entry_id' is required for tag action.".to_string())))
                }
            },
            _ => {
                Ok(ToolOutput::Immediate(serde_json::Value::String(format!("Unknown action: {}", args.action))))
            }
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structured_scratchpad_basic() {
        let mut scratchpad = StructuredScratchpad::new();
        
        // Append entries
        let id1 = scratchpad.append("First entry".to_string(), None, Vec::new(), false);
        let _id2 = scratchpad.append("Second entry".to_string(), Some(Duration::hours(2)), vec!("important".to_string()), true);
        
        assert_eq!(scratchpad.len(), 2);
        assert_eq!(scratchpad.get_size(), 24); // "First entry" (11) + "Second entry" (12) + newlines

        // Check to_string includes both entries
        let full = scratchpad.to_string();
        assert!(full.contains("First entry"));
        assert!(full.contains("Second entry"));
        assert!(full.contains("🔒")); // persistent marker
        assert!(full.contains("important")); // tag

        // Remove non-persistent entry
        let removed = scratchpad.remove(id1);
        assert!(removed);
        assert_eq!(scratchpad.len(), 1);

        // Clear non-persistent only (should not remove persistent)
        scratchpad.clear(true);
        assert_eq!(scratchpad.len(), 1); // persistent remains

        // Clear all
        scratchpad.clear(false);
        assert_eq!(scratchpad.len(), 0);
    }

    #[test]
    fn test_cleanup_expired_old_entries() {
        let mut scratchpad = StructuredScratchpad::new();

        // Add an expired entry (TTL in past)
        let mut expired_entry = ScratchpadEntry::new("Expired soon".to_string());
        expired_entry.ttl = Some(Duration::seconds(1));
        expired_entry.timestamp = Utc::now() - Duration::seconds(10);
        let expired_id = expired_entry.id;
        scratchpad.entries.insert(expired_id, expired_entry);

        // Add a non-persistent old entry (older than 1 hour)
        let mut old_entry = ScratchpadEntry::new("Old non-persistent".to_string());
        old_entry.persistent = false;
        old_entry.timestamp = Utc::now() - Duration::hours(2);
        let old_id = old_entry.id;
        scratchpad.entries.insert(old_id, old_entry);

        // Add a persistent recent entry (should NOT be cleaned)
        let mut persistent_recent = ScratchpadEntry::new("Persistent recent".to_string());
        persistent_recent.persistent = true;
        persistent_recent.timestamp = Utc::now() - Duration::minutes(30);
        let persistent_id = persistent_recent.id;
        scratchpad.entries.insert(persistent_id, persistent_recent);

        // Run cleanup
        let removed = scratchpad.cleanup();
        assert_eq!(removed, 2); // expired + old non-persistent
        assert_eq!(scratchpad.len(), 1); // only persistent recent remains
        assert!(scratchpad.get(persistent_id).is_some());
        assert!(scratchpad.get(expired_id).is_none());
        assert!(scratchpad.get(old_id).is_none());
    }

    #[test]
    fn test_list_by_tag_and_age() {
        let mut scratchpad = StructuredScratchpad::new();

        let _id1 = scratchpad.append("Tagged entry".to_string(), None, vec!("work".to_string()), false);
        let _id2 = scratchpad.append("Another tagged".to_string(), None, vec!("personal".to_string()), false);
        let _id3 = scratchpad.append("Work task".to_string(), None, vec!("work".to_string()), false);

        // List by tag
        let work_entries = scratchpad.list_by_tag("work");
        assert_eq!(work_entries.len(), 2);

        // List by age (all entries are recent, so empty)
        let old = scratchpad.list_by_age(Duration::hours(1));
        assert_eq!(old.len(), 0);
    }

    #[test]
    fn test_summarize_old_entries() {
        let mut scratchpad = StructuredScratchpad::new();

        // Add old non-persistent entry
        let mut old_entry = ScratchpadEntry::new("Important old data".to_string());
        old_entry.persistent = false;
        old_entry.timestamp = Utc::now() - Duration::hours(3);
        scratchpad.entries.insert(old_entry.id, old_entry);

        let summary = scratchpad.summarize_old_entries(Duration::hours(1));
        assert!(summary.contains("Important old data"));
        assert!(summary.contains("old non-persistent entries"));

        // No old entries after cleanup
        scratchpad.cleanup();
        let summary2 = scratchpad.summarize_old_entries(Duration::hours(1));
        assert!(summary2.contains("No old non-persistent entries"));
    }
}
