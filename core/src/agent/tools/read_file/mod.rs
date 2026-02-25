//! Read File Tool - Intelligent file reading with chunking and search
//!
//! This module provides an enhanced file reading tool that can:
//! - Read files partially using line offsets
//! - Automatically chunk large files for parallel processing
//! - Integrate with Tantivy for search-based access
//! - Extract text from PDFs
//! - Maintain persistent chunk workers for follow-up queries
//!
//! # Usage
//!
//! Direct read (small files):
//! ```json
//! {"a": "read_file", "i": {"path": "src/main.rs"}}
//! ```
//!
//! Partial read:
//! ```json
//! {"a": "read_file", "i": {"path": "src/main.rs", "line_offset": 1, "n_lines": 50}}
//! ```
//!
//! Chunked read (large files):
//! ```json
//! {"a": "read_file", "i": {"path": "large.log", "strategy": "chunked"}}
//! ```
//!
//! Search-based read:
//! ```json
//! {"a": "read_file", "i": {"path": "huge.txt", "strategy": "search", "query": "error handling"}}
//! ```

mod types;
mod chunker;
mod pdf;
mod pool;
mod search;

pub use types::*;
pub use chunker::{ChunkedReader, read_line_range};
pub use pdf::{extract_text, get_pdf_info};
pub use types::FileFormat;
pub use pool::{ChunkPool, ChunkQuery, ChunkQueryResponse, ActiveChunk};
pub use search::{SearchFilesTool, SearchResult};

use crate::agent::runtime::core::{Capability, RuntimeContext, ToolCapability, ToolError};
use crate::agent::types::events::ToolResult;
use crate::agent::types::intents::ToolCall;
use crate::agent::tools::expand_tilde;
use std::path::Path;
use std::sync::Arc;

/// Tool for reading files with intelligent chunking and search
pub struct ReadFileTool {
    /// Chunk pool for managing persistent workers
    chunk_pool: Arc<ChunkPool>,
    /// Optional search tool for large files
    search_tool: Option<Arc<SearchFilesTool>>,
    /// Maximum file size for direct read (bytes)
    max_direct_size: usize,
}

impl ReadFileTool {
    /// Create a new read file tool
    /// 
    /// # Arguments
    /// * `chunk_pool` - Pool for managing chunk workers
    /// * `search_tool` - Optional Tantivy search tool
    pub fn new(
        chunk_pool: Arc<ChunkPool>,
        search_tool: Option<Arc<SearchFilesTool>>,
    ) -> Self {
        Self {
            chunk_pool,
            search_tool,
            max_direct_size: thresholds::MAX_DIRECT,
        }
    }
    
    /// Create a tool without chunking support (simple mode)
    pub fn simple() -> Self {
        let pool = Arc::new(ChunkPool::new("simple", 1));
        Self {
            chunk_pool: pool,
            search_tool: None,
            max_direct_size: thresholds::MAX_DIRECT,
        }
    }
    
    /// Set maximum direct read size
    pub fn with_max_direct_size(mut self, size: usize) -> Self {
        self.max_direct_size = size;
        self
    }
    
    /// Main execution logic
    async fn execute_read(&self, mut args: ReadArgs) -> Result<ToolResult, ToolError> {
        // Expand tilde (~) to home directory
        args.path = expand_tilde(&args.path);
        
        // Validate arguments
        if let Err(e) = args.validate() {
            return Ok(ToolResult::Error {
                message: e.to_string(),
                code: Some("INVALID_ARGUMENT".to_string()),
                retryable: false,
            });
        }
        
        let path = Path::new(&args.path);
        let path_buf = path.to_path_buf();
        
        // Check file accessibility and format
        let format = match chunker::check_file_readable(path).await {
            Ok(f) => f,
            Err(e) => return Ok(self.error_result(e, "ACCESS_ERROR")),
        };
        
        // Handle PDF files
        if format == FileFormat::Pdf {
            return self.read_pdf(&path_buf, args).await.map_err(|e| ToolError::new(e.to_string()));
        }
        
        // Get file stats
        let (file_size, total_lines) = match chunker::get_file_stats(path).await {
            Ok(stats) => stats,
            Err(e) => return Ok(self.error_result(e, "STATS_ERROR")),
        };
        
        // Determine strategy
        let strategy = args.strategy.unwrap_or_else(|| {
            chunker::determine_strategy(file_size)
        });
        
        // Execute based on strategy
        let result = match strategy {
            ReadStrategy::Auto => unreachable!(), // Always resolved above
            ReadStrategy::Direct => {
                self.read_direct(path, args.line_offset, args.n_lines, file_size, total_lines).await
            }
            ReadStrategy::Chunked => {
                self.read_chunked(path, file_size, total_lines).await
            }
            ReadStrategy::Search => {
                self.read_with_search(path, args.query.as_deref(), file_size, total_lines).await
            }
        };
        
        match result {
            Ok(tool_result) => Ok(tool_result),
            Err(e) => Ok(self.error_result(e, "READ_ERROR")),
        }
    }
    
