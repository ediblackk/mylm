//! Chunk pool for managing persistent file chunk workers
//!
//! The ChunkPool maintains active chunk workers for large files,
//! allowing follow-up queries without re-reading and re-processing.
//! Workers persist until the session ends.

use super::types::{ChunkSummary, FileChunk, ReadError};
use crate::agent::types::events::WorkerId;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Maximum retry attempts for a failed chunk worker
const MAX_CHUNK_RETRIES: u32 = 3;

/// A query sent to a chunk worker
#[derive(Debug, Clone)]
pub struct ChunkQuery {
    /// The question or query about the chunk content
    pub question: String,
    /// Response channel
    pub response_tx: mpsc::Sender<ChunkQueryResponse>,
}

/// Response from a chunk worker query
#[derive(Debug, Clone)]
pub struct ChunkQueryResponse {
    /// Whether the chunk contains relevant information
    pub is_relevant: bool,
    /// Answer or relevant excerpt from the chunk
    pub answer: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
}

/// An active chunk worker
#[derive(Debug)]
pub struct ActiveChunk {
    /// Chunk identifier
    pub chunk_id: usize,
    /// Worker ID from the runtime
    pub worker_id: WorkerId,
    /// Line range this chunk covers
    pub line_range: (usize, usize),
    /// Summary of chunk content
    pub summary: String,
    /// Key terms extracted from chunk
    pub key_terms: Vec<String>,
    /// Content hash for cache validation
    pub content_hash: String,
    /// Channel to send queries to this worker
    pub query_tx: mpsc::Sender<ChunkQuery>,
}

impl Clone for ActiveChunk {
    fn clone(&self) -> Self {
        Self {
            chunk_id: self.chunk_id,
            worker_id: self.worker_id.clone(),
            line_range: self.line_range,
            summary: self.summary.clone(),
            key_terms: self.key_terms.clone(),
            content_hash: self.content_hash.clone(),
            query_tx: self.query_tx.clone(),
        }
    }
}

/// Pool of active chunk workers
/// 
/// The ChunkPool manages workers for large files that have been read
/// using the chunked strategy. Workers remain active until the session ends.
#[derive(Debug)]
pub struct ChunkPool {
    /// Session identifier for this pool
    session_id: String,
    /// Active chunks organized by file path
    chunks: Arc<RwLock<HashMap<PathBuf, Vec<ActiveChunk>>>>,
    /// Maximum number of persistent workers allowed
    max_workers: usize,
    /// Current worker count
    worker_count: Arc<RwLock<usize>>,
}

impl ChunkPool {
    /// Create a new chunk pool for a session
    pub fn new(session_id: impl Into<String>, max_workers: usize) -> Self {
        Self {
            session_id: session_id.into(),
            chunks: Arc::new(RwLock::new(HashMap::new())),
            max_workers: max_workers.max(1).min(50),
            worker_count: Arc::new(RwLock::new(0)),
        }
    }
    
    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    
    /// Check if we can spawn more workers
    pub async fn can_spawn(&self) -> bool {
        let count = *self.worker_count.read().await;
        count < self.max_workers
    }
    
    /// Get current worker count
    pub async fn worker_count(&self) -> usize {
        *self.worker_count.read().await
    }
    
    /// Register a chunk for a file
    /// 
    /// This is called when a chunk worker is successfully spawned
    pub async fn register_chunk(
        &self,
        file_path: PathBuf,
        chunk: ActiveChunk,
    ) -> Result<(), ReadError> {
        let mut chunks = self.chunks.write().await;
        let mut count = self.worker_count.write().await;
        
        if *count >= self.max_workers {
            return Err(ReadError::InvalidArgument(
                format!("Maximum persistent workers ({}) reached", self.max_workers)
            ));
        }
        
        chunks.entry(file_path).or_default().push(chunk);
        *count += 1;
        
        Ok(())
    }
    
    /// Get all active chunks for a file
    pub async fn get_file_chunks(&self, path: &PathBuf) -> Vec<ActiveChunk> {
        let chunks = self.chunks.read().await;
        chunks.get(path).map(|v| v.clone()).unwrap_or_default()
    }
    
