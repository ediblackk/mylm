//! Shell utility tools for agents.
//!
//! Provides native implementations of common shell utilities:
//! - tail: Show last lines of a file
//! - wc: Count lines, words, and bytes in files
//! - grep: Search for patterns in files
//! - du: Estimate file space usage

use crate::agent_old::tool::{Tool, ToolKind, ToolOutput};
use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::error::Error as StdError;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Expand tilde (~) to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        if let Some(home) = dirs::home_dir() {
            let rest = &path[1..]; // Remove the leading '~'
            return home.join(rest.trim_start_matches('/')).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

// ============================================================================
// Tail Tool - Show last lines of a file
// ============================================================================

/// A tool for showing the last lines of a file (like Unix tail).
pub struct TailTool;

#[derive(Deserialize)]
struct TailParams {
    path: String,
    #[serde(default = "default_lines")]
    lines: usize,
}

fn default_lines() -> usize {
    10
}

#[derive(Serialize)]
struct TailResult {
    path: String,
    lines: Vec<String>,
    total_lines_shown: usize,
    file_exists: bool,
}

#[async_trait]
impl Tool for TailTool {
    fn name(&self) -> &str {
        "tail"
    }

    fn description(&self) -> &str {
        "Display the last part of a file. Shows the final N lines (default 10)."
    }

    fn usage(&self) -> &str {
        "Provide a file path and optionally the number of lines. Example: { \"path\": \"log.txt\", \"lines\": 20 }"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read."
                },
                "lines": {
                    "type": "integer",
                    "description": "Number of lines to show from the end (default: 10).",
                    "minimum": 1,
                    "maximum": 10000
                }
            },
            "required": ["path"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let params = if let Ok(parsed) = serde_json::from_str::<TailParams>(args) {
            parsed
        } else {
            // Fallback for plain string path
            TailParams {
                path: args.trim().trim_matches('"').to_string(),
                lines: 10,
            }
        };

        let path_str = expand_tilde(&params.path);
        let path = Path::new(&path_str);

        if !path.exists() {
            return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error: File '{}' does not exist",
                params.path
            ))));
        }

        if !path.is_file() {
            return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error: Path '{}' is not a file",
                params.path
            ))));
        }

        let file = match fs::File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                    "Error opening file '{}': {}",
                    params.path, e
                ))));
            }
        };

        let reader = BufReader::new(file);
        let mut lines: VecDeque<String> = VecDeque::with_capacity(params.lines);

        for line_result in reader.lines() {
            if let Ok(line) = line_result {
                if lines.len() >= params.lines {
                    lines.pop_front();
                }
                lines.push_back(line);
            }
        }

        let result = TailResult {
            path: params.path.clone(),
            lines: lines.into_iter().collect(),
            total_lines_shown: params.lines,
            file_exists: true,
        };

        Ok(ToolOutput::Immediate(serde_json::to_value(result)?))
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}

// ============================================================================
// Word Count Tool - Count lines, words, and bytes
// ============================================================================

/// A tool for counting lines, words, and bytes in files (like Unix wc).
pub struct WordCountTool;

#[derive(Deserialize)]
struct WordCountParams {
    path: String,
}

#[derive(Serialize)]
struct WordCountResult {
    path: String,
    lines: usize,
    words: usize,
    bytes: u64,
    characters: usize,
}

#[async_trait]
impl Tool for WordCountTool {
    fn name(&self) -> &str {
        "wc"
    }

    fn description(&self) -> &str {
        "Count lines, words, and bytes in a file (like Unix wc command)."
    }

    fn usage(&self) -> &str {
        "Provide a file path. Example: { \"path\": \"document.txt\" } or just \"document.txt\""
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to analyze."
                }
            },
            "required": ["path"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let params = if let Ok(parsed) = serde_json::from_str::<WordCountParams>(args) {
            parsed
        } else {
            WordCountParams {
                path: args.trim().trim_matches('"').to_string(),
            }
        };

        let path_str = expand_tilde(&params.path);
        let path = Path::new(&path_str);

        if !path.exists() {
            return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error: File '{}' does not exist",
                path_str
            ))));
        }

        if !path.is_file() {
            return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error: Path '{}' is not a file",
                params.path
            ))));
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                    "Error reading file '{}': {}",
                    params.path, e
                ))));
            }
        };

        let lines = content.lines().count();
        let words = content.split_whitespace().count();
        let bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let characters = content.chars().count();

        let result = WordCountResult {
            path: params.path,
            lines,
            words,
            bytes,
            characters,
        };

        Ok(ToolOutput::Immediate(serde_json::to_value(result)?))
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}

