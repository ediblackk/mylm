//! DOCX extraction utilities
//!
//! Provides text extraction from Microsoft Word documents.
//! Uses the docx-rs crate for parsing .docx files.

use super::types::ReadError;
use std::path::Path;

/// Extract text content from a DOCX file
///
/// # Arguments
/// * `path` - Path to the DOCX file
///
/// # Returns
/// Extracted text content with paragraph structure preserved
///
/// # Errors
/// Returns `ReadError::ReadError` if extraction fails
pub async fn extract_text(path: &Path) -> Result<String, ReadError> {
    let path = path.to_path_buf();
    
    tokio::task::spawn_blocking(move || {
        extract_text_sync(&path)
    })
    .await
    .map_err(|e| ReadError::ReadError(format!("DOCX extraction task panicked: {}", e)))?
}

/// Synchronous DOCX text extraction
fn extract_text_sync(path: &Path) -> Result<String, ReadError> {
    // Read the docx file into memory
    let buf = std::fs::read(path)
        .map_err(|e| ReadError::AccessError(format!("Cannot read DOCX file: {}", e)))?;
    
    // Parse the docx
    let docx = docx_rs::read_docx(&buf)
        .map_err(|e| ReadError::ReadError(format!("Failed to parse DOCX: {}", e)))?;
    
    let mut text = String::new();
    
    // Extract text from document children (paragraphs and tables)
    for child in docx.document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(paragraph) => {
                extract_paragraph_text(&paragraph, &mut text);
                text.push('\n'); // Preserve paragraph breaks
            }
            docx_rs::DocumentChild::Table(table) => {
                extract_table_text(&table, &mut text);
                text.push('\n'); // Table separator
            }
            _ => {} // Ignore other document elements
        }
    }
    
    if text.trim().is_empty() {
        return Err(ReadError::ReadError(
            "DOCX file appears to be empty or contains no extractable text".to_string()
        ));
    }
    
    Ok(text)
}

/// Extract text from a paragraph
fn extract_paragraph_text(paragraph: &docx_rs::Paragraph, text: &mut String) {
    for child in &paragraph.children {
        if let docx_rs::ParagraphChild::Run(run) = child {
            for run_child in &run.children {
                if let docx_rs::RunChild::Text(text_element) = run_child {
                    text.push_str(&text_element.text);
                }
            }
        }
    }
}

/// Extract text from a table
fn extract_table_text(table: &docx_rs::Table, text: &mut String) {
    for row_child in &table.rows {
        let docx_rs::TableChild::TableRow(row) = row_child;
        for cell_child in &row.cells {
            let docx_rs::TableRowChild::TableCell(cell) = cell_child;
            // Extract text from all content in the cell
            for cell_content in &cell.children {
                match cell_content {
                    docx_rs::TableCellContent::Paragraph(paragraph) => {
                        extract_paragraph_text(paragraph, text);
                        text.push(' ');
                    }
                    docx_rs::TableCellContent::Table(nested_table) => {
                        extract_table_text(nested_table, text);
                    }
                    _ => {} // Ignore other content types
                }
            }
            text.push('|'); // Cell separator
        }
        text.push('\n'); // Row separator
    }
}

/// Get DOCX document info (paragraph count, word count estimate)
pub async fn get_docx_info(path: &Path) -> Result<DocxInfo, ReadError> {
    let text = extract_text(path).await?;
    
    let paragraph_count = text.lines().filter(|l| !l.trim().is_empty()).count();
    let word_count = text.split_whitespace().count();
    
    Ok(DocxInfo {
        paragraph_count,
        word_count,
    })
}

/// DOCX document information
#[derive(Debug, Clone)]
pub struct DocxInfo {
    pub paragraph_count: usize,
    pub word_count: usize,
}
