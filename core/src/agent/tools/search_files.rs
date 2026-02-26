//! Search Files Tool - Full-text search using Tantivy
//!
//! Provides fast full-text search across indexed files.
//! Files are automatically indexed when first read.

use crate::agent::runtime::core::{Capability, RuntimeContext, ToolCapability, ToolError};
use crate::agent::types::events::ToolResult;
use crate::agent::types::intents::ToolCall;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tantivy::{
    collector::TopDocs,
    directory::MmapDirectory,
    query::QueryParser,
    schema::{Schema, STORED, TEXT, FAST, Field, Value},
    Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument,
};
use tokio::sync::Mutex;

/// Maximum number of search results to return
const MAX_RESULTS: usize = 20;

/// Default memory budget for indexing (50MB)
const INDEX_MEMORY_BUDGET: usize = 50_000_000;

/// Search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// File path
    pub path: String,
    /// Starting line (1-based)
    pub line_start: usize,
    /// Ending line (1-based)
    pub line_end: usize,
    /// Text snippet
    pub snippet: String,
    /// Search relevance score
    pub score: f32,
}

/// Tool for searching files
pub struct SearchFilesTool {
    index: Arc<Index>,
    reader: Arc<IndexReader>,
    writer: Arc<Mutex<IndexWriter>>,
    _schema: Schema,  // Kept for potential future use
    path_field: Field,
    content_field: Field,
    line_start_field: Field,
    line_end_field: Field,
    modified_field: Field,
}

impl std::fmt::Debug for SearchFilesTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchFilesTool")
            .field("doc_count", &self.reader.searcher().num_docs())
            .finish()
    }
}

impl SearchFilesTool {
    /// Create a new search tool with an in-memory index
    /// 
    /// For persistent indexing, use `with_index_path()`.
    pub fn new() -> Result<Self, ToolError> {
        let schema = Self::build_schema();
        let path_field = schema.get_field("path").unwrap();
        let content_field = schema.get_field("content").unwrap();
        let line_start_field = schema.get_field("line_start").unwrap();
        let line_end_field = schema.get_field("line_end").unwrap();
        let modified_field = schema.get_field("modified").unwrap();
        
        // Create in-memory index
        let index = Index::create_in_ram(schema.clone());
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| ToolError::new(format!("Failed to create index reader: {:?}", e)))?;
        let writer = index
            .writer(INDEX_MEMORY_BUDGET)
            .map_err(|e| ToolError::new(format!("Failed to create index writer: {}", e)))?;
        