    /// Find chunks that might contain information about a query
    /// 
    /// Uses key terms matching to find relevant chunks
    pub async fn find_relevant_chunks(
        &self,
        path: &PathBuf,
        query: &str,
    ) -> Vec<usize> {
        let chunks = self.chunks.read().await;
        let file_chunks = match chunks.get(path) {
            Some(c) => c,
            None => return vec![],
        };
        
        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
        
        file_chunks
            .iter()
            .filter_map(|chunk| {
                // Check if any key term matches
                let matches = chunk.key_terms.iter().any(|term| {
                    let term_lower = term.to_lowercase();
                    query_terms.iter().any(|q| term_lower.contains(q) || q.contains(&term_lower))
                });
                
                // Also check summary
                let summary_matches = query_terms.iter().any(|q| {
                    chunk.summary.to_lowercase().contains(q)
                });
                
                if matches || summary_matches {
                    Some(chunk.chunk_id)
                } else {
                    None
                }
            })
            .collect()
    }
    
    /// Query a specific chunk
    pub async fn query_chunk(
        &self,
        path: &PathBuf,
        chunk_id: usize,
        question: String,
    ) -> Option<ChunkQueryResponse> {
        let chunks = self.chunks.read().await;
        let file_chunks = chunks.get(path)?;
        
        let chunk = file_chunks.iter().find(|c| c.chunk_id == chunk_id)?;
        
        let (tx, mut rx) = mpsc::channel(1);
        let query = ChunkQuery {
            question,
            response_tx: tx,
        };
        
        // Send query to worker
        if chunk.query_tx.send(query).await.is_err() {
            return None;
        }
        
        // Wait for response with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            rx.recv()
        ).await {
            Ok(Some(response)) => Some(response),
            _ => None,
        }
    }
    
    /// List all files with active chunks
    pub async fn list_active_files(&self) -> Vec<PathBuf> {
        let chunks = self.chunks.read().await;
        chunks.keys().cloned().collect()
    }
    
    /// Get chunk IDs for a file
    pub async fn list_chunks_for_file(&self, path: &PathBuf) -> Vec<usize> {
        let chunks = self.chunks.read().await;
        chunks
            .get(path)
            .map(|c| c.iter().map(|chunk| chunk.chunk_id).collect())
            .unwrap_or_default()
    }
    
    /// Remove all chunks for a file
    pub async fn remove_file(&self, path: &PathBuf) {
        let mut chunks = self.chunks.write().await;
        let mut count = self.worker_count.write().await;
        
        if let Some(file_chunks) = chunks.remove(path) {
            *count = count.saturating_sub(file_chunks.len());
            
            // Workers will be dropped and should terminate
            // In a full implementation, we'd send a shutdown signal
        }
    }
    
    /// Clear all chunks (called on session end)
    pub async fn clear(&self) {
        let mut chunks = self.chunks.write().await;
        let mut count = self.worker_count.write().await;
        
        chunks.clear();
        *count = 0;
    }
    
    /// Spawn chunk workers for a file
    /// 
    /// This is a placeholder for the actual worker spawning logic.
    /// The actual implementation will integrate with the DelegateTool.
    pub async fn spawn_chunks(
        &self,
        _file_path: &PathBuf,
        _chunks: Vec<FileChunk>,
        _content: &str,
    ) -> Result<Vec<ChunkSummary>, ReadError> {
        // TODO: Integrate with DelegateTool to spawn actual workers
        // For now, return mock summaries
        
        let summaries: Vec<ChunkSummary> = _chunks
            .iter()
            .map(|chunk| ChunkSummary {
                chunk_id: chunk.id,
                line_range: (chunk.line_start, chunk.line_end),
                summary: format!("Lines {}-{}", chunk.line_start, chunk.line_end),
                key_terms: vec![],
                content_hash: String::new(),
            })
            .collect();
        
        Ok(summaries)
    }
    
    /// Retry a failed chunk worker
    /// 
    /// Returns true if retry was successful
    pub async fn retry_chunk(
        &self,
        _file_path: &PathBuf,
        _chunk: &FileChunk,
        _attempt: u32,
    ) -> Result<ChunkSummary, ReadError> {
        if _attempt >= MAX_CHUNK_RETRIES {
            return Err(ReadError::ChunkWorkerFailed {
                chunk_id: _chunk.id,
                error: format!("Failed after {} retries", MAX_CHUNK_RETRIES),
            });
        }
        
        // TODO: Implement actual retry logic with DelegateTool
        // For now, simulate success after a delay
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        
        Ok(ChunkSummary {
            chunk_id: _chunk.id,
            line_range: (_chunk.line_start, _chunk.line_end),
            summary: format!("Lines {}-{} (retry {})", _chunk.line_start, _chunk.line_end, _attempt),
            key_terms: vec![],
            content_hash: String::new(),
        })
    }
}