    /// Read file directly
    async fn read_direct(
        &self,
        path: &Path,
        line_offset: Option<usize>,
        n_lines: Option<usize>,
        file_size: usize,
        total_lines: usize,
    ) -> Result<ToolResult, ReadError> {
        // Check if file is too large for direct read
        if file_size > self.max_direct_size {
            return Err(ReadError::FileTooLarge {
                size: file_size,
                max: self.max_direct_size,
            });
        }
        
        // Determine line range
        let start_line = line_offset.unwrap_or(1);
        let end_line = n_lines.map(|n| (start_line + n - 1).min(total_lines));
        
        // Read content
        let content = if line_offset.is_some() || n_lines.is_some() {
            chunker::read_line_range(path, start_line, end_line).await?
        } else {
            tokio::fs::read_to_string(path).await
                .map_err(|e| ReadError::ReadError(e.to_string()))?
        };
        
        // Build metadata
        let actual_end = end_line.unwrap_or(total_lines);
        let mut metadata = ReadMetadata::new(path.to_string_lossy(), file_size)
            .with_strategy(ReadStrategy::Direct)
            .with_tokens(tokens::estimate_from_content(&content));
        
        if line_offset.is_some() || n_lines.is_some() {
            metadata.line_range = Some((start_line, actual_end));
        }
        
        // Add warning for medium-sized files
        if file_size > thresholds::SMALL_FILE {
            metadata.warnings.push(format!(
                "File is {} bytes ({} estimated tokens). Consider using chunked strategy for large files.",
                file_size,
                tokens::estimate_from_bytes(file_size)
            ));
        }
        
        let output = self.format_output(&content, &metadata);
        
        Ok(ToolResult::Success {
            output,
            structured: Some(serde_json::to_value(metadata).unwrap_or_default()),
        })
    }
    
