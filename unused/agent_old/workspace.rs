//! Shared Workspace for MoCoWorkers (Modular Conscious Workers)
//!
//! Provides a shared scratchpad with FastEmbed indexing for semantic search,
//! enabling workers to be aware of each other's activities and findings.

use crate::agent_old::v2::jobs::{JobRegistry, JobStatus};
use crate::memory::store::VectorStore;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type of entry in the shared workspace
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkspaceEntryType {
    /// Initial worker objective and plan
    Objective,
    /// Partial findings during execution
    Finding,
    /// Tool execution result
    ToolResult,
    /// Completed task result
    CompleteResult,
    /// Error or failure
    Error,
    /// Coordination message between workers
    Coordination,
    /// Warning or important note
    Warning,
}

impl std::fmt::Display for WorkspaceEntryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceEntryType::Objective => write!(f, "objective"),
            WorkspaceEntryType::Finding => write!(f, "finding"),
            WorkspaceEntryType::ToolResult => write!(f, "tool_result"),
            WorkspaceEntryType::CompleteResult => write!(f, "complete"),
            WorkspaceEntryType::Error => write!(f, "error"),
            WorkspaceEntryType::Coordination => write!(f, "coordination"),
            WorkspaceEntryType::Warning => write!(f, "warning"),
        }
    }
}

/// A single entry in the shared workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    /// Unique entry ID
    pub id: String,
    /// Job ID that created this entry
    pub job_id: String,
    /// Worker identifier (human readable)
    pub worker_name: String,
    /// Type of entry
    pub entry_type: WorkspaceEntryType,
    /// Content of the entry
    pub content: String,
    /// Optional metadata
    pub metadata: Option<serde_json::Value>,
    /// Timestamp when created
    pub created_at: DateTime<Utc>,
    /// Vector embedding for semantic search (not serialized)
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
    /// Relevance score for search results
    #[serde(skip)]
    pub relevance_score: Option<f32>,
}

