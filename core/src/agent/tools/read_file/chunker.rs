//! File chunking utilities
//!
//! Provides efficient line-based file chunking for large file reading.
//! Chunks are designed to be processed independently by worker agents.

use super::types::{FileChunk, FileFormat, ReadError, thresholds};
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Computes chunks for a file based on size and line count
/// 
/// Returns a vector of non-overlapping chunks covering the entire file.
/// Each chunk targets approximately CHUNK_SIZE bytes.
pub fn compute_chunks(file_size: usize, line_count: usize) -> Vec<FileChunk> {
    if line_count == 0 {
        return vec![];
    }
    
    // Calculate average bytes per line
    let avg_bytes_per_line = file_size / line_count;
    let lines_per_chunk = (thresholds::CHUNK_SIZE / avg_bytes_per_line.max(1)).max(1);
    
    let mut chunks = Vec::new();
    let mut current_line = 1usize;
    let mut chunk_id = 0usize;
    
    while current_line <= line_count {
        let end_line = (current_line + lines_per_chunk - 1).min(line_count);
        let estimated_bytes = (end_line - current_line + 1) * avg_bytes_per_line;
        
        chunks.push(FileChunk::new(
            chunk_id,
            current_line,
            end_line,
            estimated_bytes,
        ));
        
        current_line = end_line + 1;
        chunk_id += 1;
    }
    
    chunks
}

/// Configuration for token-aware chunking
#[derive(Debug, Clone)]
pub struct TokenChunkConfig {
    /// Worker LLM context window size (in tokens)
    pub worker_context_window: usize,
    /// Percentage of context window to use for content (0.0 - 1.0)
    /// Default: 0.75 (75%) - leaves room for system prompt, tools, etc.
    pub utilization_ratio: f32,
    /// Overlap between chunks in tokens
    /// Default: 2500 tokens - provides context awareness
    pub overlap_tokens: usize,
    /// Minimum chunk size in tokens (avoid tiny chunks)
    pub min_chunk_tokens: usize,
}

impl Default for TokenChunkConfig {
    fn default() -> Self {
        Self {
            worker_context_window: 8192,  // Default to 8K context
            utilization_ratio: 0.75,      // 75% for content
            overlap_tokens: 2500,         // 2.5K token overlap
            min_chunk_tokens: 500,        // Minimum 500 tokens per chunk
        }
    }
}

impl TokenChunkConfig {
    /// Create config from worker profile's context window
    #[cfg(test)]
    pub fn for_worker_context(context_window: usize) -> Self {
        Self {
            worker_context_window: context_window,
            ..Default::default()
        }
    }
    
    /// Calculate effective chunk size (content budget per chunk)
    pub fn effective_chunk_size(&self) -> usize {
        let effective = (self.worker_context_window as f32 * self.utilization_ratio) as usize;
        effective.max(self.min_chunk_tokens)
    }
    
    /// Calculate step size (how much to advance between chunks)
    pub fn step_size(&self) -> usize {
        let effective = self.effective_chunk_size();
        // Step = effective size - overlap, but ensure we make progress
        (effective.saturating_sub(self.overlap_tokens)).max(self.min_chunk_tokens)
    }
}

/// Result of token-aware chunking
#[derive(Debug, Clone)]
pub struct TokenChunkResult {
    /// The chunks
    pub chunks: Vec<FileChunk>,
    /// Total tokens in document
    pub _total_tokens: usize,
    /// Tokens per chunk (target)
    pub _tokens_per_chunk: usize,
    /// Overlap in tokens
    pub _overlap_tokens: usize,
}

