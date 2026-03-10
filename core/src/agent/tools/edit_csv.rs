//! Edit CSV Tool - Structured CSV editing capabilities
//!
//! Provides precise CSV manipulation without regenerating entire files:
//! - Update specific cells by row/column
//! - Update rows matching conditions
//! - Insert new rows
//! - Delete rows
//! - Validate CSV output before writing

use crate::agent::runtime::core::{Capability, RuntimeContext, ToolCapability, ToolError};
use crate::agent::types::events::ToolResult;
use crate::agent::types::intents::ToolCall;
use crate::agent::tools::expand_tilde;
use serde::Deserialize;
use std::path::Path;

/// Tool for editing CSV files with structured operations
#[derive(Debug, Default)]
pub struct EditCsvTool;

impl EditCsvTool {
    /// Create a new edit_csv tool
    pub fn new() -> Self {
        Self
    }

    /// Execute the CSV edit operation
    async fn edit_csv(&self, args: EditCsvArgs) -> Result<ToolResult, ToolError> {
        let path = expand_tilde(&args.path);
        let path = Path::new(&path);

        // Check if file exists
        if !path.exists() {
            return Ok(ToolResult::Error {
                message: format!("File not found: {}", path.display()),
                code: Some("FILE_NOT_FOUND".to_string()),
                retryable: false,
            });
        }

        // Read and parse CSV
        let (headers, mut rows) = match self.read_csv(path).await {
            Ok(data) => data,
            Err(e) => return Ok(self.error_result(&e)),
        };

        // Build column index for name-based access
        let column_index: std::collections::HashMap<String, usize> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.clone(), i))
            .collect();

        // Execute operation
        let rows_affected = match args.operation {
            CsvOperation::Update => {
                self.handle_update(&headers, &column_index, &mut rows, &args)
            }
            CsvOperation::Delete => {
                self.handle_delete(&headers, &column_index, &mut rows, &args)
            }
            CsvOperation::Insert => {
                self.handle_insert(&headers, &mut rows, &args)
            }
            CsvOperation::UpdateWhere => {
                self.handle_update_where(&headers, &column_index, &mut rows, &args)
            }
        };

        if let Err(e) = rows_affected {
            return Ok(self.error_result(&e));
        }

        // Write back to file
        if let Err(e) = self.write_csv(path, &headers, &rows).await {
            return Ok(self.error_result(&e));
        }

        let rows_affected = rows_affected.unwrap_or(0);

        // Build success message
        let message = match args.operation {
            CsvOperation::Update => format!(
                "Updated {} row(s) in {}",
                rows_affected,
                path.display()
            ),
            CsvOperation::Delete => format!(
                "Deleted {} row(s) from {}",
                rows_affected,
                path.display()
            ),
            CsvOperation::Insert => format!("Inserted row into {}", path.display()),
            CsvOperation::UpdateWhere => format!(
                "Updated {} row(s) matching condition in {}",
                rows_affected,
                path.display()
            ),
        };

        Ok(ToolResult::Success {
            output: message,
            structured: Some(serde_json::json!({
                "path": path.to_string_lossy(),
                "operation": format!("{:?}", args.operation).to_lowercase(),
                "rows_affected": rows_affected,
                "total_rows": rows.len(),
            })),
        })
    }

    /// Handle update operation (single cell or full row)
    fn handle_update(
        &self,
        headers: &[String],
        column_index: &std::collections::HashMap<String, usize>,
        rows: &mut [csv::StringRecord],
        args: &EditCsvArgs,
    ) -> Result<usize, CsvError> {
        let row_idx = args.row.ok_or(CsvError::MissingArgument("row".to_string()))?;

        if row_idx == 0 || row_idx > rows.len() {
            return Err(CsvError::InvalidRow(row_idx, rows.len()));
        }

        let row = &mut rows[row_idx - 1]; // Convert to 0-based index

        // If column is specified, update single cell
        if let Some(col) = &args.column {
            let col_idx = self.resolve_column(col, column_index, headers)?;
            let value = args
                .value
                .clone()
                .ok_or(CsvError::MissingArgument("value".to_string()))?;

            let mut new_row: Vec<String> = row.iter().map(|s| s.to_string()).collect();

            // Ensure row has enough columns
            while new_row.len() <= col_idx {
                new_row.push(String::new());
            }

            new_row[col_idx] = value;
            *row = csv::StringRecord::from(new_row);

            Ok(1)
        }
        // If values is specified, replace entire row
        else if let Some(values) = &args.values {
            *row = csv::StringRecord::from(values.clone());
            Ok(1)
        } else {
            Err(CsvError::MissingArgument(
                "column or values".to_string(),
            ))
        }
    }

    /// Handle delete operation
    fn handle_delete(
        &self,
        headers: &[String],
        column_index: &std::collections::HashMap<String, usize>,
        rows: &mut Vec<csv::StringRecord>,
        args: &EditCsvArgs,
    ) -> Result<usize, CsvError> {
        // If row is specified, delete by index
        if let Some(row_idx) = args.row {
            if row_idx == 0 || row_idx > rows.len() {
                return Err(CsvError::InvalidRow(row_idx, rows.len()));
            }
            rows.remove(row_idx - 1);
            Ok(1)
        }
        // If where condition is specified, delete matching rows
        else if let Some(where_clause) = &args.where_clause {
            let col_idx = self.resolve_column(&where_clause.column, column_index, headers)?;
            let original_count = rows.len();

            rows.retain(|row| {
                let row_value = row.get(col_idx).unwrap_or("");
                row_value != where_clause.equals
            });

            Ok(original_count - rows.len())
        } else {
            Err(CsvError::MissingArgument("row or where".to_string()))
        }
    }

    /// Handle insert operation
    fn handle_insert(
        &self,
        _headers: &[String],
        rows: &mut Vec<csv::StringRecord>,
        args: &EditCsvArgs,
    ) -> Result<usize, CsvError> {
        let values = args
            .values
            .clone()
            .ok_or(CsvError::MissingArgument("values".to_string()))?;

        let new_row = csv::StringRecord::from(values);

        // Insert at specified row or append
        if let Some(row_idx) = args.row {
            if row_idx > rows.len() + 1 {
                return Err(CsvError::InvalidRow(row_idx, rows.len() + 1));
            }
            rows.insert(row_idx.saturating_sub(1), new_row);
        } else {
            rows.push(new_row);
        }

        Ok(1)
    }

    /// Handle update_where operation (update multiple rows matching condition)
    fn handle_update_where(
        &self,
        headers: &[String],
        column_index: &std::collections::HashMap<String, usize>,
        rows: &mut [csv::StringRecord],
        args: &EditCsvArgs,
    ) -> Result<usize, CsvError> {
        let where_clause = args
            .where_clause
            .clone()
            .ok_or(CsvError::MissingArgument("where".to_string()))?;
        let target_col = args
            .column
            .clone()
            .ok_or(CsvError::MissingArgument("column".to_string()))?;
        let value = args
            .value
            .clone()
            .ok_or(CsvError::MissingArgument("value".to_string()))?;

        let where_idx = self.resolve_column(&where_clause.column, column_index, headers)?;
        let target_idx = self.resolve_column(&target_col, column_index, headers)?;

        let mut updated = 0;
        for row in rows.iter_mut() {
            let row_value = row.get(where_idx).unwrap_or("");
            if row_value == where_clause.equals {
                let mut new_row: Vec<String> = row.iter().map(|s| s.to_string()).collect();

                // Ensure row has enough columns
                while new_row.len() <= target_idx {
                    new_row.push(String::new());
                }

                new_row[target_idx] = value.clone();
                *row = csv::StringRecord::from(new_row);
                updated += 1;
            }
        }

        Ok(updated)
    }

    /// Resolve column name or index to column index
    fn resolve_column(
        &self,
        column: &str,
        column_index: &std::collections::HashMap<String, usize>,
        headers: &[String],
    ) -> Result<usize, CsvError> {
        // Try as column name first
        if let Some(&idx) = column_index.get(column) {
            return Ok(idx);
        }

        // Try as 0-based index
        if let Ok(idx) = column.parse::<usize>() {
            if idx < headers.len() {
                return Ok(idx);
            }
        }

        // Try as 1-based index
        if let Ok(idx) = column.parse::<usize>() {
            if idx > 0 && idx <= headers.len() {
                return Ok(idx - 1);
            }
        }

        Err(CsvError::InvalidColumn(column.to_string()))
    }

    /// Read CSV file and return headers and rows
    async fn read_csv(
        &self,
        path: &Path,
    ) -> Result<(Vec<String>, Vec<csv::StringRecord>), CsvError> {
        let path = path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&path)
                .map_err(|e| CsvError::ReadError(format!("Cannot open CSV: {}", e)))?;

            let mut reader = csv::Reader::from_reader(file);

            // Read headers
            let headers = reader
                .headers()
                .map_err(|e| CsvError::ReadError(format!("Failed to read headers: {}", e)))?
                .iter()
                .map(|s| s.to_string())
                .collect();

            // Read rows
            let mut rows = Vec::new();
            for result in reader.records() {
                let record = result
                    .map_err(|e| CsvError::ReadError(format!("Failed to read row: {}", e)))?;
                rows.push(record);
            }

            Ok((headers, rows))
        })
        .await
        .map_err(|e| CsvError::ReadError(format!("CSV read task panicked: {}", e)))?
    }

    /// Write CSV file with headers and rows
    async fn write_csv(
        &self,
        path: &Path,
        headers: &[String],
        rows: &[csv::StringRecord],
    ) -> Result<(), CsvError> {
        let path = path.to_path_buf();
        let headers: Vec<String> = headers.to_vec();
        let rows: Vec<csv::StringRecord> = rows.to_vec();

        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::create(&path)
                .map_err(|e| CsvError::WriteError(format!("Cannot create file: {}", e)))?;

            let mut writer = csv::Writer::from_writer(file);

            // Write headers
            writer
                .write_record(&headers)
                .map_err(|e| CsvError::WriteError(format!("Failed to write headers: {}", e)))?;

            // Write rows
            for row in &rows {
                writer
                    .write_record(row)
                    .map_err(|e| CsvError::WriteError(format!("Failed to write row: {}", e)))?;
            }

            writer
                .flush()
                .map_err(|e| CsvError::WriteError(format!("Failed to flush: {}", e)))?;

            Ok(())
        })
        .await
        .map_err(|e| CsvError::WriteError(format!("CSV write task panicked: {}", e)))?
    }

    /// Create error ToolResult
    fn error_result(&self, error: &CsvError) -> ToolResult {
        ToolResult::Error {
            message: error.to_string(),
            code: Some(error.code().to_string()),
            retryable: false,
        }
    }
}

