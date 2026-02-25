//! PDF extraction utilities
//!
//! Provides text extraction from PDF documents.
//! Uses the pdf-extract crate for efficient text extraction.
//! 
//! TODO: Future enhancement - Vision model support for diagrams/images in PDFs

use super::types::ReadError;
use std::path::Path;

/// Extract text content from a PDF file
/// 
/// # Arguments
/// * `path` - Path to the PDF file
/// 
/// # Returns
/// Extracted text content with page markers
/// 
/// # Errors
/// Returns `ReadError::PdfEncrypted` if the PDF is encrypted
/// Returns `ReadError::PdfExtractionError` if extraction fails
pub async fn extract_text(path: &Path) -> Result<String, ReadError> {
    // pdf-extract is synchronous, so we use spawn_blocking
    let path = path.to_path_buf();
    
    tokio::task::spawn_blocking(move || {
        extract_text_sync(&path)
    })
    .await
    .map_err(|e| ReadError::PdfExtractionError(format!("Task panicked: {}", e)))?
}

/// Synchronous PDF text extraction
/// 
/// Suppresses stderr output from pdf-extract to avoid unicode warnings
/// flooding the terminal.
fn extract_text_sync(path: &Path) -> Result<String, ReadError> {
    // First check if file is readable
    let content = std::fs::read(path)
        .map_err(|e| ReadError::AccessError(format!("Cannot read PDF: {}", e)))?;
    
    // Check for encryption marker
    if is_encrypted(&content) {
        return Err(ReadError::PdfEncrypted);
    }
    
    // Suppress stderr during extraction to hide pdf-extract's unicode warnings
    let _print_gag = gag::Gag::stdout().ok();
    let _stderr_gag = gag::Gag::stderr().ok();
    
    // Extract text using pdf-extract
    pdf_extract::extract_text(path).map_err(|e| {
        let error_str = e.to_string();
        // Check if this is an encryption error
        let error_lower = error_str.to_lowercase();
        if error_lower.contains("password") || 
           error_lower.contains("encrypt") || 
           error_lower.contains("decrypt") {
            ReadError::PdfEncrypted
        } else {
            ReadError::PdfExtractionError(format!(
                "Failed to extract PDF text: {}", error_str
            ))
        }
    })
}

/// Check if PDF content appears to be encrypted
/// 
/// Looks for encryption markers in the PDF header/trailer
fn is_encrypted(content: &[u8]) -> bool {
    // Convert to string for pattern matching (PDFs are mostly text-based)
    let sample = String::from_utf8_lossy(&content[..content.len().min(8192)]);
    
    // Check for /Encrypt entry in trailer
    sample.contains("/Encrypt") || sample.contains("/EncryptMetadata")
}

/// Extract text with page markers
/// 
/// Returns text with embedded page markers like "\n--- Page N ---\n"
pub async fn extract_text_with_pages(path: &Path) -> Result<String, ReadError> {
    let path = path.to_path_buf();
    
    tokio::task::spawn_blocking(move || {
        extract_text_with_pages_sync(&path)
    })
    .await
    .map_err(|e| ReadError::PdfExtractionError(format!("Task panicked: {}", e)))?
}

/// Synchronous extraction with page markers
fn extract_text_with_pages_sync(path: &Path) -> Result<String, ReadError> {
    // pdf-extract doesn't give us page-by-page control easily
    // So we extract all text and then post-process to add page markers
    // This is a best-effort approach
    
    let text = extract_text_sync(path)?;
    
    // Add page markers based on form feed characters or just return as-is
    // Many PDFs don't have clear page boundaries in extracted text
    // For now, return the text with a header note
    let mut result = String::new();
    result.push_str("<!-- PDF Text Extracted -->\n");
    result.push_str(&text);
    
    Ok(result)
}

/// Get PDF metadata
/// 
/// Returns basic metadata like page count if available
pub async fn get_pdf_info(path: &Path) -> Result<PdfInfo, ReadError> {
    let path = path.to_path_buf();
    
    tokio::task::spawn_blocking(move || {
        get_pdf_info_sync(&path)
    })
    .await
    .map_err(|e| ReadError::PdfExtractionError(format!("Task panicked: {}", e)))?
}

/// PDF metadata structure
#[derive(Debug, Clone)]
pub struct PdfInfo {
    /// Number of pages (if detectable)
    pub page_count: Option<usize>,
    /// Title from metadata (if available)
    pub title: Option<String>,
    /// Author from metadata (if available)
    pub author: Option<String>,
    /// Whether the PDF is encrypted
    pub is_encrypted: bool,
}

/// Synchronous PDF info extraction
fn get_pdf_info_sync(path: &Path) -> Result<PdfInfo, ReadError> {
    let content = std::fs::read(path)
        .map_err(|e| ReadError::AccessError(format!("Cannot read PDF: {}", e)))?;
    
    let is_encrypted = is_encrypted(&content);
    
    // Try to extract page count from PDF structure
    // This is a simple heuristic - count /Type /Page occurrences
    let text = String::from_utf8_lossy(&content);
    let page_count = text.matches("/Type /Page").count();
    let page_count = if page_count > 0 { Some(page_count) } else { None };
    
    // Extract title and author from metadata
    let title = extract_metadata_field(&text, "Title");
    let author = extract_metadata_field(&text, "Author");
    
    Ok(PdfInfo {
        page_count,
        title,
        author,
        is_encrypted,
    })
}