/// Computes chunks based on token budget and worker context window
/// 
/// This is the smart chunking algorithm that:
/// 1. Estimates total tokens in the document
/// 2. Calculates optimal chunk size based on worker's context window (75% for overhead)
/// 3. Adds overlap between chunks for context awareness
/// 4. Returns chunks that fit within worker LLM constraints
/// 
/// # Arguments
/// * `file_size` - Total file size in bytes
/// * `line_count` - Total number of lines
/// * `config` - Token chunking configuration (includes worker context window)
/// 
/// # Returns
/// TokenChunkResult with chunks and metadata
pub fn compute_chunks_with_tokens(
    file_size: usize,
    line_count: usize,
    config: &TokenChunkConfig,
) -> TokenChunkResult {
    if line_count == 0 {
        return TokenChunkResult {
            chunks: vec![],
            _total_tokens: 0,
            _tokens_per_chunk: 0,
            _overlap_tokens: 0,
        };
    }
    
    // Estimate total tokens (1 token ≈ 4 bytes)
    let total_tokens = file_size / 4;
    
    // Calculate effective chunk size and step
    let effective_chunk_tokens = config.effective_chunk_size();
    let step_tokens = config.step_size();
    
    // Convert tokens to lines
    let avg_bytes_per_line = file_size / line_count;
    let avg_tokens_per_line = avg_bytes_per_line.max(1) / 4;
    
    let lines_per_chunk = (effective_chunk_tokens / avg_tokens_per_line.max(1)).max(10);
    let lines_per_step = (step_tokens / avg_tokens_per_line.max(1)).max(5);
    
    let mut chunks = Vec::new();
    let mut current_line = 1usize;
    let mut chunk_id = 0usize;
    
    while current_line <= line_count {
        let end_line = (current_line + lines_per_chunk - 1).min(line_count);
        let estimated_bytes = (end_line - current_line + 1) * avg_bytes_per_line;
        
        chunks.push(FileChunk::new(
            chunk_id,
            current_line,
            end_line,
            estimated_bytes,
        ));
        
        // Advance by step size (creating overlap)
        current_line = (current_line + lines_per_step).min(end_line + 1);
        
        // Break if we've reached the end
        if current_line > line_count {
            break;
        }
        
        chunk_id += 1;
        
        // Safety limit: prevent too many chunks
        if chunk_id >= 100 {
            crate::warn_log!("[Chunker] Reached maximum chunk limit (100), consolidating remaining content");
            // Create one final chunk for all remaining lines
            if current_line <= line_count {
                let final_bytes = (line_count - current_line + 1) * avg_bytes_per_line;
                chunks.push(FileChunk::new(
                    chunk_id,
                    current_line,
                    line_count,
                    final_bytes,
                ));
            }
            break;
        }
    }
    
    TokenChunkResult {
        chunks,
        _total_tokens: total_tokens,
        _tokens_per_chunk: effective_chunk_tokens,
        _overlap_tokens: config.overlap_tokens,
    }
}

/// Determine optimal chunk configuration for a file
/// 
/// Analyzes the file and returns a TokenChunkConfig optimized for:
/// - The worker LLM's context window (from profile)
/// - The file size (larger files may need different overlap strategy)
pub fn determine_chunk_config(
    file_size: usize,
    worker_context_window: usize,
) -> TokenChunkConfig {
    // For very large files, slightly reduce overlap to limit total chunks
    let overlap = if file_size > 1_000_000 {
        // Files > 1MB: use smaller overlap to keep chunk count manageable
        2000
    } else if file_size > 500_000 {
        // Files 500KB-1MB: moderate overlap
        2250
    } else {
        // Smaller files: full overlap for better context
        2500
    };
    
    TokenChunkConfig {
        worker_context_window,
        utilization_ratio: 0.75,
        overlap_tokens: overlap,
        min_chunk_tokens: 500,
    }
}

/// Count lines in a file efficiently
pub async fn count_lines(path: &Path) -> Result<usize, ReadError> {
    let file = File::open(path).await
        .map_err(|e| ReadError::AccessError(format!("Cannot open file: {}", e)))?;
    
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut count = 0usize;
    
    while lines.next_line().await
        .map_err(|e| ReadError::ReadError(format!("Error reading file: {}", e)))?
        .is_some() 
    {
        count += 1;
    }
    
    Ok(count)
}

/// Read a specific line range from a file
/// 
/// # Arguments
/// * `path` - Path to the file
/// * `start_line` - 1-based starting line (inclusive)
/// * `end_line` - 1-based ending line (inclusive), None means until end
/// 
/// # Returns
/// The content of the specified line range
pub async fn read_line_range(
    path: &Path,
    start_line: usize,
    end_line: Option<usize>,
) -> Result<String, ReadError> {
    let file = File::open(path).await
        .map_err(|e| ReadError::AccessError(format!("Cannot open file: {}", e)))?;
    
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut current_line = 0usize;
    let mut content = String::new();
    let end = end_line.unwrap_or(usize::MAX);
    
    // Skip lines before start
    while current_line < start_line - 1 {
        match lines.next_line().await {
            Ok(Some(_)) => current_line += 1,
            Ok(None) => return Ok(content), // File shorter than start_line
            Err(e) => return Err(ReadError::ReadError(format!("Error reading file: {}", e))),
        }
    }
    
    // Read lines from start to end
    while current_line < end {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(&line);
                current_line += 1;
            }
            Ok(None) => break, // End of file
            Err(e) => return Err(ReadError::ReadError(format!("Error reading file: {}", e))),
        }
    }
    
    Ok(content)
}

