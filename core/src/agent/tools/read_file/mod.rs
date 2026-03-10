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
mod docx;
mod csv;
mod search;

pub use types::*;
pub use chunker::{ChunkedReader, read_line_range, TokenChunkConfig, TokenChunkResult, compute_chunks_with_tokens, get_file_stats};
pub use pdf::{extract_text as extract_pdf_text, extract_pages as extract_pdf_pages, get_pdf_info, pdf_text_to_lines};
pub use docx::{extract_text as extract_docx_text, get_docx_info};
pub use csv::{extract_text as extract_csv_text, get_csv_info, read_row_range};
pub use types::FileFormat;
pub use search::{SearchFilesTool, SearchResult};

use crate::agent::runtime::core::{Capability, RuntimeContext, ToolCapability, ToolError};
use crate::agent::types::events::ToolResult;
use crate::agent::types::intents::ToolCall;
use crate::agent::tools::expand_tilde;
use crate::provider::LlmClient;
use crate::provider::chat::{ChatMessage, ChatRequest};
use std::path::Path;
use std::sync::Arc;
use std::hash::{DefaultHasher, Hash, Hasher};

/// Tool for reading files with intelligent chunking and search
pub struct ReadFileTool {
    /// Optional search tool for large files
    search_tool: Option<Arc<SearchFilesTool>>,
    /// Maximum file size for direct read (bytes)
    max_direct_size: usize,
    /// Cache for extracted PDF text (path -> content)
    pdf_cache: Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// Optional LLM client for chunk analysis
    llm_client: Option<Arc<crate::provider::LlmClient>>,
}

impl ReadFileTool {
    /// Create a new read file tool
    /// 
    /// # Arguments
    /// * `search_tool` - Optional Tantivy search tool
    pub fn new(
        search_tool: Option<Arc<SearchFilesTool>>,
    ) -> Self {
        Self {
            search_tool,
            max_direct_size: thresholds::MAX_DIRECT,
            pdf_cache: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            llm_client: None,
        }
    }
    
    /// Set the LLM client for chunk analysis
    pub fn with_llm_client(mut self, client: Arc<crate::provider::LlmClient>) -> Self {
        self.llm_client = Some(client);
        self
    }
    
    /// Set the LLM client optionally (for builder pattern)
    pub fn with_llm_client_opt(mut self, client: Option<Arc<crate::provider::LlmClient>>) -> Self {
        self.llm_client = client;
        self
    }
    
    /// Create a tool without chunking support (simple mode)
    pub fn simple() -> Self {
        Self {
            search_tool: None,
            max_direct_size: thresholds::MAX_DIRECT,
            pdf_cache: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            llm_client: None,
        }
    }
    
    /// Set maximum direct read size
    pub fn with_max_direct_size(mut self, size: usize) -> Self {
        self.max_direct_size = size;
        self
    }
    