        Ok(Self {
            index: Arc::new(index),
            reader: Arc::new(reader),
            writer: Arc::new(Mutex::new(writer)),
            _schema: schema,
            path_field,
            content_field,
            line_start_field,
            line_end_field,
            modified_field,
        })
    }
    
    /// Create a new search tool with a persistent index
    /// 
    /// # Arguments
    /// * `index_path` - Directory to store the index
    pub fn with_index_path(index_path: impl AsRef<Path>) -> Result<Self, ToolError> {
        let schema = Self::build_schema();
        let path_field = schema.get_field("path").unwrap();
        let content_field = schema.get_field("content").unwrap();
        let line_start_field = schema.get_field("line_start").unwrap();
        let line_end_field = schema.get_field("line_end").unwrap();
        let modified_field = schema.get_field("modified").unwrap();
        
        let index_path = index_path.as_ref();
        
        // Create directory if it doesn't exist
        std::fs::create_dir_all(index_path)
            .map_err(|e| ToolError::new(format!("Failed to create index directory: {}", e)))?;
        
        let directory = MmapDirectory::open(index_path)
            .map_err(|e| ToolError::new(format!("Failed to open index directory: {}", e)))?;
        
        // Open or create index
        let index = Index::open_or_create(directory, schema.clone())
            .map_err(|e| ToolError::new(format!("Failed to open/create index: {}", e)))?;
        
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| ToolError::new(format!("Failed to create index reader: {:?}", e)))?;
        let writer = index
            .writer(INDEX_MEMORY_BUDGET)
            .map_err(|e| ToolError::new(format!("Failed to create index writer: {}", e)))?;
        
        Ok(Self {
            index: Arc::new(index),
            reader: Arc::new(reader),
            writer: Arc::new(Mutex::new(writer)),
            _schema: schema,
            path_field,
            content_field,
            line_start_field,
            line_end_field,
            modified_field,
        })
    }
    
    /// Build the Tantivy schema
    fn build_schema() -> Schema {
        let mut schema_builder = Schema::builder();
        
        // File path (stored for retrieval, indexed for filtering)
        schema_builder.add_text_field("path", TEXT | STORED | FAST);
        
        // Content (tokenized text)
        schema_builder.add_text_field("content", TEXT | STORED);
        
        // Line range (stored for reference)
        schema_builder.add_u64_field("line_start", FAST | STORED);
        schema_builder.add_u64_field("line_end", FAST | STORED);
        
        // Last modified timestamp (for cache invalidation)
        schema_builder.add_u64_field("modified", FAST | STORED);
        
        schema_builder.build()
    }
    
    /// Index a file or file chunk
    /// 
    /// # Arguments
    /// * `path` - File path
    /// * `content` - File content
    /// * `line_start` - Starting line (1-based)
    /// * `line_end` - Ending line (1-based)
    pub async fn index_file(
        &self,
        path: impl AsRef<Path>,
        content: &str,
        line_start: usize,
        line_end: usize,
    ) -> Result<(), ToolError> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        
        // Get file modification time
        let modified = tokio::fs::metadata(path.as_ref()).await
            .map(|m| m.modified().unwrap_or(std::time::UNIX_EPOCH))
            .unwrap_or(std::time::UNIX_EPOCH);
        let modified_secs = modified.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Create document
        let mut doc = TantivyDocument::default();
        doc.add_text(self.path_field, &path_str);
        doc.add_text(self.content_field, content);
        doc.add_u64(self.line_start_field, line_start as u64);
        doc.add_u64(self.line_end_field, line_end as u64);
        doc.add_u64(self.modified_field, modified_secs);
        
        // Add to index
        let mut writer = self.writer.lock().await;
        writer.add_document(doc)
            .map_err(|e| ToolError::new(format!("Failed to add document: {}", e)))?;
        
        // Commit to make documents searchable
        writer.commit()
            .map_err(|e| ToolError::new(format!("Failed to commit index: {}", e)))?;
        
        // Explicitly reload the reader to see new commits
        drop(writer); // Release lock before reload
        self.reader.reload()
            .map_err(|e| ToolError::new(format!("Failed to reload reader: {}", e)))?;
        
        Ok(())
    }
    
    /// Search for content
    /// 
    /// # Arguments
    /// * `query` - Search query
    /// * `path_filter` - Optional path filter (matches if path contains string)
    /// 
    /// # Returns
    /// Top search results sorted by relevance
    pub async fn search(
        &self,
        query: &str,
        path_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, ToolError> {
        let searcher = self.reader.searcher();
        
        // Build query parser
        let query_parser = QueryParser::for_index(&self.index, vec![self.content_field, self.path_field]);
        
        // Parse query
        let query = query_parser.parse_query(query)
            .map_err(|e| ToolError::new(format!("Failed to parse query: {}", e)))?;
        
        // Execute search
        let top_docs = searcher.search(&query, &TopDocs::with_limit(MAX_RESULTS))
            .map_err(|e| ToolError::new(format!("Search failed: {}", e)))?;
        
        let mut results = Vec::new();
        
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address)
                .map_err(|e| ToolError::new(format!("Failed to retrieve document: {}", e)))?;
            
            // Extract fields
            let path = doc.get_first(self.path_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            
            // Apply path filter if specified
            if let Some(filter) = path_filter {
                if !path.contains(filter) {
                    continue;
                }
            }
            
            let snippet = doc.get_first(self.content_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .lines()
                .take(3)  // First 3 lines as snippet
                .collect::<Vec<_>>()
                .join("\n");
            
            let line_start = doc.get_first(self.line_start_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as usize;
            
            let line_end = doc.get_first(self.line_end_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(line_start as u64) as usize;
            
            results.push(SearchResult {
                path,
                line_start,
                line_end,
                snippet,
                score,
            });
        }
        
        Ok(results)
    }
    
    /// Check if index is available
    pub fn is_available(&self) -> bool {
        true  // Always available once created
    }
    
    /// Get the number of indexed documents
    pub fn doc_count(&self) -> usize {
        self.reader.searcher().num_docs() as usize
    }
    
    /// Clear all documents from the index
    pub async fn clear(&self) -> Result<(), ToolError> {
        let mut writer = self.writer.lock().await;
        writer.delete_all_documents()
            .map_err(|e| ToolError::new(format!("Failed to clear index: {}", e)))?;
        writer.commit()
            .map_err(|e| ToolError::new(format!("Failed to commit clear: {}", e)))?;
        
        // Explicitly reload the reader
        drop(writer);
        self.reader.reload()
            .map_err(|e| ToolError::new(format!("Failed to reload reader: {}", e)))?;
        
        Ok(())
    }
}

impl Default for SearchFilesTool {
    fn default() -> Self {
        Self::new().expect("Failed to create default SearchFilesTool")
    }
}

impl Capability for SearchFilesTool {
    fn name(&self) -> &'static str {
        "search_files"
    }
}

/// Arguments for search_files tool
#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    path_filter: Option<String>,
}