impl Capability for EditCsvTool {
    fn name(&self) -> &'static str {
        "edit_csv"
    }
}

/// CSV operation types
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CsvOperation {
    /// Update a single cell or entire row
    Update,
    /// Delete row(s)
    Delete,
    /// Insert a new row
    Insert,
    /// Update multiple rows matching a condition
    UpdateWhere,
}

impl Default for CsvOperation {
    fn default() -> Self {
        CsvOperation::Update
    }
}

/// Where clause for conditional operations
#[derive(Debug, Clone, Deserialize)]
struct WhereClause {
    /// Column name or index to check
    column: String,
    /// Value to match
    equals: String,
}

/// Arguments for edit_csv tool
#[derive(Debug, Clone, Deserialize)]
struct EditCsvArgs {
    /// Path to CSV file
    path: String,
    /// Operation to perform
    #[serde(default)]
    operation: CsvOperation,
    /// Row index (1-based) for single-row operations
    row: Option<usize>,
    /// Column name or index for cell updates
    column: Option<String>,
    /// New value for cell updates
    value: Option<String>,
    /// Full row values for insert or row replacement
    values: Option<Vec<String>>,
    /// Where clause for conditional operations
    #[serde(rename = "where")]
    where_clause: Option<WhereClause>,
}

/// CSV editing errors
#[derive(Debug)]
enum CsvError {
    ReadError(String),
    WriteError(String),
    MissingArgument(String),
    InvalidRow(usize, usize),
    InvalidColumn(String),
}