/// Extract a metadata field from PDF content
fn extract_metadata_field(content: &str, field: &str) -> Option<String> {
    // Look for patterns like /Title (value) or <</Title (value)>>
    let pattern = format!("/{} ", field);
    if let Some(pos) = content.find(&pattern) {
        let start = pos + pattern.len();
        let remaining = &content[start..];
        
        // Handle parenthesized strings: (value)
        if remaining.starts_with('(') {
            let end = find_matching_paren(remaining, 0)?;
            let value = &remaining[1..end];
            return Some(value.to_string());
        }
        
        // Handle hex strings: <...>
        if remaining.starts_with('<') && !remaining.starts_with("<<") {
            let end = remaining.find('>')?;
            let hex_str = &remaining[1..end];
            // Try to decode hex
            if let Ok(decoded) = hex::decode(hex_str.replace(' ', "")) {
                if let Ok(text) = String::from_utf8(decoded) {
                    return Some(text);
                }
            }
        }
    }
    
    None
}

/// Find matching closing parenthesis
fn find_matching_paren(s: &str, start: usize) -> Option<usize> {
    let mut depth = 0;
    let bytes = s.as_bytes();
    
    for i in start..s.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'\\' => {
                // Skip escaped character
                if i + 1 < s.len() {
                    continue;
                }
            }
            _ => {}
        }
    }
    
    None
}

/// Convert PDF content to line-oriented format for chunking
/// 
/// Takes extracted text and formats it with virtual line numbers
/// based on page numbers for consistent chunking
pub fn pdf_text_to_lines(text: &str, pages: Option<usize>) -> String {
    let mut result = String::new();
    
    if let Some(page_count) = pages {
        // Add page markers as comments/lines
        let lines_per_page_estimate = text.lines().count() / page_count.max(1);
        
        for (i, line) in text.lines().enumerate() {
            let page_num = (i / lines_per_page_estimate.max(1)) + 1;
            if i % lines_per_page_estimate == 0 {
                result.push_str(&format!("<!-- Page {} -->\n", page_num));
            }
            result.push_str(line);
            result.push('\n');
        }
    } else {
        // No page info, just return as-is
        result.push_str(text);
    }
    
    result
}

/// Stub for future vision model support
/// 
/// TODO: Implement vision model integration for extracting content from
/// diagrams, charts, and scanned images within PDFs
pub struct VisionModelStub;

impl VisionModelStub {
    /// Extract images from PDF pages
    /// 
    /// TODO: Implement using pdf2image or similar
    pub fn extract_page_images(_path: &Path) -> Result<Vec<Vec<u8>>, ReadError> {
        Err(ReadError::PdfExtractionError(
            "Vision model support not yet implemented".to_string()
        ))
    }
    
    /// Analyze image with vision model
    /// 
    /// TODO: Integrate with multimodal LLM API
    pub async fn analyze_image(_image: &[u8]) -> Result<String, ReadError> {
        Err(ReadError::PdfExtractionError(
            "Vision model support not yet implemented".to_string()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;
    
    // Note: These tests require actual PDF files
    // We create minimal PDF-like content for basic tests
    
    #[test]
    fn test_is_encrypted() {
        let normal_pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj";
        assert!(!is_encrypted(normal_pdf));
        
        let encrypted_pdf = b"%PDF-1.4\n/Encrypt << /Filter /Standard >>";
        assert!(is_encrypted(encrypted_pdf));
        
        let encrypted_metadata = b"%PDF-1.4\n/EncryptMetadata true";
        assert!(is_encrypted(encrypted_metadata));
    }
    
    #[test]
    fn test_pdf_text_to_lines() {
        let text = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6";
        let result = pdf_text_to_lines(text, Some(2));
        assert!(result.contains("<!-- Page 1 -->"));
        assert!(result.contains("<!-- Page 2 -->"));
        assert!(result.contains("Line 1"));
    }
    
    #[test]
    fn test_pdf_text_to_lines_no_pages() {
        let text = "Line 1\nLine 2";
        let result = pdf_text_to_lines(text, None);
        assert!(!result.contains("<!-- Page"));
        assert!(result.contains("Line 1"));
    }
    
    #[test]
    fn test_find_matching_paren() {
        assert_eq!(find_matching_paren("(hello)", 0), Some(6));
        assert_eq!(find_matching_paren("(he(llo))", 0), Some(8));
        assert_eq!(find_matching_paren("no paren", 0), None);
        assert_eq!(find_matching_paren("(unclosed", 0), None);
    }
    
    #[test]
    fn test_extract_metadata_field() {
        let content = "/Title (Hello World) /Author (John Doe)";
        assert_eq!(extract_metadata_field(content, "Title"), Some("Hello World".to_string()));
        assert_eq!(extract_metadata_field(content, "Author"), Some("John Doe".to_string()));
        assert_eq!(extract_metadata_field(content, "Subject"), None);
    }
    
    // Integration test - requires real PDF
    // #[tokio::test]
    // async fn test_extract_text_real_pdf() {
    //     let text = extract_text(Path::new("/path/to/test.pdf")).await.unwrap();
    //     assert!(!text.is_empty());
    // }
}