impl Clone for ChunkPool {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            chunks: Arc::clone(&self.chunks),
            max_workers: self.max_workers,
            worker_count: Arc::clone(&self.worker_count),
        }
    }
}

/// Worker handle for managing a chunk worker's lifecycle
#[derive(Debug)]
#[cfg(test)]
pub struct ChunkWorkerHandle {
    /// Worker ID
    pub worker_id: WorkerId,
    /// Chunk being processed
    pub chunk: FileChunk,
    /// Shutdown signal
    shutdown_tx: mpsc::Sender<()>,
}


#[cfg(test)]
impl ChunkWorkerHandle {
    /// Create a new worker handle
    pub fn new(worker_id: WorkerId, chunk: FileChunk) -> (Self, mpsc::Receiver<()>) {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        
        let handle = Self {
            worker_id,
            chunk,
            shutdown_tx,
        };
        
        (handle, shutdown_rx)
    }
    
    /// Signal the worker to shut down
    pub async fn shutdown(&self) -> Result<(), mpsc::error::SendError<()>> {
        self.shutdown_tx.send(()).await
    }
}

/// Builder for creating chunk workers
#[cfg(test)]
pub struct ChunkWorkerBuilder {
    file_path: PathBuf,
    chunk: FileChunk,
    content: String,
}


#[cfg(test)]
impl ChunkWorkerBuilder {
    /// Create a new worker builder
    pub fn new(file_path: PathBuf, chunk: FileChunk, content: String) -> Self {
        Self {
            file_path,
            chunk,
            content,
        }
    }
    
    /// Build the worker objective prompt
    pub fn build_objective(&self) -> String {
        format!(
            r#"You are analyzing a chunk of a large file.

File: {}
Chunk: {} (lines {}-{})

Your content:
```
{}
```

Your tasks:
1. Analyze this chunk and provide a concise summary (2-3 sentences)
2. Extract 5-10 key terms or identifiers from this chunk
3. Be ready to answer specific questions about this content

Respond in this JSON format:
{{
  "summary": "brief summary of content",
  "key_terms": ["term1", "term2", "term3"]
}}"#,
            self.file_path.display(),
            self.chunk.id,
            self.chunk.line_start,
            self.chunk.line_end,
            self.content
        )
    }
    
    /// Get the chunk
    pub fn chunk(&self) -> &FileChunk {
        &self.chunk
    }
    