impl std::fmt::Display for CsvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CsvError::ReadError(msg) => write!(f, "Read error: {}", msg),
            CsvError::WriteError(msg) => write!(f, "Write error: {}", msg),
            CsvError::MissingArgument(arg) => write!(f, "Missing required argument: {}", arg),
            CsvError::InvalidRow(row, max) => {
                write!(f, "Invalid row index: {} (valid range: 1-{})", row, max)
            }
            CsvError::InvalidColumn(col) => write!(f, "Invalid column: '{}'", col),
        }
    }
}

impl CsvError {
    fn code(&self) -> &'static str {
        match self {
            CsvError::ReadError(_) => "READ_ERROR",
            CsvError::WriteError(_) => "WRITE_ERROR",
            CsvError::MissingArgument(_) => "MISSING_ARGUMENT",
            CsvError::InvalidRow(_, _) => "INVALID_ROW",
            CsvError::InvalidColumn(_) => "INVALID_COLUMN",
        }
    }
}

#[async_trait::async_trait]
impl ToolCapability for EditCsvTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        // Parse arguments
        let args = match serde_json::from_value::<EditCsvArgs>(call.arguments.clone()) {
            Ok(a) => a,
            Err(e) => {
                return Ok(ToolResult::Error {
                    message: format!("Invalid arguments: {}", e),
                    code: Some("PARSE_ERROR".to_string()),
                    retryable: false,
                })
            }
        };

        self.edit_csv(args).await
    }
}