    /// Read file using chunking
    async fn read_chunked(
        &self,
        path: &Path,
        file_size: usize,
        total_lines: usize,
    ) -> Result<ToolResult, ReadError> {
        let path_buf = path.to_path_buf();
        
        // Check if we already have chunks for this file
        let existing_chunks = self.chunk_pool.list_chunks_for_file(&path_buf).await;
        let summaries = if !existing_chunks.is_empty() {
            // Return existing chunk summaries
            self.chunk_pool.get_file_chunks(&path_buf).await
                .into_iter()
                .map(|c| ChunkSummary {
                    chunk_id: c.chunk_id,
                    line_range: c.line_range,
                    summary: c.summary,
                    key_terms: c.key_terms,
                    content_hash: c.content_hash,
                })
                .collect()
        } else if self.chunk_pool.can_spawn().await {
            // Create new chunks
            let chunks = chunker::compute_chunks(file_size, total_lines);
            self.chunk_pool.spawn_chunks(&path_buf, chunks, "").await?
        } else {
            // Cannot spawn workers, fall back to direct read with warning
            let content = chunker::read_line_range(path, 1, Some(1000)).await?;
            let mut metadata = ReadMetadata::new(path.to_string_lossy(), file_size)
                .with_strategy(ReadStrategy::Direct)
                .with_line_range(1, content.lines().count())
                .with_tokens(tokens::estimate_from_content(&content));
            
            metadata.warnings.push(
                "Could not spawn chunk workers (limit reached). Showing first 1000 lines only.".to_string()
            );
            
            let output = self.format_output(&content, &metadata);
            return Ok(ToolResult::Success {
                output,
                structured: Some(serde_json::to_value(metadata).unwrap_or_default()),
            });
        };
        
        // Build synthesized output
        let mut metadata = ReadMetadata::new(path.to_string_lossy(), file_size)
            .with_strategy(ReadStrategy::Chunked)
            .with_tokens(tokens::estimate_from_bytes(file_size));
        
        metadata.chunk_summaries = summaries;
        
        let output = format!(
            "File analyzed using chunked strategy.\n\n\
            File: {} ({} bytes, ~{} lines, ~{} tokens)\n\
            Chunks: {}\n\n\
            Chunk Summaries:\n{}\n\n\
            Use follow-up queries to ask specific questions about file content.",
            path.display(),
            file_size,
            total_lines,
            tokens::estimate_from_bytes(file_size),
            metadata.chunk_summaries.len(),
            metadata.chunk_summaries.iter()
                .map(|s| format!("  [{}] Lines {}-{}: {}", 
                    s.chunk_id, 
                    s.line_range.0, 
                    s.line_range.1, 
                    s.summary
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
        
        Ok(ToolResult::Success {
            output,
            structured: Some(serde_json::to_value(metadata).unwrap_or_default()),
        })
    }
    
    /// Read file using search
    async fn read_with_search(
        &self,
        path: &Path,
        query: Option<&str>,
        file_size: usize,
        total_lines: usize,
    ) -> Result<ToolResult, ReadError> {
        // Check if search tool is available
        if self.search_tool.is_none() || !self.search_tool.as_ref().unwrap().is_available() {
            // Fall back to chunked strategy
            return self.read_chunked(path, file_size, total_lines).await;
        }
        
        let search_tool = self.search_tool.as_ref().unwrap();
        let search_query = query.unwrap_or("");
        
        // Search for relevant sections
        let results = search_tool.search(search_query, Some(path.to_string_lossy().as_ref())).await
            .map_err(|_e| ReadError::IndexUnavailable)?;
        
        if results.is_empty() {
            return Ok(ToolResult::Success {
                output: format!(
                    "No results found for query '{}' in {}.\n\
                    Try a different query or use chunked strategy to browse the file.",
                    search_query,
                    path.display()
                ),
                structured: None,
            });
        }
        
        // Read relevant sections
        let mut content_parts = Vec::new();
        for result in &results {
            let section = chunker::read_line_range(
                path,
                result.line_start,
                Some(result.line_end),
            ).await?;
            content_parts.push(format!(
                "<!-- Lines {}-{} (score: {:.2}) -->\n{}",
                result.line_start,
                result.line_end,
                result.score,
                section
            ));
        }
        
        let full_content = content_parts.join("\n\n");
        
        let metadata = ReadMetadata::new(path.to_string_lossy(), file_size)
            .with_strategy(ReadStrategy::Search)
            .with_tokens(tokens::estimate_from_content(&full_content));
        
        let output = self.format_output(&full_content, &metadata);
        
        Ok(ToolResult::Success {
            output,
            structured: Some(serde_json::to_value(metadata).unwrap_or_default()),
        })
    }
    
    /// Read PDF file
    async fn read_pdf(
        &self,
        path: &Path,
        args: ReadArgs,
    ) -> Result<ToolResult, ReadError> {
        // Extract text from PDF
        let text = pdf::extract_text(path).await?;
        
        // Get PDF info
        let info = pdf::get_pdf_info(path).await?;
        let file_size = tokio::fs::metadata(path).await
            .map(|m| m.len() as usize)
            .unwrap_or(0);
        
        // Handle partial reads if specified
        let content = if args.line_offset.is_some() || args.n_lines.is_some() {
            let lines: Vec<&str> = text.lines().collect();
            let start = args.line_offset.unwrap_or(1).saturating_sub(1);
            let end = args.n_lines.map(|n| start + n).unwrap_or(lines.len()).min(lines.len());
            lines[start..end].join("\n")
        } else {
            text
        };
        
        let mut metadata = ReadMetadata::new(path.to_string_lossy(), file_size)
            .with_strategy(ReadStrategy::Direct)
            .with_tokens(tokens::estimate_from_content(&content));
        
        if let Some(pages) = info.page_count {
            metadata.total_lines = Some(pages); // Using pages as line proxy
        }
        
        if info.is_encrypted {
            metadata.warnings.push("PDF is encrypted (text extraction may be limited)".to_string());
        }
        
        let output = self.format_output(&content, &metadata);
        
        Ok(ToolResult::Success {
            output,
            structured: Some(serde_json::to_value(metadata).unwrap_or_default()),
        })
    }
    
    /// Format output with metadata
    fn format_output(&self, content: &str, metadata: &ReadMetadata) -> String {
        let mut output = String::new();
        
        // Add file header
        output.push_str(&format!(
            "<!-- File: {} ({} bytes) -->\n",
            metadata.path,
            metadata.file_size
        ));
        
        if let Some((start, end)) = metadata.line_range {
            output.push_str(&format!("<!-- Lines: {}-{} -->\n", start, end));
        }
        
        output.push_str(&format!(
            "<!-- Estimated tokens: {} -->\n",
            metadata.estimated_tokens
        ));
        
        if !metadata.warnings.is_empty() {
            output.push_str("<!-- Warnings:\n");
            for warning in &metadata.warnings {
                output.push_str(&format!("  - {}\n", warning));
            }
            output.push_str("-->\n");
        }
        
        output.push('\n');
        output.push_str(content);
        
        output
    }
    
    /// Create error ToolResult
    fn error_result(&self, error: ReadError, code: &str) -> ToolResult {
        ToolResult::Error {
            message: error.to_string(),
            code: Some(code.to_string()),
            retryable: matches!(error, ReadError::ChunkWorkerFailed { .. }),
        }
    }
}

impl Capability for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }
}

#[async_trait::async_trait]
impl ToolCapability for ReadFileTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse arguments
        let args = if let Some(path) = call.arguments.as_str() {
            // Simple string argument: just the path
            ReadArgs::new(path)
        } else {
            // Structured JSON argument
            match serde_json::from_value::<ReadArgs>(call.arguments) {
                Ok(a) => a,
                Err(e) => {
                    return Ok(ToolResult::Error {
                        message: format!("Invalid arguments: {}", e),
                        code: Some("PARSE_ERROR".to_string()),
                        retryable: false,
                    });
                }
            }
        };
        