    /// Get the file path
    pub fn file_path(&self) -> &PathBuf {
        &self.file_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_chunk_pool_creation() {
        let pool = ChunkPool::new("test-session", 5);
        assert_eq!(pool.session_id(), "test-session");
        assert!(pool.can_spawn().await);
        assert_eq!(pool.worker_count().await, 0);
    }
    
    #[tokio::test]
    async fn test_chunk_registration() {
        let pool = ChunkPool::new("test-session", 2);
        let path = PathBuf::from("/test/file.txt");
        
        let (tx, _rx) = mpsc::channel(10);
        let chunk = ActiveChunk {
            chunk_id: 0,
            worker_id: WorkerId(1),
            line_range: (1, 100),
            summary: "Test summary".to_string(),
            key_terms: vec!["test".to_string()],
            content_hash: "abc123".to_string(),
            query_tx: tx,
        };
        
        pool.register_chunk(path.clone(), chunk).await.unwrap();
        assert_eq!(pool.worker_count().await, 1);
        
        // Test max workers limit
        let (tx2, _rx2) = mpsc::channel(10);
        let chunk2 = ActiveChunk {
            chunk_id: 1,
            worker_id: WorkerId(2),
            line_range: (101, 200),
            summary: "Test summary 2".to_string(),
            key_terms: vec!["test2".to_string()],
            content_hash: "def456".to_string(),
            query_tx: tx2,
        };
        
        pool.register_chunk(path.clone(), chunk2).await.unwrap();
        assert_eq!(pool.worker_count().await, 2);
        
        // Should fail - max workers reached
        let (tx3, _rx3) = mpsc::channel(10);
        let chunk3 = ActiveChunk {
            chunk_id: 2,
            worker_id: WorkerId(3),
            line_range: (201, 300),
            summary: "Test summary 3".to_string(),
            key_terms: vec!["test3".to_string()],
            content_hash: "ghi789".to_string(),
            query_tx: tx3,
        };
        
        assert!(pool.register_chunk(path.clone(), chunk3).await.is_err());
    }
    
    #[tokio::test]
    async fn test_find_relevant_chunks() {
        let pool = ChunkPool::new("test-session", 5);
        let path = PathBuf::from("/test/file.txt");
        
        let (tx, _rx) = mpsc::channel(10);
        let chunk = ActiveChunk {
            chunk_id: 0,
            worker_id: WorkerId(1),
            line_range: (1, 100),
            summary: "Functions for error handling".to_string(),
            key_terms: vec!["error".to_string(), "Result".to_string(), "panic".to_string()],
            content_hash: "abc123".to_string(),
            query_tx: tx,
        };
        
        pool.register_chunk(path.clone(), chunk).await.unwrap();
        
        // Find by key term
        let relevant = pool.find_relevant_chunks(&path, "error handling").await;
        assert!(relevant.contains(&0));
        
        // Find by summary content
        let relevant = pool.find_relevant_chunks(&path, "functions").await;
        assert!(relevant.contains(&0));
        
        // No match
        let relevant = pool.find_relevant_chunks(&path, "database").await;
        assert!(!relevant.contains(&0));
    }
    
    #[tokio::test]
    async fn test_worker_count_limits() {
        let pool = ChunkPool::new("test-session", 5);
        assert!(pool.can_spawn().await);
        
        let pool_full = ChunkPool::new("test-session", 0);
        // Should be clamped to minimum of 1
        assert!(pool_full.can_spawn().await);
        
        let pool_large = ChunkPool::new("test-session", 100);
        // Should be clamped to maximum of 50
        // We can't directly test this, but we can verify it accepts workers
        assert!(pool_large.can_spawn().await);
    }
    
    #[test]
    fn test_chunk_worker_builder() {
        let chunk = FileChunk::new(0, 1, 100, 4000);
        let builder = ChunkWorkerBuilder::new(
            PathBuf::from("/test.rs"),
            chunk,
            "fn main() {}".to_string(),
        );
        
        let objective = builder.build_objective();
        assert!(objective.contains("/test.rs"));
        assert!(objective.contains("lines 1-100"));
        assert!(objective.contains("fn main()"));
        assert!(objective.contains("JSON format"));
    }
    
    #[tokio::test]
    async fn test_chunk_pool_clear() {
        let pool = ChunkPool::new("test-session", 5);
        let path = PathBuf::from("/test/file.txt");
        
        let (tx, _rx) = mpsc::channel(10);
        let chunk = ActiveChunk {
            chunk_id: 0,
            worker_id: WorkerId(1),
            line_range: (1, 100),
            summary: "Test".to_string(),
            key_terms: vec![],
            content_hash: "abc".to_string(),
            query_tx: tx,
        };
        
        pool.register_chunk(path.clone(), chunk).await.unwrap();
        assert_eq!(pool.worker_count().await, 1);
        
        pool.clear().await;
        assert_eq!(pool.worker_count().await, 0);
        assert!(pool.list_active_files().await.is_empty());
    }
}