// ============================================================================
// Grep Tool - Search for patterns in files
// ============================================================================

/// A tool for searching text patterns in files (like Unix grep).
pub struct GrepTool;

#[derive(Deserialize)]
struct GrepParams {
    pattern: String,
    path: String,
    #[serde(default)]
    case_insensitive: bool,
    #[serde(default = "default_context")]
    context_lines: usize,
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_context() -> usize {
    0
}

fn default_max_results() -> usize {
    100
}

#[derive(Serialize)]
struct GrepMatch {
    line_number: usize,
    content: String,
    before_context: Vec<String>,
    after_context: Vec<String>,
}

#[derive(Serialize)]
struct GrepResult {
    pattern: String,
    path: String,
    matches: Vec<GrepMatch>,
    total_matches: usize,
    truncated: bool,
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search for patterns in files (like Unix grep). Supports case-insensitive search and context lines."
    }

    fn usage(&self) -> &str {
        "Provide a pattern and file path. Example: { \"pattern\": \"TODO\", \"path\": \"src/main.rs\", \"case_insensitive\": true }"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for."
                },
                "path": {
                    "type": "string",
                    "description": "Path to the file to search."
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Perform case-insensitive search (default: false)."
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines to show before and after each match (default: 0).",
                    "minimum": 0,
                    "maximum": 10
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches to return (default: 100).",
                    "minimum": 1,
                    "maximum": 1000
                }
            },
            "required": ["pattern", "path"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let params = match serde_json::from_str::<GrepParams>(args) {
            Ok(p) => p,
            Err(_) => {
                return Ok(ToolOutput::Immediate(serde_json::Value::String(
                    "Error: grep requires JSON arguments: { \"pattern\": \"...\", \"path\": \"...\" }".to_string()
                )));
            }
        };

        let path_str = expand_tilde(&params.path);
        let path = Path::new(&path_str);

        if !path.exists() {
            return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error: File '{}' does not exist",
                path_str
            ))));
        }

        if !path.is_file() {
            return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error: Path '{}' is not a file",
                params.path
            ))));
        }

        // Compile regex pattern
        let regex_result = if params.case_insensitive {
            Regex::new(&format!("(?i){}", regex::escape(&params.pattern)))
        } else {
            Regex::new(&regex::escape(&params.pattern))
        };

        let regex = match regex_result {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                    "Error: Invalid regex pattern '{}': {}",
                    params.pattern, e
                ))));
            }
        };

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                    "Error reading file '{}': {}",
                    params.path, e
                ))));
            }
        };

        let lines: Vec<&str> = content.lines().collect();
        let mut matches = Vec::new();
        let mut total_matches = 0;
        let truncated;

        for (line_num, line) in lines.iter().enumerate() {
            if regex.is_match(line) {
                total_matches += 1;

                if matches.len() < params.max_results {
                    let before_context = if params.context_lines > 0 {
                        let start = line_num.saturating_sub(params.context_lines);
                        lines[start..line_num]
                            .iter()
                            .map(|s| s.to_string())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    let after_context = if params.context_lines > 0 {
                        let end = (line_num + params.context_lines + 1).min(lines.len());
                        lines[line_num + 1..end]
                            .iter()
                            .map(|s| s.to_string())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    matches.push(GrepMatch {
                        line_number: line_num + 1, // 1-indexed
                        content: line.to_string(),
                        before_context,
                        after_context,
                    });
                }
            }
        }

        truncated = total_matches > params.max_results;

        let result = GrepResult {
            pattern: params.pattern,
            path: params.path,
            matches,
            total_matches,
            truncated,
        };

        Ok(ToolOutput::Immediate(serde_json::to_value(result)?))
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}

// ============================================================================
// Disk Usage Tool - Estimate file space usage
// ============================================================================

/// A tool for estimating file space usage (like Unix du).
pub struct DiskUsageTool;

#[derive(Deserialize)]
struct DiskUsageParams {
    path: String,
    #[serde(default)]
    recursive: bool,
    #[serde(default = "default_max_depth")]
    max_depth: usize,
}

fn default_max_depth() -> usize {
    3
}

#[derive(Serialize)]
struct DiskUsageEntry {
    path: String,
    size_bytes: u64,
    size_human: String,
    is_dir: bool,
    file_count: Option<usize>,
    dir_count: Option<usize>,
}

#[derive(Serialize)]
struct DiskUsageResult {
    path: String,
    entries: Vec<DiskUsageEntry>,
    total_size_bytes: u64,
    total_size_human: String,
    total_files: usize,
    total_dirs: usize,
}

#[async_trait]
impl Tool for DiskUsageTool {
    fn name(&self) -> &str {
        "du"
    }

    fn description(&self) -> &str {
        "Estimate file space usage (like Unix du). Shows directory and file sizes."
    }

    fn usage(&self) -> &str {
        "Provide a path. Example: { \"path\": \"src\", \"recursive\": true, \"max_depth\": 2 }"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory or file to analyze."
                },
                "recursive": {
                    "type": "boolean",
                    "description": "Calculate sizes recursively for subdirectories (default: false)."
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum recursion depth when recursive is true (default: 3).",
                    "minimum": 1,
                    "maximum": 10
                }
            },
            "required": ["path"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let params = if let Ok(parsed) = serde_json::from_str::<DiskUsageParams>(args) {
            parsed
        } else {
            DiskUsageParams {
                path: args.trim().trim_matches('"').to_string(),
                recursive: false,
                max_depth: 3,
            }
        };

        let path_str = expand_tilde(&params.path);
        let path = Path::new(&path_str);

        if !path.exists() {
            return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error: Path '{}' does not exist",
                params.path
            ))));
        }

        let mut entries = Vec::new();
        let max_depth = if params.recursive { params.max_depth } else { 0 };
        let (total_size, total_files, total_dirs) = if path.is_file() {
            let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            entries.push(DiskUsageEntry {
                path: params.path.clone(),
                size_bytes: size,
                size_human: format_size(size),
                is_dir: false,
                file_count: Some(1),
                dir_count: Some(0),
            });
            (size, 1, 0)
        } else {
            Self::calculate_dir_size(path, 0, max_depth, &mut entries)?
        };

        // Sort by size descending
        entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

        let result = DiskUsageResult {
            path: params.path,
            entries,
            total_size_bytes: total_size,
            total_size_human: format_size(total_size),
            total_files,
            total_dirs,
        };

        Ok(ToolOutput::Immediate(serde_json::to_value(result)?))
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}

