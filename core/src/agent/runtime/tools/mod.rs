//! Tool implementations for the agent runtime
//!
//! This module provides concrete tool implementations that execute
//! actions on behalf of the agent. All tools implement the `ToolCapability` trait.

pub mod shell;
pub mod fs;
pub mod list_files;
pub mod git;
pub mod web_search;
pub mod memory;

pub use shell::ShellTool;
pub use fs::{ReadFileTool, WriteFileTool};
pub use list_files::ListFilesTool;
pub use git::{GitStatusTool, GitLogTool, GitDiffTool};
pub use web_search::{WebSearchTool, WebSearchConfig, SearchProvider};
pub use memory::MemoryTool;

use std::sync::Arc;
use crate::agent::runtime::capability::{Capability, ToolCapability};
use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::ToolError;
use crate::agent::runtime::terminal::{TerminalExecutor, DefaultTerminalExecutor};
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use crate::memory::store::VectorStore;

/// Tool registry that combines all available tools
pub struct ToolRegistry {
    shell: ShellTool,
    read_file: ReadFileTool,
    write_file: WriteFileTool,
    list_files: ListFilesTool,
    git_status: GitStatusTool,
    git_log: GitLogTool,
    git_diff: GitDiffTool,
    web_search: WebSearchTool,
    memory: Option<MemoryTool>,
    terminal: Arc<dyn TerminalExecutor>,
}

impl ToolRegistry {
    /// Create a new tool registry with all default tools
    /// 
    /// Uses a default terminal executor (std::process::Command).
    /// Use `with_terminal()` to provide a custom terminal executor (e.g., PTY-based).
    pub fn new() -> Self {
        Self {
            shell: ShellTool::new(),
            read_file: ReadFileTool::new(),
            write_file: WriteFileTool::new(),
            list_files: ListFilesTool::new(),
            git_status: GitStatusTool::new(),
            git_log: GitLogTool::new(),
            git_diff: GitDiffTool::new(),
            web_search: WebSearchTool::new(),
            memory: None,
            terminal: Arc::new(DefaultTerminalExecutor::new()),
        }
    }
    
    /// Set a custom terminal executor
    /// 
    /// This allows the TUI to provide a PTY-based terminal executor
    /// so agent commands run in the shared terminal session.
    pub fn with_terminal(mut self, terminal: Arc<dyn TerminalExecutor>) -> Self {
        self.terminal = terminal;
        self
    }
    
    /// Enable memory tool with a VectorStore
    pub fn with_memory(mut self, store: Arc<VectorStore>) -> Self {
        self.memory = Some(MemoryTool::new(store));
        self
    }
    
    /// Get a reference to the terminal executor
    pub fn terminal(&self) -> &dyn TerminalExecutor {
        &*self.terminal
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<&dyn ToolCapability> {
        match name {
            "shell" => Some(&self.shell),
            "read_file" | "cat" => Some(&self.read_file),
            "write_file" => Some(&self.write_file),
            "list_files" | "ls" | "list_dir" => Some(&self.list_files),
            "git_status" => Some(&self.git_status),
            "git_log" => Some(&self.git_log),
            "git_diff" => Some(&self.git_diff),
            "web_search" => Some(&self.web_search),
            "memory" => self.memory.as_ref().map(|m| m as &dyn ToolCapability),
            _ => None,
        }
    }

    /// Check if a tool exists
    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// List all available tool names
    pub fn list(&self) -> Vec<String> {
        let mut tools = vec![
            "shell".to_string(),
            "read_file".to_string(),
            "write_file".to_string(),
            "list_files".to_string(),
            "git_status".to_string(),
            "git_log".to_string(),
            "git_diff".to_string(),
            "web_search".to_string(),
        ];
        if self.memory.is_some() {
            tools.push("memory".to_string());
        }
        tools
    }

    /// Get tool descriptions for prompt generation
    pub fn descriptions(&self) -> Vec<ToolDescription> {
        let mut descriptions = vec![
            ToolDescription {
                name: "shell",
                description: "Execute shell commands",
                usage: "shell <command> or {\"command\": \"<cmd>\"}",
            },
            ToolDescription {
                name: "read_file",
                description: "Read file contents",
                usage: "read_file <path>",
            },
            ToolDescription {
                name: "write_file",
                description: "Write content to file",
                usage: "write_file <path> <content> or {\"path\": \"<path>\", \"content\": \"<content>\"}",
            },
            ToolDescription {
                name: "list_files",
                description: "List directory contents",
                usage: "list_files <path> or {\"path\": \"<path>\"}",
            },
            ToolDescription {
                name: "git_status",
                description: "Show git working tree status",
                usage: "git_status",
            },
            ToolDescription {
                name: "git_log",
                description: "Show git commit history",
                usage: "git_log or {\"limit\": 10}",
            },
            ToolDescription {
                name: "git_diff",
                description: "Show git changes",
                usage: "git_diff or {\"path\": \"<file>\"}",
            },
            ToolDescription {
                name: "web_search",
                description: "Search the web for information",
                usage: "web_search <query> or {\"query\": \"<search>\"}",
            },
        ];
        
        if self.memory.is_some() {
            descriptions.push(ToolDescription {
                name: "memory",
                description: "Store or search long-term memories",
                usage: "memory(add: <content>) or memory(search: <query>)",
            });
        }
        
        descriptions
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Capability for ToolRegistry {
    fn name(&self) -> &'static str {
        "tool-registry"
    }
}

#[async_trait::async_trait]
impl ToolCapability for ToolRegistry {
    async fn execute(
        &self,
        ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        match self.get(&call.name) {
            Some(tool) => tool.execute(ctx, call).await,
            None => Ok(ToolResult::Error {
                message: format!("Unknown tool: {}", call.name),
                code: Some("UNKNOWN_TOOL".to_string()),
                retryable: false,
            }),
        }
    }
}

/// Tool description for prompt generation
#[derive(Debug, Clone)]
pub struct ToolDescription {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
}

impl ToolDescription {
    /// Format for inclusion in system prompt
    pub fn format_for_prompt(&self) -> String {
        format!(
            "- `{}`: {}\n  Usage: {}",
            self.name, self.description, self.usage
        )
    }
}

/// Helper function to parse JSON arguments
pub fn parse_args<T: serde::de::DeserializeOwned>(args: &serde_json::Value) -> Result<T, ToolError> {
    match serde_json::from_value(args.clone()) {
        Ok(parsed) => Ok(parsed),
        Err(e) => Err(ToolError::new(format!(
            "Failed to parse arguments: {}",
            e
        ))),
    }
}

/// Helper function to parse string arguments as JSON
pub fn parse_str_args<T: serde::de::DeserializeOwned>(args: &str) -> Result<T, ToolError> {
    // Try parsing as JSON first
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(args) {
        if let Ok(parsed) = serde_json::from_value(json) {
            return Ok(parsed);
        }
    }
    
    // Fall back to treating as raw string in "args" field
    let wrapped = serde_json::json!({ "args": args });
    match serde_json::from_value(wrapped) {
        Ok(parsed) => Ok(parsed),
        Err(e) => Err(ToolError::new(format!(
            "Failed to parse arguments: {}",
            e
        ))),
    }
}