/// Read a specific chunk from a file
pub async fn read_chunk(path: &Path, chunk: &FileChunk) -> Result<String, ReadError> {
    read_line_range(path, chunk.line_start, Some(chunk.line_end)).await
}

/// Get file statistics (size and line count)
pub async fn get_file_stats(path: &Path) -> Result<(usize, usize), ReadError> {
    let metadata = tokio::fs::metadata(path).await
        .map_err(|e| ReadError::AccessError(format!("Cannot access file: {}", e)))?;
    
    let size = metadata.len() as usize;
    let lines = count_lines(path).await?;
    
    Ok((size, lines))
}

/// Check if a file can be read directly based on size
/// 
/// Returns true if the file is small enough for direct reading
#[allow(dead_code)]
pub fn can_read_directly(file_size: usize) -> bool {
    file_size <= thresholds::MAX_DIRECT
}

/// Determine the best strategy for a file based on its size
/// 
/// This is used when ReadStrategy::Auto is selected
pub fn determine_strategy(file_size: usize) -> super::types::ReadStrategy {
    use super::types::ReadStrategy;
    
    if file_size <= thresholds::SMALL_FILE {
        ReadStrategy::Direct
    } else if file_size <= thresholds::MEDIUM_FILE {
        ReadStrategy::Direct // With warning
    } else if file_size <= thresholds::LARGE_FILE {
        ReadStrategy::Chunked
    } else {
        ReadStrategy::Search
    }
}

/// Chunker for handling large files
pub struct ChunkedReader {
    chunks: Vec<FileChunk>,
    path: std::path::PathBuf,
    total_lines: usize,
}

impl ChunkedReader {
    /// Create a new chunked reader for a file
    pub async fn new(path: impl AsRef<Path>) -> Result<Self, ReadError> {
        let path = path.as_ref().to_path_buf();
        let (size, lines) = get_file_stats(&path).await?;
        
        if lines == 0 {
            return Ok(Self {
                chunks: vec![],
                path,
                total_lines: 0,
            });
        }
        
        let chunks = compute_chunks(size, lines);
        
        Ok(Self {
            chunks,
            path,
            total_lines: lines,
        })
    }
    
    /// Get all chunks
    pub fn chunks(&self) -> &[FileChunk] {
        &self.chunks
    }
    
    /// Get chunk count
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
    
    /// Get total lines
    pub fn total_lines(&self) -> usize {
        self.total_lines
    }
    
    /// Read a specific chunk by ID
    pub async fn read_chunk_by_id(&self, chunk_id: usize) -> Result<String, ReadError> {
        let chunk = self.chunks.get(chunk_id)
            .ok_or_else(|| ReadError::InvalidArgument(
                format!("Invalid chunk ID: {}", chunk_id)
            ))?;
        
        read_chunk(&self.path, chunk).await
    }
    
    /// Find the chunk containing a specific line
    pub fn find_chunk_for_line(&self, line: usize) -> Option<&FileChunk> {
        self.chunks.iter().find(|c| c.line_start <= line && c.line_end >= line)
    }
    