impl DiskUsageTool {
    fn calculate_dir_size(
        path: &Path,
        current_depth: usize,
        max_depth: usize,
        entries: &mut Vec<DiskUsageEntry>,
    ) -> Result<(u64, usize, usize), Box<dyn StdError + Send + Sync>> {
        let mut total_size: u64 = 0;
        let mut file_count = 0;
        let mut dir_count = 0;

        match fs::read_dir(path) {
            Ok(read_dir) => {
                for entry_result in read_dir {
                    if let Ok(entry) = entry_result {
                        let entry_path = entry.path();
                        let metadata = match entry.metadata() {
                            Ok(m) => m,
                            Err(_) => continue,
                        };

                        if metadata.is_file() {
                            let size = metadata.len();
                            total_size += size;
                            file_count += 1;
                        } else if metadata.is_dir() {
                            dir_count += 1;
                            
                            if current_depth < max_depth {
                                let (sub_size, sub_files, sub_dirs) = 
                                    Self::calculate_dir_size(&entry_path, current_depth + 1, max_depth, entries)?;
                                total_size += sub_size;
                                file_count += sub_files;
                                dir_count += sub_dirs;
                            }
                        }
                    }
                }

                // Add entry for this directory
                entries.push(DiskUsageEntry {
                    path: path.to_string_lossy().to_string(),
                    size_bytes: total_size,
                    size_human: format_size(total_size),
                    is_dir: true,
                    file_count: Some(file_count),
                    dir_count: Some(dir_count),
                });

                Ok((total_size, file_count, dir_count))
            }
            Err(e) => Err(format!("Error reading directory '{}': {}", path.display(), e).into()),
        }
    }
}

/// Format bytes to human-readable string
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    
    if bytes == 0 {
        return "0 B".to_string();
    }

    let bytes_f = bytes as f64;
    let unit_idx = (bytes_f.log10() / 1024_f64.log10()).min(UNITS.len() as f64 - 1.0) as usize;
    let size = bytes_f / 1024_f64.powi(unit_idx as i32);

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
    }
}
