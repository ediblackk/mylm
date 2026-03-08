//! Notes Tool
//!
//! Provides agent access to user notes stored in JSON file.
//!
//! # Usage
//!
//! Read all notes:
//! - `notes("read")`
//! - `notes({"action": "read"})`
//!
//! Search notes:
//! - `notes("search: query")`
//! - `notes({"action": "search", "query": "query"})`

use crate::agent::runtime::core::{Capability, ToolCapability};
use crate::agent::runtime::core::RuntimeContext;
use crate::agent::runtime::core::ToolError;
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Note tag categories
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteTag {
    Work,
    Personal,
    Urgent,
    Funny,
    Diary,
}

impl std::fmt::Display for NoteTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoteTag::Work => write!(f, "work"),
            NoteTag::Personal => write!(f, "personal"),
            NoteTag::Urgent => write!(f, "urgent"),
            NoteTag::Funny => write!(f, "funny"),
            NoteTag::Diary => write!(f, "diary"),
        }
    }
}

/// A single note
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub content: String,
    pub tag: NoteTag,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Notes data structure for JSON storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotesData {
    pub version: String,
    pub notes: Vec<Note>,
}

/// Notes tool for accessing user notes
pub struct NotesTool;

impl NotesTool {
    /// Create a new notes tool
    pub fn new() -> Self {
        Self
    }
    
    /// Get notes file path
    fn get_notes_path(&self) -> Result<PathBuf, ToolError> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| ToolError::new("Could not find config directory"))?;
        let mylm_dir = config_dir.join("mylm");
        std::fs::create_dir_all(&mylm_dir)
            .map_err(|e| ToolError::new(format!("Failed to create config dir: {}", e)))?;
        Ok(mylm_dir.join("notes.json"))
    }
    
    /// Load notes from file
    fn load_notes(&self) -> Result<Vec<Note>, ToolError> {
        let path = self.get_notes_path()?;
        
        if !path.exists() {
            return Ok(vec![]);
        }
        
        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolError::new(format!("Failed to read notes: {}", e)))?;
        
        let data: NotesData = serde_json::from_str(&content)
            .map_err(|e| ToolError::new(format!("Failed to parse notes: {}", e)))?;
        
        Ok(data.notes)
    }
    
    /// Read all notes
    async fn read_notes(&self) -> Result<ToolResult, ToolError> {
        let notes = self.load_notes()?;
        
        if notes.is_empty() {
            return Ok(ToolResult::Success {
                output: "No notes found.".to_string(),
                structured: None,
            });
        }
        
        // Format notes for display
        let mut output = format!("Found {} notes:\n\n", notes.len());
        
        for (i, note) in notes.iter().take(10).enumerate() {
            let tag_str = format!("[{:?}]", note.tag);
            let preview = if note.content.len() > 100 {
                format!("{}...", &note.content[..100])
            } else {
                note.content.clone()
            };
            output.push_str(&format!("{}. {} {}\n   {}\n\n", 
                i + 1, 
                tag_str,
                chrono::DateTime::from_timestamp_millis(note.created_at)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "unknown date".to_string()),
                preview
            ));
        }
        
        if notes.len() > 10 {
            output.push_str(&format!("... and {} more notes\n", notes.len() - 10));
        }
        
        Ok(ToolResult::Success {
            output,
            structured: Some(serde_json::to_value(notes).unwrap_or_default()),
        })
    }
    
    /// Search notes
    async fn search_notes(&self, query: &str) -> Result<ToolResult, ToolError> {
        let notes = self.load_notes()?;
        let query_lower = query.to_lowercase();
        
        let filtered: Vec<&Note> = notes
            .iter()
            .filter(|n| {
                n.content.to_lowercase().contains(&query_lower) ||
                n.tag.to_string().to_lowercase().contains(&query_lower)
            })
            .collect();
        
        if filtered.is_empty() {
            return Ok(ToolResult::Success {
                output: format!("No notes found matching '{}'", query),
                structured: None,
            });
        }
        
        let mut output = format!("Found {} notes matching '{}':\n\n", filtered.len(), query);
        
        for (i, note) in filtered.iter().take(10).enumerate() {
            let tag_str = format!("[{:?}]", note.tag);
            let preview = if note.content.len() > 100 {
                format!("{}...", &note.content[..100])
            } else {
                note.content.clone()
            };
            output.push_str(&format!("{}. {} {}\n   {}\n\n", 
                i + 1, 
                tag_str,
                chrono::DateTime::from_timestamp_millis(note.created_at)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "unknown date".to_string()),
                preview
            ));
        }
        
        if filtered.len() > 10 {
            output.push_str(&format!("... and {} more notes\n", filtered.len() - 10));
        }
        
        Ok(ToolResult::Success {
            output,
            structured: Some(serde_json::to_value(filtered).unwrap_or_default()),
        })
    }
}

impl Capability for NotesTool {
    fn name(&self) -> &'static str {
        "notes"
    }
}

#[async_trait::async_trait]
impl ToolCapability for NotesTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse arguments
        let args = &call.arguments;
        
        // Get action from arguments
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("Missing 'action' field. Use 'read' or 'search'"))?;
        
        match action {
            "read" | "list" => self.read_notes().await,
            "search" => {
                let query = args.get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Search requires 'query' field"))?;
                self.search_notes(query).await
            }
            _ => Ok(ToolResult::Error {
                message: format!("Unknown action: {}. Use 'read' or 'search'", action),
                code: Some("INVALID_ACTION".to_string()),
                retryable: false,
            })
        }
    }
}

impl Default for NotesTool {
    fn default() -> Self {
        Self::new()
    }
}
