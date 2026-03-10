//! Tool implementations for the agent runtime
//!
//! This module provides concrete tool implementations that execute
//! actions on behalf of the agent. All tools implement the `ToolCapability` trait.

pub mod shell;
pub mod read_file;
pub mod write_file;
pub mod edit_csv;
pub mod list_files;
pub mod git;
pub mod web_search;
pub mod memory;
pub mod notes;
pub mod delegate;
pub mod scratchpad;
pub mod worker_shell;
pub mod commonboard;
pub mod search_files;
pub mod document_workers;

pub use shell::ShellTool;
pub use read_file::ReadFileTool;
pub use write_file::WriteFileTool;
pub use edit_csv::EditCsvTool;
pub use list_files::ListFilesTool;
pub use git::{GitStatusTool, GitLogTool, GitDiffTool};
pub use web_search::{WebSearchTool, WebSearchConfig, SearchProvider};
pub use memory::MemoryTool;
pub use notes::NotesTool;
pub use delegate::DelegateTool;
pub use scratchpad::{ScratchpadTool, create_shared_scratchpad, SharedScratchpad};
pub use worker_shell::{WorkerShellTool, WorkerShellPermissions, EscalationRequest, EscalationResponse};
pub use commonboard::CommonboardTool;
pub use search_files::SearchFilesTool;
pub use document_workers::{QueryFileTool, QueryChunkTool, CloseFileTool, ChunkWorkerRegistry};

use std::sync::Arc;
use std::path::Path;
use crate::agent::runtime::core::{Capability, ToolCapability, RuntimeContext, ToolError};
use crate::agent::runtime::core::terminal::{TerminalExecutor, DefaultTerminalExecutor};
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use crate::memory::store::VectorStore;

/// Tool registry that combines all available tools
pub struct ToolRegistry {
    shell: ShellTool,
    read_file: ReadFileTool,
    write_file: WriteFileTool,
    edit_csv: EditCsvTool,
    list_files: ListFilesTool,
    git_status: GitStatusTool,
    git_log: GitLogTool,
    git_diff: GitDiffTool,
    web_search: WebSearchTool,
    memory: Option<MemoryTool>,
    notes: NotesTool,
    terminal: Arc<dyn TerminalExecutor>,
    /// Delegate tool for spawning workers (optional, requires initialization)
    delegate: Option<Arc<DelegateTool>>,
    /// Scratchpad tool for agent-local persistent notes (optional)
    scratchpad: Option<ScratchpadTool>,
    /// Commonboard tool for inter-agent coordination (optional)
    commonboard: Option<CommonboardTool>,
    /// Search tool for full-text file search (optional)
    search_files: Option<SearchFilesTool>,
    /// Document worker tools for active chunk processing (optional)
    query_file: Option<QueryFileTool>,
    query_chunk_worker: Option<QueryChunkTool>,
    close_file: Option<CloseFileTool>,
}

impl ToolRegistry {
    /// Create a new tool registry with all default tools
    /// 
    /// Uses a default terminal executor (std::process::Command).
    /// Use `with_terminal()` to provide a custom terminal executor (e.g., PTY-based).
    /// Use `with_delegate()` to enable worker spawning.
    /// Use `with_scratchpad()` to add agent-local persistent notes.
    /// Use `with_commonboard()` to enable inter-agent coordination.
    pub fn new() -> Self {
        Self {
            shell: ShellTool::new(),
            read_file: ReadFileTool::simple(),
            write_file: WriteFileTool::new(),
            edit_csv: EditCsvTool::new(),
            list_files: ListFilesTool::new(),
            git_status: GitStatusTool::new(),
            git_log: GitLogTool::new(),
            git_diff: GitDiffTool::new(),
            web_search: WebSearchTool::new(),
            memory: None,
            notes: NotesTool::new(),
            terminal: Arc::new(DefaultTerminalExecutor::new()),
            delegate: None,
            scratchpad: None,
            commonboard: None,
            search_files: None,
            query_file: None,
            query_chunk_worker: None,
            close_file: None,
        }
    }
    
