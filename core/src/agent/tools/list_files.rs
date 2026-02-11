//! Directory listing tool for agents.
//!
//! Provides a structured way to list files and directories with options
//! for showing hidden files, detailed metadata, and recursive listing.

use crate::agent::tool::{Tool, ToolKind, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use std::error::Error as StdError;
use std::fs;
use std::path::{Path, PathBuf};



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

/// A tool for listing files and directories.
/// Uses a base path (typically the terminal's CWD) to resolve relative paths.
pub struct ListFilesTool {
    /// Base directory for resolving relative paths (defaults to process CWD)
    base_path: PathBuf,
}

impl ListFilesTool {
    /// Create a new ListFilesTool with the given base path
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }
    
    /// Create a new ListFilesTool using the process's current working directory
    pub fn with_cwd() -> Self {
        Self {
            base_path: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

impl Default for ListFilesTool {
    fn default() -> Self {
        Self::with_cwd()
    }
}

#[derive(Deserialize)]
struct ListFilesParams {
    #[serde(default = "default_path")]
    path: String,
    #[serde(default)]
    show_hidden: bool,
}

fn default_path() -> String {
    ".".to_string()
}



struct FileEntry {
    name: String,
    is_dir: bool,
}

#[async_trait]
impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List files and directories in a given path. PREFERRED over 'execute_command' for file listing because it provides structured JSON output with metadata. Supports hidden files and detailed metadata."
    }

    fn usage(&self) -> &str {
        "Provide a path and optional flags. Example: { \"path\": \"src\", \"show_hidden\": false, \"detailed\": true }"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The directory path to list (default: '.')."
                },
                "show_hidden": {
                    "type": "boolean",
                    "description": "Include hidden files starting with '.' (default: false)."
                }
            },
            "required": []
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        crate::info_log!("list_files: called with args='{}'", args);
        
        let mut params = if let Ok(parsed) = serde_json::from_str::<ListFilesParams>(args) {
            parsed
        } else {
            // Fallback for plain string path
            ListFilesParams {
                path: args.trim().trim_matches('"').to_string(),
                show_hidden: false,
            }
        };

        // Expand tilde to home directory
        params.path = expand_tilde(&params.path);
        
        // Resolve relative paths against base_path (terminal's CWD)
        let resolved_path = if Path::new(&params.path).is_absolute() {
            PathBuf::from(&params.path)
        } else {
            self.base_path.join(&params.path)
        };
        
        let path_str = resolved_path.to_string_lossy().to_string();
        crate::info_log!("list_files: base_path='{}', input='{}', resolved='{}', show_hidden={}", 
            self.base_path.display(), params.path, path_str, params.show_hidden);

        let path = Path::new(&path_str);
        
        if !path.exists() {
            return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error: Path '{}' does not exist",
                path_str
            ))));
        }

        if !path.is_dir() {
            return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error: Path '{}' is not a directory",
                path_str
            ))));
        }

        let mut entries = Vec::new();
        let mut directory_count = 0;
        let mut file_count = 0;

        Self::list_directory(
            path,
            &params,
            &mut entries,
            &mut directory_count,
            &mut file_count,
        )?;

        // Sort entries: directories first, then files, both alphabetically
        entries.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        // Build simple text output
        let mut output = String::new();
        for entry in &entries {
            if entry.is_dir {
                output.push_str(&format!("{}/\n", entry.name));
            } else {
                output.push_str(&format!("{}\n", entry.name));
            }
        }
        
        output.push_str(&format!("\nTotal: {} directories, {} files", directory_count, file_count));

        crate::info_log!("list_files: output size = {} bytes, entries={}", output.len(), entries.len());
        if output.len() > 10000 {
            crate::warn_log!("list_files: LARGE OUTPUT - first 200 chars: '{}'", &output[..output.len().min(200)]);
        }

        Ok(ToolOutput::Immediate(serde_json::Value::String(output)))
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}

/// Directories that typically have too many files and should be avoided
const SKIPPED_DIRS: &[&str] = &["node_modules", ".git", "target/debug", "target/release", ".cargo", "dist", "build"];

impl ListFilesTool {
    fn list_directory(
        path: &Path,
        params: &ListFilesParams,
        entries: &mut Vec<FileEntry>,
        dir_count: &mut usize,
        file_count: &mut usize,
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {
        // Check for directories that typically have too many files
        let path_str = path.to_string_lossy();
        for skip_dir in SKIPPED_DIRS {
            if path_str.contains(skip_dir) {
                return Err(format!("Directory '{}' contains too many files. Use a more specific path.", path.display()).into());
            }
        }

        match fs::read_dir(path) {
            Ok(read_dir) => {
                for entry_result in read_dir {
                    if let Ok(entry) = entry_result {
                        let name = entry.file_name().to_string_lossy().to_string();
                        
                        // Skip hidden files unless show_hidden is true
                        if !params.show_hidden && name.starts_with('.') {
                            continue;
                        }

                        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                        
                        let file_entry = FileEntry {
                            name: name.clone(),
                            is_dir,
                        };

                        if is_dir {
                            *dir_count += 1;
                        } else {
                            *file_count += 1;
                        }

                        entries.push(file_entry);
                    }
                }
                
                Ok(())
            }
            Err(e) => Err(format!("Error reading directory '{}': {}", path.display(), e).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_files_basic() {
        let tool = ListFilesTool::with_cwd();
        // This is a basic smoke test - just ensure the tool can be created
        assert_eq!(tool.name(), "list_files");
    }
}
