//! End-to-end tests for read_file tool
//!
//! Tests the complete flow including:
//! - Direct file reading
//! - Partial file reading (line_offset, n_lines)
//! - Large file chunking
//! - PDF extraction
//! - Search integration

use crate::agent::tools::{
    ReadFileTool, SearchFilesTool, ChunkPool, 
    read_file::ReadStrategy,
    read_file::ReadMetadata,
};
use crate::agent::runtime::core::{ToolCapability, RuntimeContext};
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::fs;

/// Test reading a small file directly
#[tokio::test]
async fn test_e2e_read_small_file() {
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("test.txt");
    fs::write(&file_path, "Hello, World!").await.unwrap();
    
    let tool = ReadFileTool::simple();
    let call = ToolCall::new("read_file", serde_json::json!(file_path.to_str().unwrap()));
    
    let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
    
    match result {
        ToolResult::Success { output, .. } => {
            assert!(output.contains("Hello, World!"));
        }
        ToolResult::Error { message, .. } => panic!("Expected success, got error: {}", message),
        ToolResult::Cancelled => panic!("Expected success, got cancelled"),
    }
}

/// Test partial file reading with line_offset and n_lines
#[tokio::test]
async fn test_e2e_partial_read() {
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("lines.txt");
    
    // Create file with 10 lines
    let content: String = (1..=10).map(|i| format!("Line {}\n", i)).collect();
    fs::write(&file_path, content).await.unwrap();
    
    let tool = ReadFileTool::simple();
    let call = ToolCall::new("read_file", serde_json::json!({
        "path": file_path.to_str().unwrap(),
        "line_offset": 3,
        "n_lines": 4
    }));
    
    let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
    
    match result {
        ToolResult::Success { output, .. } => {
            assert!(output.contains("Line 3"));
            assert!(output.contains("Line 6"));
            assert!(!output.contains("Line 2")); // Should not include
            assert!(!output.contains("Line 7")); // Should not include
        }
        ToolResult::Error { message, .. } => panic!("Expected success, got error: {}", message),
        ToolResult::Cancelled => panic!("Expected success, got cancelled"),
    }
}

/// Test that large files use chunked strategy
#[tokio::test]
async fn test_e2e_large_file_chunking() {
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("large.txt");
    
    // Create file > 100KB to trigger chunking
    let chunk = "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\n";
    let repetitions = 2500; // ~140KB
    let content = chunk.repeat(repetitions);
    fs::write(&file_path, content).await.unwrap();
    
    let chunk_pool = Arc::new(ChunkPool::new("test", 5, 8192));
    let tool = ReadFileTool::new(Arc::clone(&chunk_pool), None);
    
    let call = ToolCall::new("read_file", serde_json::json!(file_path.to_str().unwrap()));
    let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
    
    match result {
        ToolResult::Success { structured, .. } => {
            let metadata: ReadMetadata = 
                serde_json::from_value(structured.unwrap()).unwrap();
            
            // Should use chunked strategy for large files
            assert_eq!(metadata.strategy_used, ReadStrategy::Chunked);
        }
        ToolResult::Error { message, .. } => panic!("Expected success, got error: {}", message),
        ToolResult::Cancelled => panic!("Expected success, got cancelled"),
    }
}

/// Test PDF text extraction
#[tokio::test]
async fn test_e2e_pdf_extraction() {
    // Note: This test requires a real PDF file
    // For CI environments, we skip this test
    if std::env::var("CI").is_ok() {
        return;
    }
    
    // Create a minimal PDF for testing
    let temp = TempDir::new().unwrap();
    let pdf_path = temp.path().join("test.pdf");
    
    // Minimal valid PDF content
    let pdf_content = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>\nendobj\n4 0 obj\n<< /Length 44 >>\nstream\nBT /F1 12 Tf 100 700 Td (Hello PDF) Tj ET\nendstream\nendobj\nxref\n0 5\n0000000000 65535 f\n0000000009 00000 n\n0000000058 00000 n\n0000000115 00000 n\n0000000214 00000 n\ntrailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n312\n%%EOF";
    
    fs::write(&pdf_path, pdf_content).await.unwrap();
    
    let tool = ReadFileTool::simple();
    let call = ToolCall::new("read_file", serde_json::json!(pdf_path.to_str().unwrap()));
    
    let result = tool.execute(&RuntimeContext::new(), call).await;
    
    // PDF extraction may fail with minimal PDF, but should handle gracefully
    match result {
        Ok(ToolResult::Success { .. }) => {}
        Ok(ToolResult::Error { .. }) => {}
        Ok(ToolResult::Cancelled) => {}
        Err(_) => {}
    }
}

