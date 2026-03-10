//! Types for the read_file tool
//!
//! Defines argument structures, strategies, and result types for
//! intelligent file reading with support for partial reads, chunking,
//! and search-based access.

use serde::{Deserialize, Serialize};

/// Arguments for the read_file tool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReadArgs {
    /// Path to the file to read
    pub path: String,
    
    /// Line offset to start reading from (1-based, inclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_offset: Option<usize>,
    
    /// Maximum number of lines to read (max 1000)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_lines: Option<usize>,
    
    /// Reading strategy to use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<ReadStrategy>,
    
    /// Search query for search-based reading
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

impl ReadArgs {
    /// Create a simple read request for a path
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            line_offset: None,
            n_lines: None,
            strategy: None,
            query: None,
        }
    }
    
    /// Set line offset (1-based)
    pub fn with_line_offset(mut self, offset: usize) -> Self {
        self.line_offset = Some(offset.max(1));
        self
    }
    
    /// Set number of lines to read (max 10000, validated later)
    pub fn with_n_lines(mut self, n: usize) -> Self {
        self.n_lines = Some(n);
        self
    }
    
    /// Set reading strategy
    pub fn with_strategy(mut self, strategy: ReadStrategy) -> Self {
        self.strategy = Some(strategy);
        self
    }
    
    /// Set search query
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }
    
    /// Validate arguments
    pub fn validate(&self) -> Result<(), ReadError> {
        if self.path.is_empty() {
            return Err(ReadError::InvalidArgument("path cannot be empty".to_string()));
        }
        
        if let Some(n) = self.n_lines {
            if n == 0 {
                return Err(ReadError::InvalidArgument("n_lines must be greater than 0".to_string()));
            }
            if n > thresholds::MAX_LINES {
                return Err(ReadError::InvalidArgument(format!("n_lines cannot exceed {}", thresholds::MAX_LINES)));
            }
        }
        
        // Search strategy requires a query
        if matches!(self.strategy, Some(ReadStrategy::Search)) && self.query.is_none() {
            return Err(ReadError::InvalidArgument(
                "search strategy requires a query parameter".to_string()
            ));
        }
        
        Ok(())
    }
}

/// Reading strategy for file access
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadStrategy {
    /// Automatically choose strategy based on file size
    Auto,
    
    /// Read file directly (fails if too large)
    Direct,
    
    /// Read file in chunks using worker delegation
    Chunked,
    
    /// Use search index to find relevant sections
    Search,
}

impl Default for ReadStrategy {
    fn default() -> Self {
        Self::Auto
    }
}

/// File format detection result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    /// Plain text file
    Text,
    
    /// Markdown file
    Markdown,
    
    /// CSV file
    Csv,
    
    /// XML file
    Xml,
    
    /// PDF document
    Pdf,
    
    /// Microsoft Word document
    Docx,
    
    /// Unknown or binary format
    Unknown,
}

impl FileFormat {
    /// Detect format from file extension
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "txt" | "rs" | "js" | "ts" | "py" | "java" | "go" | "c" | "cpp" | "h" | "hpp" |
            "json" | "yaml" | "yml" | "toml" | "ini" | "conf" | "config" |
            "sh" | "bash" | "zsh" | "fish" |
            "html" | "htm" | "css" | "scss" | "sass" | "less" |
            "sql" | "graphql" | "proto" |
            "log" | "md"  => Self::Text,
            "markdown" => Self::Markdown,
            "csv" => Self::Csv,
            "xml" | "xhtml" | "svg" => Self::Xml,
            "pdf" => Self::Pdf,
            "docx" | "doc" => Self::Docx,
            _ => Self::Unknown,
        }
    }
    
    /// Check if format supports line-based chunking
    pub fn supports_chunking(&self) -> bool {
        matches!(self, Self::Text | Self::Markdown | Self::Csv | Self::Xml)
    }
    
    /// Check if format requires special extraction
    pub fn requires_extraction(&self) -> bool {
        matches!(self, Self::Pdf | Self::Docx)
    }
    
    /// Get human-readable name for the format
    pub fn name(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Csv => "csv",
            Self::Xml => "xml",
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Unknown => "unknown",
        }
    }
}

