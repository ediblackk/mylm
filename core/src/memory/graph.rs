use crate::memory::store::{Memory, VectorStore};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGraphNode {
    pub memory: Memory,
    pub connections: Vec<i64>, // IDs of connected memories
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryGraph {
    pub nodes: Vec<MemoryGraphNode>,
}

impl MemoryGraph {
    pub async fn generate_related_graph(store: &VectorStore, query: &str, limit: usize) -> Result<Self> {
        let memories = store.search_memory(query, limit).await?;
        let mut nodes = Vec::new();

        for (i, current) in memories.iter().enumerate() {
            let mut connections = Vec::new();
            
            // Heuristic: Connect if they share tags or keywords in content
            // For now, let's use a simple keyword overlap or just connect adjacent in search results (as a sequence)
            // Or better: connect if they share the same category_id
            for (j, other) in memories.iter().enumerate() {
                if i == j { continue; }
                
                let shared_category = current.category_id.is_some() 
                    && current.category_id == other.category_id;
                
                let current_words: HashSet<&str> = current.content.split_whitespace().collect();
                let other_words: HashSet<&str> = other.content.split_whitespace().collect();
                let overlap = current_words.intersection(&other_words).count();
                
                // Heuristic: Connect if shared category OR significant keyword overlap
                if shared_category || overlap > 3 {
                    connections.push(other.id);
                }
            }

            nodes.push(MemoryGraphNode {
                memory: current.clone(),
                connections,
            });
        }

        Ok(Self { nodes })
    }
}