    /// Main execution logic
    async fn execute_read(&self, mut args: ReadArgs) -> Result<ToolResult, ToolError> {
        crate::info_log!(
            "[ReadFile] === EXECUTE CALLED === path={}, strategy={:?}, line_offset={:?}, n_lines={:?}",
            args.path,
            args.strategy,
            args.line_offset,
            args.n_lines
        );
        
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
        
        // Get preliminary file size to determine strategy
        let preliminary_size = tokio::fs::metadata(path).await
            .map(|m| m.len() as usize)
            .unwrap_or(0);
            
        // Determine strategy
        let mut strategy = args.strategy.unwrap_or_else(|| {
            chunker::determine_strategy(preliminary_size)
        });
        
        // If the agent is requesting specific lines (like chunk workers do), force Direct strategy
        if args.line_offset.is_some() || args.n_lines.is_some() {
            strategy = ReadStrategy::Direct;
        }
        
        // For direct reading, handle files requiring special extraction (PDF, DOCX, CSV)
        if strategy == ReadStrategy::Direct {
            match format {
                FileFormat::Pdf => {
                    return self.read_pdf(&path_buf, args).await.map_err(|e| ToolError::new(e.to_string()));
                }
                FileFormat::Docx => {
                    return self.read_docx(&path_buf, args).await.map_err(|e| ToolError::new(e.to_string()));
                }
                FileFormat::Csv => {
                    return self.read_csv(&path_buf, args).await.map_err(|e| ToolError::new(e.to_string()));
                }
                _ => {} // Continue with text-based handling
            }
        }
        
        // Get precise file stats (line count) for chunking
        let path_str = path.to_string_lossy().to_string();
        let (file_size, total_lines) = match format {
            FileFormat::Pdf => {
                // For PDFs with chunked strategy: skip line count extraction here
                // read_chunked will extract once and use content-based chunking
                if strategy == ReadStrategy::Chunked {
                    crate::info_log!("[ReadFile] PDF with chunked strategy: skipping line count, will extract in read_chunked");
                    // Estimate based on ~18000 lines per MB (typical for dense text files)
                    let estimated_lines = (preliminary_size / 56).max(1);
                    (preliminary_size, estimated_lines)
                } else {
                    // Check cache first to avoid double extraction
                    let cache = self.pdf_cache.lock().await;
                    let text = if let Some(cached) = cache.get(&path_str) {
                        crate::info_log!("[ReadFile] Using cached PDF for line count: {}", path_str);
                        cached.clone()
                    } else {
                        drop(cache);
                        crate::info_log!("[ReadFile] Extracting PDF for line count: {}", path_str);
                        let extracted = pdf::extract_text(path).await.map_err(|e| ToolError::new(e.to_string()))?;
                        let mut cache = self.pdf_cache.lock().await;
                        cache.insert(path_str.clone(), extracted.clone());
                        extracted
                    };
                    (preliminary_size, text.lines().count().max(1))
                }
            }
            FileFormat::Docx => {
                let text = docx::extract_text(path).await.map_err(|e| ToolError::new(e.to_string()))?;
                (preliminary_size, text.lines().count().max(1))
            }
            FileFormat::Csv => {
                let info = csv::get_csv_info(path).await.map_err(|e| ToolError::new(e.to_string()))?;
                (preliminary_size, info.row_count.max(1))
            }
            _ => {
                match chunker::get_file_stats(path).await {
                    Ok(stats) => (stats.0, stats.1.max(1)),
                    Err(e) => return Ok(self.error_result(e, "STATS_ERROR")),
                }
            }
        };
        
        // Execute based on strategy
        let result = match strategy {
            ReadStrategy::Auto => unreachable!(), // Always resolved above
            ReadStrategy::Direct => self.read_direct(path, args.line_offset, args.n_lines, file_size, total_lines).await,
            ReadStrategy::Search => self.read_with_search(path, args.query.as_deref(), file_size, total_lines).await,
            ReadStrategy::Chunked => self.read_chunked(path, file_size, total_lines).await,
        };
        
        match result {
            Ok(tool_result) => Ok(tool_result),
            Err(e) => {
                crate::error_log!("[ReadFile] Execution failed: {:?}", e);
                Ok(self.error_result(e, "READ_ERROR"))
            }
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
        // Check if file is too large for direct read, UNLESS we're doing a partial read
        let is_partial_read = line_offset.is_some() || n_lines.is_some();
        if !is_partial_read && file_size > self.max_direct_size {
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
                "File is {} bytes ({} estimated tokens). Consider using query_file tool for large files.",
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
    
    /// Extract chunk content from full file text
    fn extract_chunk_content(content: &str, line_start: usize, line_end: usize) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let start_idx = (line_start.saturating_sub(1)).min(lines.len());
        let end_idx = line_end.min(lines.len());
        lines[start_idx..end_idx].join("\n")
    }
    
    /// Analyze a chunk using direct LLM call
    async fn analyze_chunk_with_llm(
        client: &Arc<LlmClient>,
        chunk_id: usize,
        line_start: usize,
        line_end: usize,
        content: &str,
    ) -> Result<(String, Vec<String>), String> {
        // Truncate content if too large (max ~4000 tokens for analysis)
        let max_chars = 16000; // ~4000 tokens
        let truncated_content = if content.len() > max_chars {
            format!("{}... [truncated]", &content[..max_chars])
        } else {
            content.to_string()
        };
        
        let prompt = format!(
            r#"You are analyzing a section of a document (Chunk {}).

Lines {}-{}

CONTENT:
```
{}
```

Your task:
1. Provide a concise summary (2-3 sentences) of what this section contains
2. Extract 5-10 key terms or identifiers from this section

Respond ONLY with a JSON object in this exact format:
{{"summary": "brief summary here", "key_terms": ["term1", "term2", "term3", "term4", "term5"]}}"#,
            chunk_id, line_start, line_end, truncated_content
        );
        
        // Create simple LLM request
        let messages = vec![
            ChatMessage::system("You are a document analysis assistant. Respond only with valid JSON."),
            ChatMessage::user(prompt),
        ];
        let request = ChatRequest::new(client.model().to_string(), messages)
            .with_temperature(0.3)
            .with_max_tokens(500)
            .with_json_mode();
        
        // Call LLM
        match client.chat(&request).await {
            Ok(response) => {
                let text = response.content();
                Self::parse_analysis_response(&text)
            }
            Err(e) => Err(format!("LLM call failed: {}", e)),
        }
    }
    
    /// Parse LLM analysis response
    fn parse_analysis_response(response: &str) -> Result<(String, Vec<String>), String> {
        // Try to find JSON in the response
        let json_start = response.find('{');
        let json_end = response.rfind('}');
        
        if let (Some(start), Some(end)) = (json_start, json_end) {
            if end > start {
                let json_str = &response[start..=end];
                match serde_json::from_str::<serde_json::Value>(json_str) {
                    Ok(json) => {
                        let summary = json.get("summary")
                            .and_then(|s| s.as_str())
                            .unwrap_or("Analysis completed")
                            .to_string();
                        let key_terms = json.get("key_terms")
                            .and_then(|k| k.as_array())
                            .map(|arr| arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .take(10)
                                .collect())
                            .unwrap_or_default();
                        return Ok((summary, key_terms));
                    }
                    Err(e) => {
                        crate::warn_log!("[ReadFile] Failed to parse analysis JSON: {}", e);
                    }
                }
            }
        }
        
        // Fallback: return raw response as summary
        Ok((response.lines().next().unwrap_or("Analysis").to_string(), vec![]))
    }
    
    /// Read file using chunking
    /// 
    /// NOTE: For persistent chunk workers with follow-up query support,
    /// use the query_file tool instead. This method provides one-time
    /// chunk analysis without persistent workers.
    async fn read_chunked(
        &self,
        path: &Path,
        file_size: usize,
        total_lines: usize,
    ) -> Result<ToolResult, ReadError> {
        let _path_buf = path.to_path_buf();
        
        // Create chunks using token-aware chunking
        // Use a default worker context window for chunk sizing
        let worker_context = 8192; // Default 8K context
        
        // Determine optimal chunk configuration based on file size and worker context
        let chunk_config = chunker::determine_chunk_config(file_size, worker_context);
        
        // Compute chunks with token awareness
        let chunk_result = chunker::compute_chunks_with_tokens(
            file_size,
            total_lines,
            &chunk_config,
        );
        
        // Document processing summary - consolidated log format
        let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "unknown".to_string());
        let total_tokens = tokens::estimate_from_bytes(file_size);
        crate::info_log!(
            "[Document] Processing: {} ({} bytes, ~{} tokens, {} lines)",
            file_name, file_size, total_tokens, total_lines
        );
        crate::info_log!(
            "[Document] Chunked: {} chunks ({} tokens/chunk, {} overlap)",
            chunk_result.chunks.len(),
            chunk_config.effective_chunk_size(),
            chunk_config.overlap_tokens
        );
        
        // Log each chunk details
        for (i, chunk) in chunk_result.chunks.iter().enumerate() {
            let chunk_tokens = chunk.estimated_tokens();
            crate::info_log!(
                "[Document] Chunk {}: lines {}-{} (~{} tokens)",
                i, chunk.line_start, chunk.line_end, chunk_tokens
            );
        }
        
        // Read file content for chunk analysis
        let file_content = tokio::fs::read_to_string(path).await
            .map_err(|e| ReadError::ReadError(format!("Failed to read file: {}", e)))?;
        
        // Analyze chunks using direct LLM calls (simplified architecture)
        let llm_client = self.llm_client.clone();
        let chunk_summaries: Vec<ChunkSummary> = if let Some(client) = llm_client {
            
            // Create analysis tasks for each chunk
            let total_chunks = chunk_result.chunks.len();
            let mut tasks = Vec::new();
            for chunk in &chunk_result.chunks {
                let chunk_content = Self::extract_chunk_content(&file_content, chunk.line_start, chunk.line_end);
                let client = Arc::clone(&client);
                let chunk_id = chunk.id;
                let line_start = chunk.line_start;
                let line_end = chunk.line_end;
                let total = total_chunks;
                
                let task = tokio::spawn(async move {
                    let chunk_tokens = tokens::estimate_from_content(&chunk_content);
                    crate::info_log!("[Worker:{}] Context: {} tokens | Processing chunk {}/{} (lines {}-{})", 
                        chunk_id, chunk_tokens, chunk_id + 1, total, line_start, line_end);
                    let start = std::time::Instant::now();
                    
                    let result = Self::analyze_chunk_with_llm(&client, chunk_id, line_start, line_end, &chunk_content).await;
                    
                    let elapsed = start.elapsed().as_secs_f32();
                    match &result {
                        Ok((summary, terms)) => {
                            let truncated_summary = if summary.len() > 120 { &summary[..120] } else { summary };
                            crate::info_log!("[Worker:{}] Completed in {:.1}s | {} terms | {}", 
                                chunk_id, elapsed, terms.len(), truncated_summary);
                        }
                        Err(e) => {
                            crate::warn_log!("[Worker:{}] Analysis failed: {}", chunk_id, e);
                        }
                    }
                    result
                });
                tasks.push((chunk.id, task));
            }
            
            // Collect all results with timeout
            let mut chunk_summaries = Vec::new();
            let timeout = tokio::time::Duration::from_secs(30); // 30s timeout per chunk
            
            for (chunk_id, task) in tasks {
                match tokio::time::timeout(timeout, task).await {
                    Ok(Ok(Ok((summary, key_terms)))) => {
                        let chunk = chunk_result.chunks.iter().find(|c| c.id == chunk_id).unwrap();
                        chunk_summaries.push(ChunkSummary {
                            chunk_id,
                            line_range: (chunk.line_start, chunk.line_end),
                            summary,
                            key_terms,
                            content_hash: {
                                let mut hasher = DefaultHasher::new();
                                format!("{}:{}-{}", path.display(), chunk.line_start, chunk.line_end).hash(&mut hasher);
                                format!("{:016x}", hasher.finish())
                            },
                        });
                    }
                    Ok(Ok(Err(e))) => {
                        crate::warn_log!("[ReadFile] Chunk {} analysis error: {}", chunk_id, e);
                        let chunk = chunk_result.chunks.iter().find(|c| c.id == chunk_id).unwrap();
                        chunk_summaries.push(ChunkSummary {
                            chunk_id,
                            line_range: (chunk.line_start, chunk.line_end),
                            summary: format!("Lines {}-{} (analysis failed)", chunk.line_start, chunk.line_end),
                            key_terms: vec![],
                            content_hash: String::new(),
                        });
                    }
                    Ok(Err(e)) => {
                        crate::warn_log!("[ReadFile] Chunk {} task panicked: {}", chunk_id, e);
                        let chunk = chunk_result.chunks.iter().find(|c| c.id == chunk_id).unwrap();
                        chunk_summaries.push(ChunkSummary {
                            chunk_id,
                            line_range: (chunk.line_start, chunk.line_end),
                            summary: format!("Lines {}-{} (task error)", chunk.line_start, chunk.line_end),
                            key_terms: vec![],
                            content_hash: String::new(),
                        });
                    }
                    Err(_) => {
                        crate::warn_log!("[ReadFile] Chunk {} analysis timed out", chunk_id);
                        let chunk = chunk_result.chunks.iter().find(|c| c.id == chunk_id).unwrap();
                        chunk_summaries.push(ChunkSummary {
                            chunk_id,
                            line_range: (chunk.line_start, chunk.line_end),
                            summary: format!("Lines {}-{} (timeout)", chunk.line_start, chunk.line_end),
                            key_terms: vec![],
                            content_hash: String::new(),
                        });
                    }
                }
            }
            
            chunk_summaries
        } else {
            // No LLM client available - return placeholder summaries
            crate::warn_log!("[ReadFile] No LLM client available for chunk analysis, returning placeholders");
            chunk_result.chunks.iter().map(|chunk| ChunkSummary {
                chunk_id: chunk.id,
                line_range: (chunk.line_start, chunk.line_end),
                summary: format!("Lines {}-{} ({} chars)", chunk.line_start, chunk.line_end, 
                    chunk.line_end.saturating_sub(chunk.line_start) * 40), // Rough estimate
                key_terms: vec![],
                content_hash: {
                    use std::hash::Hash;
                    let mut hasher = DefaultHasher::new();
                    let hash_input = format!("{}:{}-{}", path.display(), chunk.line_start, chunk.line_end);
                    hash_input.hash(&mut hasher);
                    format!("{:016x}", hasher.finish())
                },
            }).collect()
        };
        
        // Build synthesized output
        let mut metadata = ReadMetadata::new(path.to_string_lossy(), file_size)
            .with_strategy(ReadStrategy::Chunked)
            .with_tokens(tokens::estimate_from_bytes(file_size));
        
        metadata.chunk_summaries = chunk_summaries;
        
        // Calculate stats
        let total_chunk_lines: usize = metadata.chunk_summaries.iter()
            .map(|s| s.line_range.1 - s.line_range.0 + 1)
            .sum();
        let completed_chunks = metadata.chunk_summaries.iter()
            .filter(|s| !s.summary.contains("analyzing..."))
            .count();
        
        let output = format!(
            "File analyzed using chunked strategy.\n\n\
            File: {}\n\
            Size: {} bytes\n\
            Total lines: {}\n\
            Estimated tokens: {}\n\
            Chunks analyzed: {}/{}\n\
            Coverage: {} lines ({:.0}%)\n\n\
            Chunk Summaries ({} completed):\n{}\n\n\
            NOTE: For follow-up queries with persistent workers, use the query_file tool.\n\
            Example: query_file with file_path='{}' and prompt='your question'",
            path.display(),
            file_size,
            total_lines,
            tokens::estimate_from_bytes(file_size),
            completed_chunks,
            metadata.chunk_summaries.len(),
            total_chunk_lines,
            (total_chunk_lines as f32 / total_lines.max(1) as f32) * 100.0,
            completed_chunks,
            metadata.chunk_summaries.iter()
                .map(|s| {
                    let status = if s.summary.contains("analyzing...") || s.summary.contains("failed") || s.summary.contains("timeout") { "⚠" } else { "✓" };
                    let line_count = s.line_range.1 - s.line_range.0 + 1;
                    format!("  {} [{}] Lines {}-{} ({} lines): {}", 
                        status,
                        s.chunk_id, 
                        s.line_range.0, 
                        s.line_range.1,
                        line_count,
                        if s.summary.len() > 100 { format!("{}...", &s.summary[..100]) } else { s.summary.clone() }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            path.display()
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
        crate::info_log!("[ReadFile] Extracting PDF text from: {}", path.display());
        
        // Extract text from PDF
        let text = pdf::extract_text(path).await.map_err(|e| {
            crate::error_log!("[ReadFile] PDF extraction failed for {}: {}", path.display(), e);
            e
        })?;
        
        crate::info_log!("[ReadFile] PDF extracted successfully: {} characters", text.len());
        
        // Get PDF info
        let info = pdf::get_pdf_info(path).await?;
        let file_size = tokio::fs::metadata(path).await
            .map(|m| m.len() as usize)
            .unwrap_or(0);
        
        let lines_per_page = 40;
        let text = pdf::pdf_text_to_lines(&text, Some(info.page_count.unwrap_or_else(|| {
             (text.lines().count() + lines_per_page - 1) / lines_per_page
        })));
        
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
    
    /// Read DOCX file
    async fn read_docx(
        &self,
        path: &Path,
        args: ReadArgs,
    ) -> Result<ToolResult, ReadError> {
        // Extract text from DOCX
        let text = docx::extract_text(path).await?;
        
        // Get file info
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
        
        metadata.total_lines = Some(content.lines().count());
        
        let output = self.format_output(&content, &metadata);
        
        Ok(ToolResult::Success {
            output,
            structured: Some(serde_json::to_value(metadata).unwrap_or_default()),
        })
    }
    
    /// Read CSV file
    async fn read_csv(
        &self,
        path: &Path,
        args: ReadArgs,
    ) -> Result<ToolResult, ReadError> {
        // Get file info
        let file_size = tokio::fs::metadata(path).await
            .map(|m| m.len() as usize)
            .unwrap_or(0);
        
        // For CSV, we use row-based reading
        let content = if args.line_offset.is_some() || args.n_lines.is_some() {
            let start_row = args.line_offset.unwrap_or(1);
            let end_row = args.n_lines.map(|n| start_row + n - 1);
            csv::read_row_range(path, start_row, end_row).await?
        } else {
            csv::extract_text(path).await?
        };
        
        let mut metadata = ReadMetadata::new(path.to_string_lossy(), file_size)
            .with_strategy(ReadStrategy::Direct)
            .with_tokens(tokens::estimate_from_content(&content));
        
        // Get CSV info for additional metadata
        if let Ok(csv_info) = csv::get_csv_info(path).await {
            metadata.total_lines = Some(csv_info.row_count);
            metadata.warnings.push(format!(
                "CSV file: {} rows, {} columns",
                csv_info.row_count,
                csv_info.column_count
            ));
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

impl ReadFileTool {
    /// Get tool description for LLM prompt
    pub fn description() -> &'static str {
        r#"# read_file - Read file contents

Read file contents with support for partial reads and automatic chunking for large files.
It natively extracts text from PDF, DOCX, and CSV files, so DO NOT use external shell tools to convert them!

## Strategies

### Direct (default for files < 100KB)
Reads the entire file content directly.
```json
{"a": "read_file", "i": {"path": "document.pdf"}}
```

### Partial Read (AVOID for PDFs > 100KB)
Read specific line ranges. Only use for text files, NOT for large PDFs.
```json
{"a": "read_file", "i": {"path": "src/main.rs", "line_offset": 1, "n_lines": 50}}
```

### Chunked (REQUIRED for large files > 100KB, especially PDFs)
Automatically splits large files into chunks and analyzes each chunk.
This is the ONLY correct way to read large PDFs - do NOT use partial reads with line_offset on PDFs!
```json
{"a": "read_file", "i": {"path": "large_document.pdf", "strategy": "chunked"}}
```

## CRITICAL: Large PDF Workflow

For PDFs larger than 100KB (books, reports, documents):

1. **SINGLE call with chunked strategy** - This is the ONLY step:
   ```json
   {"a": "read_file", "i": {"path": "book.pdf", "strategy": "chunked"}}
   ```
   - The tool extracts the PDF ONCE, splits into chunks
   - Analyzes each chunk (may take 30-60 seconds)
   - WAIT for the response showing chunk summaries

2. **DO NOT attempt partial reads** - Never do this after chunked:
   ```json
   {"a": "read_file", "i": {"path": "book.pdf", "line_offset": 1, "n_lines": 1000}}  // WRONG!
   ```

3. **For follow-up queries with persistent workers, use query_file tool**:
   ```json
   {"a": "query_file", "i": {"file_path": "book.pdf", "prompt": "What are the main themes?"}}
   ```

## Document Workflow

1. **Initial read**: Use `strategy: "chunked"` - returns summaries
2. **Check results**: Response shows "✓" for completed chunks with summaries
3. **For interactive queries**: Use `query_file` tool to spawn persistent chunk workers

## When to use chunked strategy

- Files larger than 100KB (especially PDFs, books, reports)
- When you need to understand/analyze large documents
- The ONLY way to read large PDFs properly

The chunked strategy extracts PDF text once and analyzes chunks.
For persistent workers with follow-up query support, use the query_file tool."#
    }
}

#[async_trait::async_trait]
impl ToolCapability for ReadFileTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse arguments first to get the path for logging
        let path_for_log = if let Some(path) = call.arguments.as_str() {
            path.to_string()
        } else if let Some(path) = call.arguments.get("path").and_then(|p| p.as_str()) {
            path.to_string()
        } else {
            "unknown".to_string()
        };
        
        let strategy = call.arguments.get("strategy").and_then(|s| s.as_str()).unwrap_or("auto");
        crate::info_log!("[ReadFile] Called: path='{}', strategy='{}'", path_for_log, strategy);
        
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
        
        // Large files should use chunked strategy
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