/// A chunk of a file defined by line range
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    /// Chunk identifier (0-based index)
    pub id: usize,
    
    /// Starting line number (1-based, inclusive)
    pub line_start: usize,
    
    /// Ending line number (1-based, inclusive)
    pub line_end: usize,
    
    /// Approximate byte size of this chunk
    pub byte_size: usize,
}

impl FileChunk {
    /// Create a new file chunk
    pub fn new(id: usize, line_start: usize, line_end: usize, byte_size: usize) -> Self {
        Self {
            id,
            line_start,
            line_end,
            byte_size,
        }
    }
    
    /// Get the number of lines in this chunk
    pub fn line_count(&self) -> usize {
        self.line_end.saturating_sub(self.line_start) + 1
    }
    
    /// Estimate token count (approximate: bytes / 4)
    pub fn estimated_tokens(&self) -> usize {
        self.byte_size / 4
    }
}

/// Summary of a chunk after processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkSummary {
    /// Chunk identifier
    pub chunk_id: usize,
    
    /// Line range covered
    pub line_range: (usize, usize),
    
    /// LLM-generated summary of the chunk
    pub summary: String,
    
    /// Key terms extracted from the chunk
    pub key_terms: Vec<String>,
    
    /// Content hash for cache invalidation
    pub content_hash: String,
}

/// Result metadata for read operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadMetadata {
    /// File path
    pub path: String,
    
    /// File size in bytes
    pub file_size: usize,
    
    /// Total number of lines in file (if known)
    pub total_lines: Option<usize>,
    
    /// Strategy that was used
    pub strategy_used: ReadStrategy,
    
    /// Line range that was read (for partial reads)
    pub line_range: Option<(usize, usize)>,
    
    /// Token estimate for the content
    pub estimated_tokens: usize,
    
    /// Whether the content was truncated
    pub was_truncated: bool,
    
    /// Warning messages
    pub warnings: Vec<String>,
    
    /// Chunk summaries (for chunked reading)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub chunk_summaries: Vec<ChunkSummary>,
}

impl ReadMetadata {
    /// Create new metadata
    pub fn new(path: impl Into<String>, file_size: usize) -> Self {
        Self {
            path: path.into(),
            file_size,
            total_lines: None,
            strategy_used: ReadStrategy::Auto,
            line_range: None,
            estimated_tokens: 0,
            was_truncated: false,
            warnings: Vec::new(),
            chunk_summaries: Vec::new(),
        }
    }
    
    /// Add a warning
    pub fn warn(mut self, message: impl Into<String>) -> Self {
        self.warnings.push(message.into());
        self
    }
    
    /// Set the strategy used
    pub fn with_strategy(mut self, strategy: ReadStrategy) -> Self {
        self.strategy_used = strategy;
        self
    }
    
    /// Set line range
    pub fn with_line_range(mut self, start: usize, end: usize) -> Self {
        self.line_range = Some((start, end));
        self
    }
    
    /// Set token estimate
    pub fn with_tokens(mut self, tokens: usize) -> Self {
        self.estimated_tokens = tokens;
        self
    }
}

/// Error types for read operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReadError {
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    
    #[error("File not found: {0}")]
    FileNotFound(String),
    
    #[error("Path is a directory: {0}")]
    IsDirectory(String),
    
    #[error("File too large for direct read: {size} bytes (max {max})")]
    FileTooLarge { size: usize, max: usize },
    
    #[error("Access error: {0}")]
    AccessError(String),
    
    #[error("Read error: {0}")]
    ReadError(String),
    
    #[error("PDF extraction error: {0}")]
    PdfExtractionError(String),
    
    #[error("PDF is encrypted and cannot be read")]
    PdfEncrypted,
    
    #[error("Chunk worker failed after retries: chunk {chunk_id}, error: {error}")]
    ChunkWorkerFailed { chunk_id: usize, error: String },
    
    #[error("Search index not available")]
    IndexUnavailable,
    
    #[error("IO error: {0}")]
    Io(String),
}

