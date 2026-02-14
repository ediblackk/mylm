//! Vector Store Implementation
//!
//! In-memory vector store with cosine similarity search.
//! For production, replace with persistent store (Qdrant, Milvus, etc.)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Vector store entry
#[derive(Debug, Clone)]
pub struct VectorEntry {
    pub id: String,
    pub embedding: Vec<f32>,
    pub content: String,
    pub metadata: HashMap<String, String>,
}

/// In-memory vector store
pub struct InMemoryVectorStore {
    entries: Arc<RwLock<Vec<VectorEntry>>>,
}

impl InMemoryVectorStore {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    /// Store a vector with content
    pub async fn store(
        &self,
        id: impl Into<String>,
        embedding: Vec<f32>,
        content: impl Into<String>,
        metadata: HashMap<String, String>,
    ) {
        let entry = VectorEntry {
            id: id.into(),
            embedding,
            content: content.into(),
            metadata,
        };
        
        self.entries.write().await.push(entry);
    }
    
    /// Search by similarity (cosine similarity)
    pub async fn search(&self, query_embedding: &[f32], top_k: usize) -> Vec<SearchResult> {
        let entries = self.entries.read().await;
        
        if entries.is_empty() {
            return Vec::new();
        }
        
        // Calculate cosine similarity for each entry
        let mut scored: Vec<(f32, &VectorEntry)> = entries
            .iter()
            .map(|entry| {
                let similarity = cosine_similarity(query_embedding, &entry.embedding);
                (similarity, entry)
            })
            .collect();
        
        // Sort by similarity (descending)
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        
        // Return top_k results
        scored
            .into_iter()
            .take(top_k)
            .map(|(score, entry)| SearchResult {
                id: entry.id.clone(),
                content: entry.content.clone(),
                score,
                metadata: entry.metadata.clone(),
            })
            .collect()
    }
    
    /// Delete by ID
    pub async fn delete(&self, id: &str) -> bool {
        let mut entries = self.entries.write().await;
        let initial_len = entries.len();
        entries.retain(|e| e.id != id);
        entries.len() < initial_len
    }
    
    /// Get count of entries
    pub async fn count(&self) -> usize {
        self.entries.read().await.len()
    }
    
    /// Clear all entries
    pub async fn clear(&self) {
        self.entries.write().await.clear();
    }
}

impl Default for InMemoryVectorStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Search result
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub metadata: HashMap<String, String>,
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    
    dot_product / (norm_a * norm_b)
}

/// Simple embedding generator (for testing)
/// In production, use a real embedding model (OpenAI, local, etc.)
pub struct SimpleEmbedder;

impl SimpleEmbedder {
    /// Generate a simple embedding (bag of words style)
    /// This is NOT for production - use real embeddings!
    pub fn embed(text: &str) -> Vec<f32> {
        // Simple character frequency embedding
        // Just for demonstration - replace with real embeddings
        let mut vec = vec![0.0f32; 256];
        for byte in text.bytes() {
            vec[byte as usize] += 1.0;
        }
        // Normalize
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            vec.iter_mut().for_each(|x| *x /= norm);
        }
        vec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_vector_store() {
        let store = InMemoryVectorStore::new();
        
        // Store some vectors
        store.store(
            "doc1",
            vec![1.0, 0.0, 0.0],
            "First document",
            HashMap::new(),
        ).await;
        
        store.store(
            "doc2",
            vec![0.0, 1.0, 0.0],
            "Second document",
            HashMap::new(),
        ).await;
        
        store.store(
            "doc3",
            vec![0.9, 0.1, 0.0],
            "Third document similar to first",
            HashMap::new(),
        ).await;
        
        // Search
        let results = store.search(&[1.0, 0.0, 0.0], 2).await;
        
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "doc1"); // Exact match
        assert_eq!(results[1].id, "doc3"); // Similar
        
        // Check score is reasonable
        assert!(results[0].score > 0.99); // Close to 1.0 for exact match
        assert!(results[1].score > 0.5);  // Similar should be > 0.5
    }
    
    #[tokio::test]
    async fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0];
        let d = vec![0.707, 0.707, 0.0]; // 45 degrees
        
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
        assert!((cosine_similarity(&a, &d) - 0.707).abs() < 0.01);
    }
    
    #[tokio::test]
    async fn test_delete() {
        let store = InMemoryVectorStore::new();
        
        store.store("doc1", vec![1.0], "Content", HashMap::new()).await;
        assert_eq!(store.count().await, 1);
        
        assert!(store.delete("doc1").await);
        assert_eq!(store.count().await, 0);
        
        assert!(!store.delete("nonexistent").await);
    }
}