    /// Enable delegate tool for spawning workers
    pub fn with_delegate(mut self, delegate: Arc<DelegateTool>) -> Self {
        self.delegate = Some(delegate);
        self
    }
    
    /// Enable scratchpad tool for agent-local persistent notes
    pub fn with_scratchpad(mut self, scratchpad: ScratchpadTool) -> Self {
        self.scratchpad = Some(scratchpad);
        self
    }
    
    /// Enable commonboard tool for inter-agent coordination
    pub fn with_commonboard(mut self, commonboard: CommonboardTool) -> Self {
        self.commonboard = Some(commonboard);
        self
    }
    
    /// Enable search_files tool for full-text file search
    /// 
    /// # Arguments
    /// * `index_path` - Optional path for persistent index storage.
    ///                  If None, an in-memory index is used.
    pub fn with_search_files(mut self, index_path: Option<&Path>) -> Result<Self, ToolError> {
        let tool = if let Some(path) = index_path {
            SearchFilesTool::with_index_path(path)?
        } else {
            SearchFilesTool::new()?
        };
        self.search_files = Some(tool);
        Ok(self)
    }
    
    /// Set a custom terminal executor
    /// 
    /// This allows the TUI to provide a PTY-based terminal executor
    /// so agent commands run in the shared terminal session.
    pub fn with_terminal(mut self, terminal: Arc<dyn TerminalExecutor>) -> Self {
        self.terminal = terminal;
        self
    }
    
    /// Enable memory tool with a VectorStore (default search limit)
    pub fn with_memory(mut self, store: Arc<VectorStore>) -> Self {
        self.memory = Some(MemoryTool::new(store));
        self
    }
    
    /// Enable memory tool with a VectorStore and custom search limit
    pub fn with_memory_and_limit(mut self, store: Arc<VectorStore>, search_limit: usize) -> Self {
        self.memory = Some(MemoryTool::with_search_limit(store, search_limit));
        self
    }
    
