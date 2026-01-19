use crate::agent::tool::{Tool, ToolKind, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use std::error::Error as StdError;
use std::fs;

/// A tool for reading file contents.
pub struct FileReadTool;

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from the file system. Use this instead of 'cat' for better reliability and internal processing."
    }

    fn usage(&self) -> &str {
        "Provide the path to the file. Example: 'src/main.rs'"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to read."
                }
            },
            "required": ["path"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        // Try to parse as JSON (modern Tool Calling)
        let path = if let Ok(parsed) = serde_json::from_str::<ReadArgs>(args) {
            parsed.path
        } else {
            // Fallback for ReAct or plain string
            args.trim().trim_matches('"').to_string()
        };

        match fs::read_to_string(&path) {
            Ok(content) => Ok(ToolOutput::Immediate(serde_json::Value::String(content))),
            Err(e) => Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error reading file '{}': {}",
                path, e
            )))),
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}

/// A tool for writing/overwriting file contents.
pub struct FileWriteTool;

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write or overwrite a file with provided content. Use this for creating new files or updating existing ones."
    }

    fn usage(&self) -> &str {
        "Provide the path and the content. Example: { \"path\": \"test.txt\", \"content\": \"hello world\" }"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to write."
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        // Try to parse as JSON (modern Tool Calling)
        let (path, content) = if let Ok(parsed) = serde_json::from_str::<WriteArgs>(args) {
            (parsed.path, parsed.content)
        } else {
            // ReAct fallback is harder for multiple args, but we can try simple split for "path content"
            // though it's unreliable. We prefer JSON.
            return Ok(ToolOutput::Immediate(serde_json::Value::String(
                "Error: write_file requires structured JSON arguments: { \"path\": \"...\", \"content\": \"...\" }".to_string()
            )));
        };

        // Create parent directories if they don't exist
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                        "Error creating directory '{}': {}",
                        parent.display(),
                        e
                    ))));
                }
            }
        }

        match fs::write(&path, &content) {
            Ok(_) => Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Successfully wrote to file '{}'",
                path
            )))),
            Err(e) => Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                "Error writing to file '{}': {}",
                path, e
            )))),
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}