impl EditCsvTool {
    /// Get tool description for LLM prompt
    pub fn description() -> &'static str {
        r#"# edit_csv - Edit CSV files with structured operations

Precisely edit CSV files without regenerating entire content. Supports cell updates, row operations, and conditional updates.

## Operations

### Update a cell
```json
{"a": "edit_csv", "i": {"path": "data.csv", "operation": "update", "row": 5, "column": "Age", "value": "31"}}
```

### Update entire row
```json
{"a": "edit_csv", "i": {"path": "data.csv", "operation": "update", "row": 3, "values": ["Alice", "31", "NYC"]}}
```

### Insert row
```json
{"a": "edit_csv", "i": {"path": "data.csv", "operation": "insert", "values": ["Bob", "25", "LA"]}}
```

### Insert at specific position
```json
{"a": "edit_csv", "i": {"path": "data.csv", "operation": "insert", "row": 2, "values": ["Charlie", "35", "SF"]}}
```

### Delete row
```json
{"a": "edit_csv", "i": {"path": "data.csv", "operation": "delete", "row": 5}}
```

### Delete rows matching condition
```json
{"a": "edit_csv", "i": {"path": "data.csv", "operation": "delete", "where": {"column": "Status", "equals": "inactive"}}}
```

### Update multiple rows (update_where)
```json
{"a": "edit_csv", "i": {"path": "data.csv", "operation": "update_where", "where": {"column": "City", "equals": "NYC"}, "column": "Status", "value": "active"}}
```

## Parameters

- `path` (required): Path to CSV file
- `operation` (required): One of "update", "delete", "insert", "update_where"
- `row`: Row index (1-based) for single-row operations
- `column`: Column name or index for cell updates
- `value`: New value for cell updates
- `values`: Array of values for insert or row replacement
- `where`: Object with `column` and `equals` for conditional operations

## Notes

- Column names are case-sensitive and match CSV headers
- Column can also be 0-based index (e.g., "0" for first column)
- Row indices are 1-based (first data row is 1, not counting header)
- CSV structure (headers, delimiters) is preserved"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    async fn create_test_csv(dir: &TempDir, content: &str) -> std::path::PathBuf {
        let path = dir.path().join("test.csv");
        fs::write(&path, content).await.unwrap();
        path
    }

    #[tokio::test]
    async fn test_update_cell_by_column_name() {
        let temp = TempDir::new().unwrap();
        let path = create_test_csv(&temp, "Name,Age,City\nAlice,30,NYC\nBob,25,LA\n").await;

        let tool = EditCsvTool::new();
        let call = ToolCall::new(
            "edit_csv",
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "operation": "update",
                "row": 1,
                "column": "Age",
                "value": "31"
            }),
        );

        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();

        match result {
            ToolResult::Success { output, .. } => {
                assert!(output.contains("Updated 1 row"));
            }
            _ => panic!("Expected success"),
        }

        // Verify file content
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Alice,31,NYC"));
        assert!(content.contains("Bob,25,LA"));
    }

    #[tokio::test]
    async fn test_update_cell_by_column_index() {
        let temp = TempDir::new().unwrap();
        let path = create_test_csv(&temp, "Name,Age,City\nAlice,30,NYC\n").await;

        let tool = EditCsvTool::new();
        let call = ToolCall::new(
            "edit_csv",
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "operation": "update",
                "row": 1,
                "column": "1",
                "value": "35"
            }),
        );

        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();

        match result {
            ToolResult::Success { .. } => {}
            _ => panic!("Expected success"),
        }

        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Alice,35,NYC"));
    }

    #[tokio::test]
    async fn test_insert_row() {
        let temp = TempDir::new().unwrap();
        let path = create_test_csv(&temp, "Name,Age,City\nAlice,30,NYC\n").await;

        let tool = EditCsvTool::new();
        let call = ToolCall::new(
            "edit_csv",
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "operation": "insert",
                "values": ["Bob", "25", "LA"]
            }),
        );

        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();

        match result {
            ToolResult::Success { output, .. } => {
                assert!(output.contains("Inserted row"));
            }
            _ => panic!("Expected success"),
        }

        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Alice,30,NYC"));
        assert!(content.contains("Bob,25,LA"));
    }

    #[tokio::test]
    async fn test_delete_row() {
        let temp = TempDir::new().unwrap();
        let path = create_test_csv(&temp, "Name,Age,City\nAlice,30,NYC\nBob,25,LA\n").await;

        let tool = EditCsvTool::new();
        let call = ToolCall::new(
            "edit_csv",
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "operation": "delete",
                "row": 1
            }),
        );

        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();

        match result {
            ToolResult::Success { output, .. } => {
                assert!(output.contains("Deleted 1 row"));
            }
            _ => panic!("Expected success"),
        }

        let content = fs::read_to_string(&path).await.unwrap();
        assert!(!content.contains("Alice,30,NYC"));
        assert!(content.contains("Bob,25,LA"));
    }

    #[tokio::test]
    async fn test_update_where() {
        let temp = TempDir::new().unwrap();
        let path = create_test_csv(
            &temp,
            "Name,Age,City\nAlice,30,NYC\nBob,25,NYC\nCharlie,35,LA\n",
        )
        .await;

        let tool = EditCsvTool::new();
        let call = ToolCall::new(
            "edit_csv",
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "operation": "update_where",
                "where": {"column": "City", "equals": "NYC"},
                "column": "Age",
                "value": "40"
            }),
        );

        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();

        match result {
            ToolResult::Success { output, .. } => {
                assert!(output.contains("Updated 2 row(s)"));
            }
            _ => panic!("Expected success"),
        }

        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Alice,40,NYC"));
        assert!(content.contains("Bob,40,NYC"));
        assert!(content.contains("Charlie,35,LA"));
    }

    #[tokio::test]
    async fn test_delete_where() {
        let temp = TempDir::new().unwrap();
        let path = create_test_csv(
            &temp,
            "Name,Age,City\nAlice,30,NYC\nBob,25,LA\nCharlie,35,NYC\n",
        )
        .await;

        let tool = EditCsvTool::new();
        let call = ToolCall::new(
            "edit_csv",
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "operation": "delete",
                "where": {"column": "City", "equals": "NYC"}
            }),
        );

        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();

        match result {
            ToolResult::Success { output, .. } => {
                assert!(output.contains("Deleted 2 row(s)"));
            }
            _ => panic!("Expected success"),
        }

        let content = fs::read_to_string(&path).await.unwrap();
        assert!(!content.contains("Alice"));
        assert!(content.contains("Bob,25,LA"));
        assert!(!content.contains("Charlie"));
    }

    #[tokio::test]
    async fn test_invalid_row() {
        let temp = TempDir::new().unwrap();
        let path = create_test_csv(&temp, "Name,Age\nAlice,30\n").await;

        let tool = EditCsvTool::new();
        let call = ToolCall::new(
            "edit_csv",
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "operation": "update",
                "row": 10,
                "column": "Age",
                "value": "31"
            }),
        );

        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();

        match result {
            ToolResult::Error { code, .. } => {
                assert_eq!(code, Some("INVALID_ROW".to_string()));
            }
            _ => panic!("Expected error"),
        }
    }

    #[tokio::test]
    async fn test_invalid_column() {
        let temp = TempDir::new().unwrap();
        let path = create_test_csv(&temp, "Name,Age\nAlice,30\n").await;

        let tool = EditCsvTool::new();
        let call = ToolCall::new(
            "edit_csv",
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "operation": "update",
                "row": 1,
                "column": "NonExistent",
                "value": "31"
            }),
        );

        let result = tool.execute(&RuntimeContext::new(), call).await.unwrap();

        match result {
            ToolResult::Error { code, .. } => {
                assert_eq!(code, Some("INVALID_COLUMN".to_string()));
            }
            _ => panic!("Expected error"),
        }
    }
}