/// Size thresholds for reading strategies (in bytes)
pub mod thresholds {
    /// Small file limit: ~50K tokens - direct read
    pub const SMALL_FILE: usize = 200_000;
    
    /// Medium file limit: ~125K tokens - direct with warning
    pub const MEDIUM_FILE: usize = 500_000;
    
    /// Chunk size target: ~12.5K tokens per worker
    pub const CHUNK_SIZE: usize = 50_000;
    
    /// Large file threshold: use search strategy
    pub const LARGE_FILE: usize = 1_000_000;
    
    /// Maximum direct read size (with warning)
    pub const MAX_DIRECT: usize = 500_000;
    
    /// Maximum lines per partial read request
    pub const MAX_LINES: usize = 10_000;
}

/// Token estimation utilities
pub mod tokens {
    /// Estimate token count from byte size
    /// 
    /// Uses the approximation: 1 token ≈ 4 characters (bytes)
    /// This is a rough estimate that works for English text and code.
    pub fn estimate_from_bytes(bytes: usize) -> usize {
        bytes / 4
    }
    
    /// Estimate token count from string content
    pub fn estimate_from_content(content: &str) -> usize {
        estimate_from_bytes(content.len())
    }
    
    /// Check if content would exceed token budget
    pub fn would_exceed_budget(content: &str, max_tokens: usize) -> bool {
        estimate_from_content(content) > max_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_read_args_validation() {
        let args = ReadArgs::new("test.txt");
        assert!(args.validate().is_ok());
        
        let args = ReadArgs::new("");
        assert!(args.validate().is_err());
        
        let args = ReadArgs::new("test.txt").with_n_lines(1001);
        assert!(args.validate().is_err());
        
        let args = ReadArgs::new("test.txt").with_n_lines(0);
        assert!(args.validate().is_err());
        
        let args = ReadArgs::new("test.txt")
            .with_strategy(ReadStrategy::Search);
        assert!(args.validate().is_err()); // Missing query
        
        let args = ReadArgs::new("test.txt")
            .with_strategy(ReadStrategy::Search)
            .with_query("function main");
        assert!(args.validate().is_ok());
    }
    
    #[test]
    fn test_file_format_detection() {
        assert_eq!(FileFormat::from_extension("rs"), FileFormat::Text);
        assert_eq!(FileFormat::from_extension("md"), FileFormat::Text);
        assert_eq!(FileFormat::from_extension("markdown"), FileFormat::Markdown);
        assert_eq!(FileFormat::from_extension("pdf"), FileFormat::Pdf);
        assert_eq!(FileFormat::from_extension("bin"), FileFormat::Unknown);
    }
    
    #[test]
    fn test_file_chunk() {
        let chunk = FileChunk::new(0, 1, 100, 4000);
        assert_eq!(chunk.line_count(), 100);
        assert_eq!(chunk.estimated_tokens(), 1000);
    }
    
    #[test]
    fn test_token_estimation() {
        assert_eq!(tokens::estimate_from_bytes(4000), 1000);
        assert_eq!(tokens::estimate_from_content("hello world"), 2); // 11 bytes / 4 = 2
    }
    
    #[test]
    fn test_read_metadata_builder() {
        let meta = ReadMetadata::new("test.txt", 1000)
            .with_strategy(ReadStrategy::Direct)
            .with_line_range(1, 50)
            .with_tokens(250)
            .warn("Large file");
        
        assert_eq!(meta.strategy_used, ReadStrategy::Direct);
        assert_eq!(meta.line_range, Some((1, 50)));
        assert_eq!(meta.estimated_tokens, 250);
        assert_eq!(meta.warnings.len(), 1);
    }
}