    /// Enable document worker tools with registry and LLM client
    pub fn with_document_workers(
        mut self,
        registry: Arc<ChunkWorkerRegistry>,
        llm_client: Arc<crate::provider::LlmClient>,
        worker_context_window: usize,
        output_tx: crate::agent::runtime::orchestrator::OutputSender,
    ) -> Self {
        self.query_file = Some(QueryFileTool::new(
            Arc::clone(&registry),
            llm_client.clone(),
            worker_context_window,
            output_tx,
        ));
        self.query_chunk_worker = Some(QueryChunkTool::new(Arc::clone(&registry)));
        self.close_file = Some(CloseFileTool::new(registry));
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
            "edit_csv" => Some(&self.edit_csv),
            "list_files" | "ls" | "list_dir" => Some(&self.list_files),
            "git_status" => Some(&self.git_status),
            "git_log" => Some(&self.git_log),
            "git_diff" => Some(&self.git_diff),
            "web_search" => Some(&self.web_search),
            "memory" => self.memory.as_ref().map(|m| m as &dyn ToolCapability),
            "notes" => Some(&self.notes),
            "delegate" => self.delegate.as_ref().map(|d| d.as_ref() as &dyn ToolCapability),
            "scratchpad" => self.scratchpad.as_ref().map(|s| s as &dyn ToolCapability),
            "commonboard" => self.commonboard.as_ref().map(|c| c as &dyn ToolCapability),
            "search_files" => self.search_files.as_ref().map(|s| s as &dyn ToolCapability),
            "query_file" => self.query_file.as_ref().map(|q| q as &dyn ToolCapability),
            "query_chunk_worker" => self.query_chunk_worker.as_ref().map(|q| q as &dyn ToolCapability),
            "close_file" => self.close_file.as_ref().map(|c| c as &dyn ToolCapability),
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
            "edit_csv".to_string(),
            "list_files".to_string(),
            "git_status".to_string(),
            "git_log".to_string(),
            "git_diff".to_string(),
            "web_search".to_string(),
            "notes".to_string(),
        ];
        if self.memory.is_some() {
            tools.push("memory".to_string());
        }
        if self.delegate.is_some() {
            tools.push("delegate".to_string());
        }
        if self.scratchpad.is_some() {
            tools.push("scratchpad".to_string());
        }
        if self.commonboard.is_some() {
            tools.push("commonboard".to_string());
        }
        if self.search_files.is_some() {
            tools.push("search_files".to_string());
        }
        if self.query_file.is_some() {
            tools.push("query_file".to_string());
        }
        if self.query_chunk_worker.is_some() {
            tools.push("query_chunk_worker".to_string());
        }
        if self.close_file.is_some() {
            tools.push("close_file".to_string());
        }
        tools
    }

    /// Get tool descriptions for prompt generation
    pub fn descriptions(&self) -> Vec<ToolDescription> {
        let mut descriptions = vec![
            ToolDescription {
                name: "shell",
                description: "Execute or suggest shell commands",
                usage: "Execute: {\"a\": \"shell\", \"i\": {\"command\": \"<cmd>\"}} | Suggest: {\"a\": \"shell\", \"i\": {\"command\": \"<cmd>\", \"mode\": \"suggest\"}}",
            },
            ToolDescription {
                name: "read_file",
                description: "Read file contents. DO NOT use this tool for files the user uploaded. Only use for quick, small code reading or specific line checks.",
                usage: r#"Small file: {"a": "read_file", "i": {"path": "<path>"}} | Partial: {"a": "read_file", "i": {"path": "<path>", "line_offset": 1, "n_lines": 100}}"#,
            },
            ToolDescription {
                name: "write_file",
                description: "Write content to file",
                usage: "{\"a\": \"write_file\", \"i\": {\"path\": \"<path>\", \"content\": \"<content>\"}}",
            },
            ToolDescription {
                name: "edit_csv",
                description: "Edit CSV files with structured operations (update, delete, insert, update_where)",
                usage: "Update cell: {\"a\": \"edit_csv\", \"i\": {\"path\": \"<path>\", \"operation\": \"update\", \"row\": 1, \"column\": \"Name\", \"value\": \"New\"}} | Update where: {\"a\": \"edit_csv\", \"i\": {\"path\": \"<path>\", \"operation\": \"update_where\", \"where\": {\"column\": \"Status\", \"equals\": \"inactive\"}, \"column\": \"Status\", \"value\": \"active\"}}",
            },
            ToolDescription {
                name: "list_files",
                description: "List directory contents",
                usage: "{\"a\": \"list_files\", \"i\": {\"path\": \"<path>\"}}",
            },
            ToolDescription {
                name: "git_status",
                description: "Show git working tree status",
                usage: "{\"a\": \"git_status\"}",
            },
            ToolDescription {
                name: "git_log",
                description: "Show git commit history",
                usage: "{\"a\": \"git_log\", \"i\": {\"limit\": 10}}",
            },
            ToolDescription {
                name: "git_diff",
                description: "Show git changes",
                usage: "{\"a\": \"git_diff\", \"i\": {\"path\": \"<file>\"}}",
            },
            ToolDescription {
                name: "web_search",
                description: "Search the web for information",
                usage: "{\"a\": \"web_search\", \"i\": {\"query\": \"<search>\"}}",
            },
            ToolDescription {
                name: "notes",
                description: "Access user's quick notes for context and reminders",
                usage: "{\"a\": \"notes\", \"i\": {\"action\": \"read\"}} or {\"a\": \"notes\", \"i\": {\"action\": \"search\", \"query\": \"<search>\"}}",
            },
        ];
        
        if self.memory.is_some() {
            descriptions.push(ToolDescription {
                name: "memory",
                description: "Store or search long-term memories. CRITICAL: Use EXACT JSON format shown",
                usage: "Add: {\"a\": \"memory\", \"i\": {\"add\": \"User prefers dark mode\"}} | Search: {\"a\": \"memory\", \"i\": {\"search\": \"dark mode preference\"}}",
            });
        }
        
        if self.delegate.is_some() {
            descriptions.push(ToolDescription {
                name: "delegate",
                description: "Spawn worker agents for parallel/independent tasks. USE FOR: large file analysis, batch processing, background tasks, parallel searches. Workers run independently with isolated shells",
                usage: r#"Large file analysis: {"a": "delegate", "i": {"workers": [{"id": "analyzer", "objective": "Read src/main.rs and summarize key functions", "tools": ["read_file", "shell"]}]}} | Parallel tasks: {"a": "delegate", "i": {"workers": [{"id": "w1", "objective": "Find TODOs in src/", "tools": ["shell"], "allowed_commands": ["grep -r TODO src/"]}, {"id": "w2", "objective": "Find FIXMEs in src/", "tools": ["shell"], "allowed_commands": ["grep -r FIXME src/"]}]}}"#,
            });
        }
        
        if self.scratchpad.is_some() {
            descriptions.push(ToolDescription {
                name: "scratchpad",
                description: "Agent-local persistent notes (survives pruning)",
                usage: "{\"a\": \"scratchpad\", \"i\": {\"action\": \"append\", \"text\": \"note\"}} or {\"a\": \"scratchpad\", \"i\": {\"action\": \"list\"}}",
            });
        }
        
        if self.commonboard.is_some() {
            descriptions.push(ToolDescription {
                name: "commonboard",
                description: "Inter-agent coordination (claims, progress, completion)",
                usage: "{\"a\": \"commonboard\", \"i\": {\"action\": \"claim\", \"resource\": \"file.rs\"}} or {\"a\": \"commonboard\", \"i\": {\"action\": \"query\"}}",
            });
        }
        
        if self.search_files.is_some() {
            descriptions.push(ToolDescription {
                name: "search_files",
                description: "Full-text search across indexed files",
                usage: "{\"a\": \"search_files\", \"i\": {\"query\": \"function main\"}} or {\"a\": \"search_files\", \"i\": {\"query\": \"TODO\", \"path_filter\": \"src/\"}}",
            });
        }
        
        if self.query_file.is_some() {
            descriptions.push(ToolDescription {
                name: "query_file",
                description: "ALWAYS use this tool to process user-uploaded files or large documents. It splits the document into chunks, spawns sandboxed LLM workers for each chunk to prevent overwhelming context, and returns a summary plus chunk IDs for follow-ups.",
                usage: r#"Process file: {"a": "query_file", "i": {"file_path": "large_document.pdf", "prompt": "Summarize the key points"}}"#,
            });
        }
        
        if self.query_chunk_worker.is_some() {
            descriptions.push(ToolDescription {
                name: "query_chunk_worker",
                description: "Send a query to a specific chunk worker after using query_file. Use this for follow-up questions about specific chunks",
                usage: r#"Query specific chunk: {"a": "query_chunk_worker", "i": {"chunk_id": "document_chunk_0", "prompt": "What does this section say about error handling?"}}"#,
            });
        }
        
        if self.close_file.is_some() {
            descriptions.push(ToolDescription {
                name: "close_file",
                description: "Clean up chunk workers for a file when done. Call this to free memory after finishing analysis",
                usage: r#"Close file workers: {"a": "close_file", "i": {"file_name": "large_document.pdf"}}"#,
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

/// Expand tilde (~) to home directory in a path
/// 
/// This handles:
/// - `~/path` -> `/home/user/path`
/// - `~user/path` -> `/home/user/path` (if user's home can be determined)
/// - `/absolute/path` -> unchanged
/// - `relative/path` -> unchanged
pub fn expand_tilde(path: &str) -> String {
    if !path.starts_with('~') {
        return path.to_string();
    }
    
    // Get home directory
    let home = dirs::home_dir();
    
    if path == "~" {
        // Just tilde - return home directory
        return home.map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
    }
    
    if path.starts_with("~/") {
        // Tilde with path after it
        return home.map(|h| {
            let rest = &path[2..]; // Skip "~/"
            h.join(rest).to_string_lossy().to_string()
        }).unwrap_or_else(|| path.to_string());
    }
    
    // ~user/... format - we don't handle other users' home directories
    // Just return as-is, the OS will resolve it if possible
    path.to_string()
}