#[async_trait::async_trait]
impl ToolCapability for SearchFilesTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse arguments
        let args = if let Some(query) = call.arguments.as_str() {
            SearchArgs {
                query: query.to_string(),
                path_filter: None,
            }
        } else {
            serde_json::from_value::<SearchArgs>(call.arguments)
                .map_err(|e| ToolError::new(format!("Invalid arguments: {}", e)))?
        };
        
        // Execute search
        let results = self.search(&args.query, args.path_filter.as_deref()).await?;
        
        // Format output
        if results.is_empty() {
            return Ok(ToolResult::Success {
                output: format!("No results found for query: {}", args.query),
                structured: Some(serde_json::json!({
                    "query": args.query,
                    "results": [],
                    "count": 0,
                })),
            });
        }
        
        let output = format!(
            "Found {} results for '{}':\n\n{}",
            results.len(),
            args.query,
            results.iter()
                .enumerate()
                .map(|(i, r)| format!(
                    "{}. {} (lines {}-{}, score: {:.2})\n{}",
                    i + 1,
                    r.path,
                    r.line_start,
                    r.line_end,
                    r.score,
                    r.snippet.lines()
                        .map(|l| format!("   > {}", l))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
                .collect::<Vec<_>>()
                .join("\n\n")
        );
        
        Ok(ToolResult::Success {
            output,
            structured: Some(serde_json::json!({
                "query": args.query,
                "results": results,
                "count": results.len(),
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_search_tool_creation() {
        let tool = SearchFilesTool::new();
        assert!(tool.is_ok());
    }
    
    #[tokio::test]
    async fn test_index_and_search() {
        let tool = SearchFilesTool::new().unwrap();
        
        // Index some content
        tool.index_file("test.rs", "fn main() {\n    println!(\"Hello\");\n}", 1, 3).await.unwrap();
        tool.index_file("lib.rs", "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}", 1, 2).await.unwrap();
        
        // Search for "main"
        let results = tool.search("main", None).await.unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.path == "test.rs"));
        
        // Search for "add"
        let results = tool.search("add", None).await.unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.path == "lib.rs"));
        
        // Search with no results
        let results = tool.search("nonexistent", None).await.unwrap();
        assert!(results.is_empty());
    }
    
    #[tokio::test]
    async fn test_search_with_path_filter() {
        let tool = SearchFilesTool::new().unwrap();
        
        tool.index_file("src/main.rs", "fn main() {}", 1, 1).await.unwrap();
        tool.index_file("tests/test.rs", "fn test() {}", 1, 1).await.unwrap();
        
        // Search with path filter
        let results = tool.search("fn", Some("src")).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/main.rs");
    }
    
    #[tokio::test]
    async fn test_persisted_index() {
        let temp = TempDir::new().unwrap();
        
        // Create tool with persisted index
        let tool = SearchFilesTool::with_index_path(temp.path()).unwrap();
        tool.index_file("file.txt", "Hello world", 1, 1).await.unwrap();
        
        // Verify search works
        let results = tool.search("Hello", None).await.unwrap();
        assert_eq!(results.len(), 1);
        
        // Verify doc count
        assert_eq!(tool.doc_count(), 1);
        
        // Note: Creating a new tool instance with the same path 
        // requires the IndexReader to reload. For now, we verify 
        // the first instance works correctly with persistence.
        // Testing multiple instances requires index reloading logic.
    }
    
    #[tokio::test]
    async fn test_clear_index() {
        let tool = SearchFilesTool::new().unwrap();
        tool.index_file("file.txt", "content", 1, 1).await.unwrap();
        
        assert_eq!(tool.doc_count(), 1);
        
        tool.clear().await.unwrap();
        
        assert_eq!(tool.doc_count(), 0);
        let results = tool.search("content", None).await.unwrap();
        assert!(results.is_empty());
    }
}