    /// Get the file path
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Utility to detect if content is binary
/// 
/// Checks for null bytes in the first 8KB of content
pub fn is_binary_content(content: &[u8]) -> bool {
    const SAMPLE_SIZE: usize = 8192;
    let sample = &content[..content.len().min(SAMPLE_SIZE)];
    sample.contains(&0)
}

/// Detect file format and check if it's readable
pub async fn check_file_readable(path: &Path) -> Result<FileFormat, ReadError> {
    // Check if file exists
    let metadata = tokio::fs::metadata(path).await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ReadError::FileNotFound(path.to_string_lossy().to_string()),
            _ => ReadError::AccessError(format!("Cannot access file: {}", e)),
        })?;
    
    // Check if it's a directory
    if metadata.is_dir() {
        return Err(ReadError::IsDirectory(path.to_string_lossy().to_string()));
    }
    
    // Check file size
    if metadata.len() > thresholds::MAX_DIRECT as u64 {
        // Will need chunking, but file is accessible
    }
    
    // Detect format from extension
    let format = path.extension()
        .and_then(|e| e.to_str())
        .map(FileFormat::from_extension)
        .unwrap_or(FileFormat::Unknown);
    
    // If unknown format, check if it's binary
    if format == FileFormat::Unknown {
        let sample = tokio::fs::read(path).await
            .map_err(|e| ReadError::ReadError(format!("Cannot read file: {}", e)))?;
        
        if is_binary_content(&sample) {
            // It's binary - we'll try to extract strings or reject
            // For now, return Unknown and let caller decide
        }
    }
    
    Ok(format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;
    
    #[tokio::test]
    async fn test_compute_chunks() {
        // 1000 lines, 100KB file = ~100 bytes/line
        let chunks = compute_chunks(100_000, 1000);
        
        // Each chunk should be ~500 lines (50KB / 100 bytes/line)
        assert!(chunks.len() >= 1);
        assert_eq!(chunks[0].line_start, 1);
        assert_eq!(chunks.last().unwrap().line_end, 1000);
    }
    
    #[tokio::test]
    async fn test_read_line_range() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        
        fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").await.unwrap();
        
        // Read lines 2-4
        let content = read_line_range(&path, 2, Some(4)).await.unwrap();
        assert_eq!(content, "line2\nline3\nline4");
        
        // Read from line 3 to end
        let content = read_line_range(&path, 3, None).await.unwrap();
        assert_eq!(content, "line3\nline4\nline5");
        
        // Read beyond file length
        let content = read_line_range(&path, 10, Some(20)).await.unwrap();
        assert!(content.is_empty());
    }
    
    #[tokio::test]
    async fn test_count_lines() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        
        fs::write(&path, "line1\nline2\nline3\n").await.unwrap();
        assert_eq!(count_lines(&path).await.unwrap(), 3);
        
        // Empty file
        fs::write(&path, "").await.unwrap();
        assert_eq!(count_lines(&path).await.unwrap(), 0);
        
        // Single line without newline
        fs::write(&path, "single").await.unwrap();
        assert_eq!(count_lines(&path).await.unwrap(), 1);
    }
    
    #[tokio::test]
    async fn test_chunked_reader() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        
        // Create file with 100 lines
        let content: String = (1..=100).map(|i| format!("line{}\n", i)).collect();
        fs::write(&path, content).await.unwrap();
        
        let reader = ChunkedReader::new(&path).await.unwrap();
        assert_eq!(reader.total_lines(), 100);
        assert!(reader.chunk_count() >= 1);
        
        // Test reading chunk 0
        let chunk_content = reader.read_chunk_by_id(0).await.unwrap();
        assert!(chunk_content.starts_with("line1"));
        
        // Test find_chunk_for_line
        let chunk = reader.find_chunk_for_line(50).unwrap();
        assert!(chunk.line_start <= 50 && chunk.line_end >= 50);
    }
    
    #[test]
    fn test_is_binary_content() {
        assert!(!is_binary_content(b"hello world"));
        assert!(!is_binary_content(b"line1\nline2\n"));
        assert!(is_binary_content(b"hello\x00world"));
        assert!(is_binary_content(&[0u8; 100]));
    }
    
    #[tokio::test]
    async fn test_check_file_readable() {
        let temp = TempDir::new().unwrap();
        
        // Text file
        let txt_path = temp.path().join("test.txt");
        fs::write(&txt_path, "content").await.unwrap();
        let format = check_file_readable(&txt_path).await.unwrap();
        assert_eq!(format, FileFormat::Text);
        
        // PDF file
        let pdf_path = temp.path().join("test.pdf");
        fs::write(&pdf_path, "%PDF-1.4").await.unwrap();
        let format = check_file_readable(&pdf_path).await.unwrap();
        assert_eq!(format, FileFormat::Pdf);
        
        // Non-existent file
        let missing = temp.path().join("missing.txt");
        assert!(check_file_readable(&missing).await.is_err());
        
        // Directory
        let dir = temp.path().join("testdir");
        fs::create_dir(&dir).await.unwrap();
        assert!(check_file_readable(&dir).await.is_err());
    }
    
    #[test]
    fn test_determine_strategy() {
        use super::super::types::ReadStrategy;
        
        assert_eq!(determine_strategy(5_000), ReadStrategy::Direct);
        assert_eq!(determine_strategy(50_000), ReadStrategy::Direct);
        assert_eq!(determine_strategy(500_000), ReadStrategy::Chunked);
        assert_eq!(determine_strategy(2_000_000), ReadStrategy::Search);
    }
    
    #[test]
    fn test_token_chunk_config() {
        // Default config
        let config = TokenChunkConfig::default();
        assert_eq!(config.worker_context_window, 8192);
        assert_eq!(config.utilization_ratio, 0.75);
        assert_eq!(config.overlap_tokens, 2500);
        assert_eq!(config.min_chunk_tokens, 500);
        
        // Effective chunk size: 8192 * 0.75 = 6144
        assert_eq!(config.effective_chunk_size(), 6144);
        
        // Step size: 6144 - 2500 = 3644
        assert_eq!(config.step_size(), 3644);
        
        // For worker context
        let config = TokenChunkConfig::for_worker_context(32000);
        assert_eq!(config.worker_context_window, 32000);
        assert_eq!(config.effective_chunk_size(), 24000); // 32000 * 0.75
    }
    
    #[test]
    fn test_compute_chunks_with_tokens_small_file() {
        // Small file: 1000 lines, 50KB
        let config = TokenChunkConfig::for_worker_context(8192);
        let result = compute_chunks_with_tokens(50_000, 1000, &config);
        
        // Should create chunks that fit in worker context
        assert!(!result.chunks.is_empty());
        assert_eq!(result.total_tokens, 12_500); // 50KB / 4
        assert_eq!(result.tokens_per_chunk, 6144); // 8192 * 0.75
        
        // First chunk should start at line 1
        assert_eq!(result.chunks[0].line_start, 1);
        // Last chunk should end at line 1000
        assert_eq!(result.chunks.last().unwrap().line_end, 1000);
    }
    
    #[test]
    fn test_compute_chunks_with_tokens_large_file() {
        // Large file: 10000 lines, 500KB (simulating your PDF scenario)
        let config = TokenChunkConfig::for_worker_context(8192);
        let result = compute_chunks_with_tokens(500_000, 10_000, &config);
        
        // 500KB = ~125K tokens
        assert_eq!(result.total_tokens, 125_000);
        
        // With 6144 tokens per chunk and 2500 overlap, step = 3644
        // Expected chunks: 125000 / 3644 ≈ 35 chunks
        // But we limit to 100 max
        assert!(result.chunks.len() > 1);
        assert!(result.chunks.len() <= 100);
        
        // Verify overlap: consecutive chunks should overlap
        if result.chunks.len() > 1 {
            let first_end = result.chunks[0].line_end;
            let second_start = result.chunks[1].line_start;
            // Second chunk should start before first ends (overlap)
            assert!(second_start <= first_end);
        }
    }
    
    #[test]
    fn test_compute_chunks_with_tokens_large_context() {
        // Large context window (128K) should create fewer, bigger chunks
        let config = TokenChunkConfig::for_worker_context(128_000);
        let result = compute_chunks_with_tokens(500_000, 10_000, &config);
        
        // 128K * 0.75 = 96K tokens per chunk
        assert_eq!(result.tokens_per_chunk, 96_000);
        
        // Should have fewer chunks than with 8K context
        assert!(result.chunks.len() <= 10);
    }
    
    #[test]
    fn test_determine_chunk_config() {
        // Small file (< 500KB)
        let config = determine_chunk_config(400_000, 8192);
        assert_eq!(config.overlap_tokens, 2500);
        
        // Medium file (500KB - 1MB)
        let config = determine_chunk_config(750_000, 8192);
        assert_eq!(config.overlap_tokens, 2250);
        
        // Large file (> 1MB)
        let config = determine_chunk_config(2_000_000, 8192);
        assert_eq!(config.overlap_tokens, 2000);
        
        // Context window is preserved
        assert_eq!(config.worker_context_window, 8192);
    }
    
    #[test]
    fn test_empty_file_chunks() {
        let config = TokenChunkConfig::default();
        let result = compute_chunks_with_tokens(0, 0, &config);
        
        assert!(result.chunks.is_empty());
        assert_eq!(result.total_tokens, 0);
    }
}