impl WorkspaceEntry {
    pub fn new(
        job_id: String,
        worker_name: String,
        entry_type: WorkspaceEntryType,
        content: String,
    ) -> Self {
        Self {
            id: format!("{}_{}", job_id, Utc::now().timestamp_millis()),
            job_id,
            worker_name,
            entry_type,
            content,
            metadata: None,
            created_at: Utc::now(),
            embedding: None,
            relevance_score: None,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Format entry for display in scratchpad
    pub fn format_for_scratchpad(&self) -> String {
        let icon = match self.entry_type {
            WorkspaceEntryType::Objective => "🎯",
            WorkspaceEntryType::Finding => "🔍",
            WorkspaceEntryType::ToolResult => "🛠️",
            WorkspaceEntryType::CompleteResult => "✅",
            WorkspaceEntryType::Error => "❌",
            WorkspaceEntryType::Coordination => "📡",
            WorkspaceEntryType::Warning => "⚠️",
        };
        
        format!(
            "[{}] {} {}: {}\n",
            self.created_at.format("%H:%M:%S"),
            icon,
            self.worker_name,
            self.content.lines().next().unwrap_or(&self.content)
        )
    }
}

/// Summary of workspace activity
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceSummary {
    pub total_entries: usize,
    pub active_workers: usize,
    pub completed_workers: usize,
    pub failed_workers: usize,
    pub recent_findings: Vec<WorkspaceEntry>,
    pub last_updated: DateTime<Utc>,
}

/// Shared workspace for MoCoWorkers
/// 
/// This provides:
/// 1. A shared scratchpad that all workers can read/write
/// 2. Vector indexing for semantic search across worker outputs
/// 3. Worker awareness - knowing what other workers are doing
/// 4. Coordination - ability to send messages between workers
pub struct SharedWorkspace {
    /// Raw scratchpad content (concatenated entries for human reading)
    scratchpad: Arc<RwLock<String>>,
    /// Structured entries with metadata
    entries: Arc<RwLock<Vec<WorkspaceEntry>>>,
    /// Reference to job registry for worker awareness
    job_registry: JobRegistry,
    /// Optional vector store for semantic search
    vector_store: Option<Arc<VectorStore>>,
    /// Worker objectives (job_id -> objective)
    worker_objectives: Arc<RwLock<HashMap<String, String>>>,
    /// Last update timestamp
    last_updated: Arc<RwLock<DateTime<Utc>>>,
}

impl SharedWorkspace {
    /// Create a new shared workspace
    pub fn new(job_registry: JobRegistry) -> Self {
        Self {
            scratchpad: Arc::new(RwLock::new(String::new())),
            entries: Arc::new(RwLock::new(Vec::new())),
            job_registry,
            vector_store: None,
            worker_objectives: Arc::new(RwLock::new(HashMap::new())),
            last_updated: Arc::new(RwLock::new(Utc::now())),
        }
    }

    /// Create a new shared workspace with vector search capability
    pub fn with_vector_store(mut self, vector_store: Arc<VectorStore>) -> Self {
        self.vector_store = Some(vector_store);
        self
    }

    /// Register a worker's objective
    pub async fn register_worker(&self, job_id: &str, objective: &str) {
        let mut objectives = self.worker_objectives.write().await;
        objectives.insert(job_id.to_string(), objective.to_string());
        
        // Also add as an entry
        let entry = WorkspaceEntry::new(
            job_id.to_string(),
            format!("Worker-{}", &job_id[..8.min(job_id.len())]),
            WorkspaceEntryType::Objective,
            objective.to_string(),
        );
        
        drop(objectives);
        self.add_entry(entry).await;
        
        crate::info_log!("[WORKSPACE] Worker {} registered with objective: {}", 
            &job_id[..8.min(job_id.len())], objective);
    }

    /// Add an entry to the workspace
    pub async fn add_entry(&self, entry: WorkspaceEntry) {
        // Update scratchpad
        let formatted = entry.format_for_scratchpad();
        {
            let mut scratchpad = self.scratchpad.write().await;
            scratchpad.push_str(&formatted);
        }
        
        // Add to entries
        {
            let mut entries = self.entries.write().await;
            entries.push(entry.clone());
            
            // Keep only last 1000 entries to prevent memory bloat
            if entries.len() > 1000 {
                entries.remove(0);
            }
        }
        
        // Update timestamp
        {
            let mut last = self.last_updated.write().await;
            *last = Utc::now();
        }
        
        // If we have vector store, index this entry
        if let Some(ref store) = self.vector_store {
            let content = format!("[{}] {}", entry.entry_type, entry.content);
            let _ = store.add_memory(&content).await;
        }
        
        crate::debug_log!("[WORKSPACE] Added entry from {}: {} ({})", 
            entry.worker_name, entry.entry_type, &entry.content[..50.min(entry.content.len())]);
    }

    /// Add a finding from a worker
    pub async fn add_finding(&self, job_id: &str, finding: &str, metadata: Option<serde_json::Value>) {
        let worker_name = format!("Worker-{}", &job_id[..8.min(job_id.len())]);
        let mut entry = WorkspaceEntry::new(
            job_id.to_string(),
            worker_name,
            WorkspaceEntryType::Finding,
            finding.to_string(),
        );
        if let Some(meta) = metadata {
            entry = entry.with_metadata(meta);
        }
        self.add_entry(entry).await;
    }

    /// Add a completed result from a worker
    pub async fn add_result(&self, job_id: &str, result: &str) {
        let worker_name = format!("Worker-{}", &job_id[..8.min(job_id.len())]);
        let entry = WorkspaceEntry::new(
            job_id.to_string(),
            worker_name,
            WorkspaceEntryType::CompleteResult,
            result.to_string(),
        );
        self.add_entry(entry).await;
        
        crate::info_log!("[WORKSPACE] Worker {} completed with result: {}", 
            &job_id[..8.min(job_id.len())], &result[..100.min(result.len())]);
    }

    /// Add an error from a worker
    pub async fn add_error(&self, job_id: &str, error: &str) {
        let worker_name = format!("Worker-{}", &job_id[..8.min(job_id.len())]);
        let entry = WorkspaceEntry::new(
            job_id.to_string(),
            worker_name,
            WorkspaceEntryType::Error,
            error.to_string(),
        );
        self.add_entry(entry).await;
    }

    /// Send a coordination message (worker-to-worker communication)
    pub async fn send_coordination(&self, from_job_id: &str, to_job_id: Option<&str>, message: &str) {
        let worker_name = format!("Worker-{}", &from_job_id[..8.min(from_job_id.len())]);
        let content = if let Some(to) = to_job_id {
            format!("@Worker-{}: {}", &to[..8.min(to.len())], message)
        } else {
            format!("@all: {}", message)
        };
        
        let entry = WorkspaceEntry::new(
            from_job_id.to_string(),
            worker_name,
            WorkspaceEntryType::Coordination,
            content,
        );
        self.add_entry(entry).await;
    }

    /// Get current scratchpad content
    pub async fn get_scratchpad(&self) -> String {
        self.scratchpad.read().await.clone()
    }

    /// Get recent entries (last N)
    pub async fn get_recent_entries(&self, n: usize) -> Vec<WorkspaceEntry> {
        let entries = self.entries.read().await;
        entries.iter().rev().take(n).cloned().collect()
    }

    /// Get entries by job ID
    pub async fn get_entries_by_job(&self, job_id: &str) -> Vec<WorkspaceEntry> {
        let entries = self.entries.read().await;
        entries.iter()
            .filter(|e| e.job_id == job_id)
            .cloned()
            .collect()
    }

    /// Get entries by type
    pub async fn get_entries_by_type(&self, entry_type: WorkspaceEntryType) -> Vec<WorkspaceEntry> {
        let entries = self.entries.read().await;
        entries.iter()
            .filter(|e| e.entry_type == entry_type)
            .cloned()
            .collect()
    }

    /// Search for relevant entries using semantic similarity (requires vector store)
    pub async fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<WorkspaceEntry>> {
        if let Some(ref store) = self.vector_store {
            let memories = store.search_memory(query, limit).await?;
            
            // Convert memories back to workspace entries
            // This is a simplified mapping - in practice you'd store entry IDs with memories
            let entries = self.entries.read().await;
            let results: Vec<WorkspaceEntry> = memories.into_iter()
                .filter_map(|m| {
                    // Find matching entry by content similarity
                    entries.iter()
                        .find(|e| m.content.contains(&e.content[..50.min(e.content.len())]))
                        .cloned()
                })
                .collect();
            
            Ok(results)
        } else {
            // Fallback to simple text search
            let entries = self.entries.read().await;
            let query_lower = query.to_lowercase();
            let results: Vec<WorkspaceEntry> = entries.iter()
                .filter(|e| e.content.to_lowercase().contains(&query_lower))
                .take(limit)
                .cloned()
                .collect();
            Ok(results)
        }
    }

    /// Get a summary of what other workers are doing
    pub async fn get_worker_awareness_summary(&self) -> String {
        let jobs = self.job_registry.list_active_jobs();
        let objectives = self.worker_objectives.read().await;
        let entries = self.entries.read().await;
        
        let mut summary = String::from("## Active Workers\n\n");
        
        for job in &jobs {
            let obj = objectives.get(&job.id)
                .map(|s| s.as_str())
                .unwrap_or("Unknown objective");
            
            summary.push_str(&format!("- **Worker-{}**: {}\n", 
                &job.id[..8.min(job.id.len())], 
                obj.lines().next().unwrap_or(obj)));
            
            // Add recent findings for this worker
            let worker_entries: Vec<_> = entries.iter()
                .filter(|e| e.job_id == job.id && e.entry_type == WorkspaceEntryType::Finding)
                .rev()
                .take(2)
                .collect();
            
            for entry in worker_entries {
                summary.push_str(&format!("  - {}\n", &entry.content[..80.min(entry.content.len())]));
            }
        }
        
        // Add completed workers count
        let all_jobs = self.job_registry.list_all_jobs();
        let completed = all_jobs.iter().filter(|j| j.status == JobStatus::Completed).count();
        let failed = all_jobs.iter().filter(|j| j.status == JobStatus::Failed).count();
        
        summary.push_str(&format!("\n## Summary\n- Active: {}\n- Completed: {}\n- Failed: {}\n",
            jobs.len(), completed, failed));
        
        summary
    }

    /// Get a comprehensive workspace summary
    pub async fn get_summary(&self) -> WorkspaceSummary {
        let entries = self.entries.read().await;
        let all_jobs = self.job_registry.list_all_jobs();
        
        let active_workers = all_jobs.iter().filter(|j| j.status == JobStatus::Running).count();
        let completed_workers = all_jobs.iter().filter(|j| j.status == JobStatus::Completed).count();
        let failed_workers = all_jobs.iter().filter(|j| j.status == JobStatus::Failed).count();
        
        let recent_findings: Vec<_> = entries.iter()
            .filter(|e| e.entry_type == WorkspaceEntryType::Finding || e.entry_type == WorkspaceEntryType::CompleteResult)
            .rev()
            .take(5)
            .cloned()
            .collect();
        
        WorkspaceSummary {
            total_entries: entries.len(),
            active_workers,
            completed_workers,
            failed_workers,
            recent_findings,
            last_updated: *self.last_updated.read().await,
        }
    }

    /// Clear the workspace (use with caution)
    pub async fn clear(&self) {
        let mut scratchpad = self.scratchpad.write().await;
        scratchpad.clear();
        
        let mut entries = self.entries.write().await;
        entries.clear();
        
        let mut objectives = self.worker_objectives.write().await;
        objectives.clear();
        
        crate::info_log!("[WORKSPACE] Workspace cleared");
    }

    /// Get workspace statistics for debugging
    pub async fn get_stats(&self) -> serde_json::Value {
        let entries = self.entries.read().await;
        let objectives = self.worker_objectives.read().await;
        let scratchpad = self.scratchpad.read().await;
        
        let by_type: HashMap<String, usize> = entries.iter()
            .fold(HashMap::new(), |mut acc, e| {
                *acc.entry(e.entry_type.to_string()).or_insert(0) += 1;
                acc
            });
        
        serde_json::json!({
            "total_entries": entries.len(),
            "scratchpad_size_bytes": scratchpad.len(),
            "registered_workers": objectives.len(),
            "by_type": by_type,
            "has_vector_store": self.vector_store.is_some(),
        })
    }
}

impl Clone for SharedWorkspace {
    fn clone(&self) -> Self {
        Self {
            scratchpad: self.scratchpad.clone(),
            entries: self.entries.clone(),
            job_registry: self.job_registry.clone(),
            vector_store: self.vector_store.clone(),
            worker_objectives: self.worker_objectives.clone(),
            last_updated: self.last_updated.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_workspace_entry_formatting() {
        let entry = WorkspaceEntry::new(
            "test-job-123".to_string(),
            "Worker-abc".to_string(),
            WorkspaceEntryType::Finding,
            "Found something important".to_string(),
        );
        
        let formatted = entry.format_for_scratchpad();
        assert!(formatted.contains("🔍"));
        assert!(formatted.contains("Worker-abc"));
        assert!(formatted.contains("Found something important"));
    }

    #[tokio::test]
    async fn test_workspace_add_and_retrieve() {
        let registry = JobRegistry::new();
        let workspace = SharedWorkspace::new(registry);
        
        workspace.register_worker("job-1", "Test objective").await;
        workspace.add_finding("job-1", "Found key insight", None).await;
        
        let entries = workspace.get_entries_by_job("job-1").await;
        assert_eq!(entries.len(), 2); // objective + finding
        
        let scratchpad = workspace.get_scratchpad().await;
        assert!(scratchpad.contains("Test objective"));
        assert!(scratchpad.contains("Found key insight"));
    }
}
