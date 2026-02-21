//! Coordination board for inter-agent communication

use crate::agent::identity::AgentId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Coordination entry for the commonboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationEntry {
    /// Entry ID
    pub id: Uuid,
    /// Agent that created this entry
    pub agent_id: AgentId,
    /// Entry type: claim, progress, complete, signal
    pub entry_type: String,
    /// Resource being claimed (for claims) or message content
    pub content: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Optional tags
    pub tags: Vec<String>,
}

/// Coordination board for inter-agent communication
#[derive(Debug, Clone, Default)]
pub struct CoordinationBoard {
    /// All coordination entries
    entries: Vec<CoordinationEntry>,
}

impl CoordinationBoard {
    /// Add a new entry
    pub fn add(&mut self, entry: CoordinationEntry) {
        self.entries.push(entry);
    }

    /// Get all entries
    pub fn list(&self) -> &[CoordinationEntry] {
        &self.entries
    }

    /// Find claims for a specific resource
    pub fn find_claims(&self, resource: &str) -> Vec<&CoordinationEntry> {
        self.entries
            .iter()
            .filter(|e| e.entry_type == "claim" && e.content.contains(resource))
            .collect()
    }

    /// Find entries by agent
    pub fn find_by_agent(&self, agent_id: &AgentId) -> Vec<&CoordinationEntry> {
        self.entries
            .iter()
            .filter(|e| &e.agent_id == agent_id)
            .collect()
    }

    /// Remove a specific claim by resource.
    /// Returns the entry if found and removed.
    pub fn remove_claim(&mut self, resource: &str) -> Option<CoordinationEntry> {
        let idx = self.entries.iter().position(|e| {
            e.entry_type == "claim" && e.content == resource
        })?;
        Some(self.entries.remove(idx))
    }

    /// Clear completed entries older than threshold
    pub fn cleanup_completed(&mut self, older_than: DateTime<Utc>) {
        self.entries.retain(|e| {
            !(e.entry_type == "complete" && e.timestamp < older_than)
        });
    }

    /// Format as LLM-readable string
    pub fn format_for_llm(&self) -> String {
        if self.entries.is_empty() {
            return "No coordination entries.".to_string();
        }

        let lines: Vec<String> = self
            .entries
            .iter()
            .map(|e| {
                format!(
                    "[{}] {}: {}",
                    e.entry_type.to_uppercase(),
                    e.agent_id.short_name(),
                    e.content
                )
            })
            .collect();

        lines.join("\n")
    }
}
