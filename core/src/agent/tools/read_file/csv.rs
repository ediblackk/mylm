//! CSV extraction utilities
//!
//! Provides text extraction from CSV files with proper handling of
//! headers, rows, and formatting for LLM consumption.

use super::types::ReadError;
use std::path::Path;

/// Extract text content from a CSV file
///
/// # Arguments
/// * `path` - Path to the CSV file
///
/// # Returns
/// Formatted text representation of the CSV with headers and rows
///
/// # Errors
/// Returns `ReadError::ReadError` if parsing fails
pub async fn extract_text(path: &Path) -> Result<String, ReadError> {
    let path = path.to_path_buf();
    
    tokio::task::spawn_blocking(move || {
        extract_text_sync(&path)
    })
    .await
    .map_err(|e| ReadError::ReadError(format!("CSV extraction task panicked: {}", e)))?
}

/// Synchronous CSV text extraction
fn extract_text_sync(path: &Path) -> Result<String, ReadError> {
    let file = std::fs::File::open(path)
        .map_err(|e| ReadError::AccessError(format!("Cannot open CSV: {}", e)))?;
    
    let mut reader = csv::Reader::from_reader(file);
    let headers = reader.headers()
        .map_err(|e| ReadError::ReadError(format!("Failed to read CSV headers: {}", e)))?
        .clone();
    
    let mut output = String::new();
    
    // Add headers
    output.push_str("CSV Headers:\n");
    for (i, header) in headers.iter().enumerate() {
        if i > 0 {
            output.push_str(" | ");
        }
        output.push_str(header);
    }
    output.push('\n');
    output.push_str(&"-".repeat(50));
    output.push('\n');
    
    // Add rows
    let mut row_count = 0;
    for result in reader.records() {
        let record = result.map_err(|e| ReadError::ReadError(format!("Failed to read CSV row: {}", e)))?;
        
        for (i, field) in record.iter().enumerate() {
            if i > 0 {
                output.push_str(" | ");
            }
            output.push_str(field);
        }
        output.push('\n');
        row_count += 1;
        
        // Limit to first 1000 rows for preview
        if row_count >= 1000 {
            output.push_str("... (truncated after 1000 rows)\n");
            break;
        }
    }
    
    output.push_str(&format!("\nTotal rows: {}\n", row_count));
    
    Ok(output)
}

/// Get CSV statistics (row count, column count)
pub async fn get_csv_info(path: &Path) -> Result<CsvInfo, ReadError> {
    let path = path.to_path_buf();
    
    tokio::task::spawn_blocking(move || {
        get_csv_info_sync(&path)
    })
    .await
    .map_err(|e| ReadError::ReadError(format!("CSV info task panicked: {}", e)))?
}

fn get_csv_info_sync(path: &Path) -> Result<CsvInfo, ReadError> {
    let file = std::fs::File::open(path)
        .map_err(|e| ReadError::AccessError(format!("Cannot open CSV: {}", e)))?;
    
    let mut reader = csv::Reader::from_reader(file);
    let headers = reader.headers()
        .map_err(|e| ReadError::ReadError(format!("Failed to read CSV headers: {}", e)))?;
    
    let column_count = headers.len();
    let mut row_count = 0;
    
    for result in reader.records() {
        let _ = result.map_err(|e| ReadError::ReadError(format!("Failed to read CSV row: {}", e)))?;
        row_count += 1;
    }
    
    Ok(CsvInfo {
        row_count,
        column_count,
    })
}

/// CSV file information
#[derive(Debug, Clone)]
pub struct CsvInfo {
    pub row_count: usize,
    pub column_count: usize,
}

/// Read specific row range from CSV
pub async fn read_row_range(
    path: &Path,
    start_row: usize,
    end_row: Option<usize>,
) -> Result<String, ReadError> {
    let path = path.to_path_buf();
    
    tokio::task::spawn_blocking(move || {
        read_row_range_sync(&path, start_row, end_row)
    })
    .await
    .map_err(|e| ReadError::ReadError(format!("CSV range read task panicked: {}", e)))?
}

fn read_row_range_sync(
    path: &Path,
    start_row: usize,
    end_row: Option<usize>,
) -> Result<String, ReadError> {
    let file = std::fs::File::open(path)
        .map_err(|e| ReadError::AccessError(format!("Cannot open CSV: {}", e)))?;
    
    let mut reader = csv::Reader::from_reader(file);
    let headers = reader.headers()
        .map_err(|e| ReadError::ReadError(format!("Failed to read CSV headers: {}", e)))?
        .clone();
    
    let mut output = String::new();
    let end = end_row.unwrap_or(usize::MAX);
    let mut current_row = 0;
    
    // Add headers
    output.push_str("CSV Headers:\n");
    for (i, header) in headers.iter().enumerate() {
        if i > 0 {
            output.push_str(" | ");
        }
        output.push_str(header);
    }
    output.push('\n');
    output.push_str(&"-".repeat(50));
    output.push('\n');
    
    // Read requested rows
    for result in reader.records() {
        let record = result.map_err(|e| ReadError::ReadError(format!("Failed to read CSV row: {}", e)))?;
        
        if current_row >= start_row && current_row <= end {
            for (i, field) in record.iter().enumerate() {
                if i > 0 {
                    output.push_str(" | ");
                }
                output.push_str(field);
            }
            output.push('\n');
        }
        
        current_row += 1;
        
        if current_row > end {
            break;
        }
    }
    
    Ok(output)
}