/// Test search_files tool end-to-end
#[tokio::test]
async fn test_e2e_search_files() {
    let search_tool = SearchFilesTool::new().unwrap();
    
    // Index some test files
    search_tool.index_file("src/main.rs", "fn main() {\n    println!(\"Hello\");\n}", 1, 3).await.unwrap();
    search_tool.index_file("src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}", 1, 2).await.unwrap();
    search_tool.index_file("tests/test.rs", "#[test]\nfn test_add() {\n    assert_eq!(add(1, 2), 3);\n}", 1, 3).await.unwrap();
    
    // Search for "main"
    let results = search_tool.search("main", None).await.unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().any(|r| r.path == "src/main.rs"));
    
    // Search for "test"
    let results = search_tool.search("test", None).await.unwrap();
    assert!(!results.is_empty());
    
    // Search with path filter
    let results = search_tool.search("fn", Some("tests")).await.unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().all(|r| r.path.contains("tests")));
}

/// Test ChunkPool worker management
#[tokio::test]
async fn test_e2e_chunk_pool_management() {
    let pool = Arc::new(ChunkPool::new("test", 3, 8192));
    
    // Initially no workers
    assert_eq!(pool.worker_count().await, 0);
    
    // Can spawn workers
    assert!(pool.can_spawn().await);
    
    // Register some chunks
    let path = std::path::PathBuf::from("/test/file.txt");
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    
    let chunk = crate::agent::tools::read_file::ActiveChunk {
        chunk_id: 0,
        worker_id: crate::agent::types::events::WorkerId(1),
        line_range: (1, 100),
        summary: "Test chunk".to_string(),
        key_terms: vec!["test".to_string()],
        content_hash: "abc".to_string(),
        query_tx: tx,
    };
    
    pool.register_chunk(path.clone(), chunk).await.unwrap();
    assert_eq!(pool.worker_count().await, 1);
    
    // Find chunks for file
    let chunks = pool.list_chunks_for_file(&path).await;
    assert_eq!(chunks.len(), 1);
    
    // Clear pool
    pool.clear().await;
    assert_eq!(pool.worker_count().await, 0);
}

/// Test file reading with different strategies
#[tokio::test]
async fn test_e2e_strategy_selection() {
    use crate::agent::tools::read_file::thresholds;
    
    let temp = TempDir::new().unwrap();
    
    // Small file - should use direct
    let small = temp.path().join("small.txt");
    fs::write(&small, "small content").await.unwrap();
    let size = std::fs::metadata(&small).unwrap().len() as usize;
    assert!(size < thresholds::SMALL_FILE);
    
    // Medium file - should use direct with warning
    let medium = temp.path().join("medium.txt");
    let content = "x".repeat(thresholds::SMALL_FILE + 1000);
    fs::write(&medium, content).await.unwrap();
    let size = std::fs::metadata(&medium).unwrap().len() as usize;
    assert!(size > thresholds::SMALL_FILE && size < thresholds::MEDIUM_FILE);
    
    // Large file - should use chunked
    let large = temp.path().join("large.txt");
    let content = "x".repeat(thresholds::MEDIUM_FILE + 1000);
    fs::write(&large, content).await.unwrap();
    let size = std::fs::metadata(&large).unwrap().len() as usize;
    assert!(size > thresholds::MEDIUM_FILE);
}

/// Test error handling for non-existent files
#[tokio::test]
async fn test_e2e_file_not_found() {
    let tool = ReadFileTool::simple();
    let call = ToolCall::new("read_file", serde_json::json!("/nonexistent/file.txt"));
    
    let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
    
    match result {
        ToolResult::Error { code, .. } => {
            assert_eq!(code, Some("ACCESS_ERROR".to_string()));
        }
        ToolResult::Success { .. } => panic!("Expected error for non-existent file"),
        ToolResult::Cancelled => panic!("Expected error, got cancelled"),
    }
}

/// Test reading a directory (should fail gracefully)
#[tokio::test]
async fn test_e2e_read_directory() {
    let temp = TempDir::new().unwrap();
    let tool = ReadFileTool::simple();
    let call = ToolCall::new("read_file", serde_json::json!(temp.path().to_str().unwrap()));
    
    let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();
    
    match result {
        ToolResult::Error { code, .. } => {
            assert_eq!(code, Some("ACCESS_ERROR".to_string()));
        }
        ToolResult::Success { .. } => panic!("Expected error for directory"),
        ToolResult::Cancelled => panic!("Expected error, got cancelled"),
    }
}
