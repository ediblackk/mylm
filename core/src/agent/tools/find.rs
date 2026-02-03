use crate::agent::tool::{Tool, ToolKind, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use ignore::WalkBuilder;
use glob::Pattern;
use chrono::{DateTime, Utc};

/// A tool for searching files and directories matching a pattern.
pub struct FindTool;

#[derive(Deserialize)]
struct FindParams {
    #[serde(default = "default_path")]
    path: String,
    pattern: String,
    #[serde(default = "default_file_type")]
    file_type: String,
}

fn default_path() -> String {
    ".".to_string()
}

fn default_file_type() -> String {
    "both".to_string()
}

#[derive(Serialize)]
struct FindResult {
    path: String,
    is_dir: bool,
    size: u64,
    modified: String,
}

#[async_trait]
impl Tool for FindTool {
    fn name(&self) -> &str {
        "find"
    }

    fn description(&self) -> &str {
        "Search for files and directories matching a glob pattern. Respects .gitignore by default."
    }

    fn usage(&self) -> &str {
        "Provide a pattern and optionally a path and file_type. Example: { \"pattern\": \"*.rs\", \"path\": \"src\", \"file_type\": \"file\" }"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The directory to search in (default: '.')."
                },
                "pattern": {
                    "type": "string",
                    "description": "The glob pattern to match against filenames (e.g., '*.ts', '**/config.json')."
                },
                "file_type": {
                    "type": "string",
                    "enum": ["file", "dir", "both"],
                    "description": "The type of entries to return (default: 'both')."
                }
            },
            "required": ["pattern"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let params = if let Ok(parsed) = serde_json::from_str::<FindParams>(args) {
            parsed
        } else {
            // Fallback for ReAct or plain string
            FindParams {
                path: ".".to_string(),
                pattern: args.trim().trim_matches('"').to_string(),
                file_type: "both".to_string(),
            }
        };

        let pattern_str = if params.pattern.contains('/') {
            params.pattern.clone()
        } else {
            format!("**/{}", params.pattern)
        };

        let pattern = Pattern::new(&pattern_str)?;
        let mut results = Vec::new();

        let walker = WalkBuilder::new(&params.path)
            .hidden(false)
            .git_ignore(true)
            .build();

        for result in walker {
            if let Ok(entry) = result {
                let path = entry.path();
                
                // For matching, we use the relative path from the search root if possible
                let match_path = if let Ok(rel_path) = path.strip_prefix(&params.path) {
                    rel_path
                } else {
                    path
                };

                if pattern.matches_path(match_path) {
                    let metadata = entry.metadata().ok();
                    let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    
                    // Filter by file_type
                    match params.file_type.as_str() {
                        "file" if is_dir => continue,
                        "dir" if !is_dir => continue,
                        _ => {}
                    }

                    let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                    let modified = metadata.as_ref()
                        .and_then(|m| m.modified().ok())
                        .map(|t| DateTime::<Utc>::from(t).to_rfc3339())
                        .unwrap_or_else(|| "unknown".to_string());

                    results.push(FindResult {
                        path: path.to_string_lossy().to_string(),
                        is_dir,
                        size,
                        modified,
                    });
                }
            }
            
            // Limit results to 1000 to prevent context overflow
            if results.len() >= 1000 {
                break;
            }
        }

        Ok(ToolOutput::Immediate(serde_json::to_value(results)?))
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}