        self.execute_read(args).await
    }
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::simple()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;
    
    #[tokio::test]
    async fn test_read_file_direct() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "line1\nline2\nline3\n").await.unwrap();
        
        let tool = ReadFileTool::simple();
        let call = ToolCall::new("read_file", serde_json::json!(path.to_str().unwrap()));
        
        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
        
        match result {
            ToolResult::Success { output, structured } => {
                assert!(output.contains("line1"));
                assert!(output.contains("line2"));
                assert!(structured.is_some());
            }
            _ => panic!("Expected success"),
        }
    }
    
    #[tokio::test]
    async fn test_read_file_partial() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").await.unwrap();
        
        let tool = ReadFileTool::simple();
        let call = ToolCall::new("read_file", serde_json::json!({
            "path": path.to_str().unwrap(),
            "line_offset": 2,
            "n_lines": 2
        }));
        
        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
        
        match result {
            ToolResult::Success { output, .. } => {
                assert!(output.contains("line2"));
                assert!(output.contains("line3"));
                assert!(!output.contains("line1")); // Should not include line 1
                assert!(!output.contains("line5")); // Should not include line 5
            }
            _ => panic!("Expected success"),
        }
    }
    
    #[tokio::test]
    async fn test_read_file_large_uses_chunked() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("large.txt");
        
        // Create a file larger than MAX_DIRECT
        let large_content = "x".repeat(thresholds::MAX_DIRECT + 1000);
        fs::write(&path, large_content).await.unwrap();
        
        let tool = ReadFileTool::simple();
        let call = ToolCall::new("read_file", serde_json::json!(path.to_str().unwrap()));
        
        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
        
        // Large files should use chunked strategy (simple mode has 1 worker)
        match result {
            ToolResult::Success { structured, .. } => {
                let metadata: ReadMetadata = serde_json::from_value(
                    structured.expect("should have metadata")
                ).unwrap();
                assert_eq!(metadata.strategy_used, ReadStrategy::Chunked);
            }
            _ => panic!("Expected success with chunked strategy"),
        }
    }
    
    #[tokio::test]
    async fn test_read_file_directory() {
        let temp = TempDir::new().unwrap();
        
        let tool = ReadFileTool::simple();
        let call = ToolCall::new("read_file", serde_json::json!(temp.path().to_str().unwrap()));
        
        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
        
        match result {
            ToolResult::Error { code, .. } => {
                assert_eq!(code, Some("ACCESS_ERROR".to_string()));
            }
            _ => panic!("Expected error for directory"),
        }
    }
    
    #[test]
    fn test_read_file_capability_name() {
        let tool = ReadFileTool::simple();
        assert_eq!(tool.name(), "read_file");
    }
}
