//! Restricted Tool Registry - Tool registry with filtered capabilities for workers
//!
//! Creates a restricted view of tools based on an allowlist.
//! Used by the delegate tool to give workers limited tool access.

use crate::agent::runtime::core::{Capability, ToolCapability};
use crate::agent::runtime::core::RuntimeContext;
use crate::agent::runtime::core::ToolError;
use crate::agent::runtime::tools::{
    ShellTool, ReadFileTool, WriteFileTool, ListFilesTool,
    GitStatusTool, GitLogTool, GitDiffTool, WebSearchTool, MemoryTool,
    ScratchpadTool, WorkerShellTool, WorkerShellPermissions, EscalationRequest, EscalationResponse,
};
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// A restricted tool registry that only exposes allowed tools
pub struct RestrictedToolRegistry {
    /// Allowed tool names (stored for reference/debugging)
    #[allow(dead_code)]
    allowed_tools: HashSet<String>,
    /// Shell tool (may be WorkerShellTool for escalation)
    shell: Option<Arc<dyn ToolCapability>>,
    /// Read file tool
    read_file: Option<ReadFileTool>,
    /// Write file tool
    write_file: Option<WriteFileTool>,
    /// List files tool
    list_files: Option<ListFilesTool>,
    /// Git status tool
    git_status: Option<GitStatusTool>,
    /// Git log tool
    git_log: Option<GitLogTool>,
    /// Git diff tool
    git_diff: Option<GitDiffTool>,
    /// Web search tool
    web_search: Option<WebSearchTool>,
    /// Memory tool
    memory: Option<MemoryTool>,
    /// Scratchpad tool
    scratchpad: Option<ScratchpadTool>,
}

impl RestrictedToolRegistry {
    /// Create a restricted registry from a parent registry and allowed tools list
    pub fn new(
        _parent: &dyn ToolCapability,
        allowed: Option<&Vec<String>>,
        worker_config: Option<WorkerToolConfig>,
    ) -> Self {
        let allowed_set: HashSet<String> = allowed
            .map(|list| list.iter().cloned().collect())
            .unwrap_or_else(|| {
                // If no allowlist specified, include all standard tools
                [
                    "shell", "read_file", "write_file", "list_files",
                    "git_status", "git_log", "git_diff", "web_search",
                    "memory", "scratchpad",
                ].iter().map(|s| s.to_string()).collect()
            });

        let config = worker_config.unwrap_or_default();

        Self {
            allowed_tools: allowed_set.clone(),
            
            // Shell tool - use WorkerShellTool if escalation is configured
            shell: if allowed_set.contains("shell") {
                if let Some(esc_config) = config.escalation {
                    Some(Arc::new(WorkerShellTool::new(
                        esc_config.permissions,
                        esc_config.worker_id,
                        esc_config.job_id,
                        esc_config.escalation_tx,
                    )) as Arc<dyn ToolCapability>)
                } else {
                    Some(Arc::new(ShellTool::new()) as Arc<dyn ToolCapability>)
                }
            } else {
                None
            },
            
            read_file: if allowed_set.contains("read_file") {
                Some(ReadFileTool::new())
            } else {
                None
            },
            
            write_file: if allowed_set.contains("write_file") {
                Some(WriteFileTool::new())
            } else {
                None
            },
            
            list_files: if allowed_set.contains("list_files") {
                Some(ListFilesTool::new())
            } else {
                None
            },
            
            git_status: if allowed_set.contains("git_status") {
                Some(GitStatusTool::new())
            } else {
                None
            },
            
            git_log: if allowed_set.contains("git_log") {
                Some(GitLogTool::new())
            } else {
                None
            },
            
            git_diff: if allowed_set.contains("git_diff") {
                Some(GitDiffTool::new())
            } else {
                None
            },
            
            web_search: if allowed_set.contains("web_search") {
                Some(WebSearchTool::new())
            } else {
                None
            },
            
            memory: if allowed_set.contains("memory") {
                // Note: Memory tool would need to be cloned from parent if it has state
                None
            } else {
                None
            },
            
            scratchpad: if allowed_set.contains("scratchpad") {
                config.scratchpad
            } else {
                None
            },
        }
    }
    
    /// Check if a tool is available
    pub fn has(&self, name: &str) -> bool {
        match name {
            "shell" => self.shell.is_some(),
            "read_file" | "cat" => self.read_file.is_some(),
            "write_file" => self.write_file.is_some(),
            "list_files" | "ls" | "list_dir" => self.list_files.is_some(),
            "git_status" => self.git_status.is_some(),
            "git_log" => self.git_log.is_some(),
            "git_diff" => self.git_diff.is_some(),
            "web_search" => self.web_search.is_some(),
            "memory" => self.memory.is_some(),
            "scratchpad" => self.scratchpad.is_some(),
            _ => false,
        }
    }
    
    /// Get a tool by name
    fn get_tool(&self, name: &str) -> Option<&dyn ToolCapability> {
        match name {
            "shell" => self.shell.as_ref().map(|t| t.as_ref()),
            "read_file" | "cat" => self.read_file.as_ref().map(|t| t as &dyn ToolCapability),
            "write_file" => self.write_file.as_ref().map(|t| t as &dyn ToolCapability),
            "list_files" | "ls" | "list_dir" => self.list_files.as_ref().map(|t| t as &dyn ToolCapability),
            "git_status" => self.git_status.as_ref().map(|t| t as &dyn ToolCapability),
            "git_log" => self.git_log.as_ref().map(|t| t as &dyn ToolCapability),
            "git_diff" => self.git_diff.as_ref().map(|t| t as &dyn ToolCapability),
            "web_search" => self.web_search.as_ref().map(|t| t as &dyn ToolCapability),
            "memory" => self.memory.as_ref().map(|t| t as &dyn ToolCapability),
            "scratchpad" => self.scratchpad.as_ref().map(|t| t as &dyn ToolCapability),
            _ => None,
        }
    }
    
    /// List available tool names
    pub fn list(&self) -> Vec<String> {
        let mut tools = Vec::new();
        if self.shell.is_some() { tools.push("shell".to_string()); }
        if self.read_file.is_some() { tools.push("read_file".to_string()); }
        if self.write_file.is_some() { tools.push("write_file".to_string()); }
        if self.list_files.is_some() { tools.push("list_files".to_string()); }
        if self.git_status.is_some() { tools.push("git_status".to_string()); }
        if self.git_log.is_some() { tools.push("git_log".to_string()); }
        if self.git_diff.is_some() { tools.push("git_diff".to_string()); }
        if self.web_search.is_some() { tools.push("web_search".to_string()); }
        if self.memory.is_some() { tools.push("memory".to_string()); }
        if self.scratchpad.is_some() { tools.push("scratchpad".to_string()); }
        tools
    }
    
    /// Get tool descriptions for prompt generation
    pub fn descriptions(&self) -> Vec<super::ToolDescription> {
        let mut descriptions = Vec::new();
        
        if self.shell.is_some() {
            descriptions.push(super::ToolDescription {
                name: "shell",
                description: "Execute shell commands",
                usage: "shell <command> or {\"command\": \"<cmd>\"}",
            });
        }
        
        if self.read_file.is_some() {
            descriptions.push(super::ToolDescription {
                name: "read_file",
                description: "Read file contents",
                usage: "read_file <path>",
            });
        }
        
        if self.write_file.is_some() {
            descriptions.push(super::ToolDescription {
                name: "write_file",
                description: "Write content to file",
                usage: "write_file <path> <content> or {\"path\": \"<path>\", \"content\": \"<content>\"}",
            });
        }
        
        if self.list_files.is_some() {
            descriptions.push(super::ToolDescription {
                name: "list_files",
                description: "List directory contents",
                usage: "list_files <path> or {\"path\": \"<path>\"}",
            });
        }
        
        if self.git_status.is_some() {
            descriptions.push(super::ToolDescription {
                name: "git_status",
                description: "Show git working tree status",
                usage: "git_status",
            });
        }
        
        if self.git_log.is_some() {
            descriptions.push(super::ToolDescription {
                name: "git_log",
                description: "Show git commit history",
                usage: "git_log or {\"limit\": 10}",
            });
        }
        
        if self.git_diff.is_some() {
            descriptions.push(super::ToolDescription {
                name: "git_diff",
                description: "Show git changes",
                usage: "git_diff or {\"path\": \"<file>\"}",
            });
        }
        
        if self.web_search.is_some() {
            descriptions.push(super::ToolDescription {
                name: "web_search",
                description: "Search the web for information",
                usage: "web_search <query> or {\"query\": \"<search>\"}",
            });
        }
        
        if self.memory.is_some() {
            descriptions.push(super::ToolDescription {
                name: "memory",
                description: "Store or search long-term memories",
                usage: "memory(add: <content>) or memory(search: <query>)",
            });
        }
        
        if self.scratchpad.is_some() {
            descriptions.push(super::ToolDescription {
                name: "scratchpad",
                description: "Shared workspace for worker coordination",
                usage: "scratchpad {\"action\": \"append\", \"text\": \"...\"}",
            });
        }
        
        descriptions
    }
}

impl Capability for RestrictedToolRegistry {
    fn name(&self) -> &'static str {
        "restricted-tool-registry"
    }
}

#[async_trait::async_trait]
impl ToolCapability for RestrictedToolRegistry {
    async fn execute(
        &self,
        ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        match self.get_tool(&call.name) {
            Some(tool) => tool.execute(ctx, call).await,
            None => Ok(ToolResult::Error {
                message: format!("Tool '{}' is not available to this worker", call.name),
                code: Some("TOOL_NOT_ALLOWED".to_string()),
                retryable: false,
            }),
        }
    }
}

/// Configuration for worker tools
#[derive(Clone, Default)]
pub struct WorkerToolConfig {
    /// Escalation configuration for shell commands
    pub escalation: Option<WorkerEscalationConfig>,
    /// Shared scratchpad for coordination
    pub scratchpad: Option<ScratchpadTool>,
}

/// Escalation configuration for worker shell
#[derive(Clone)]
pub struct WorkerEscalationConfig {
    /// Worker ID
    pub worker_id: String,
    /// Job ID
    pub job_id: String,
    /// Permissions
    pub permissions: WorkerShellPermissions,
    /// Escalation channel
    pub escalation_tx: Option<mpsc::Sender<(EscalationRequest, oneshot::Sender<EscalationResponse>)>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::tools::ToolRegistry;

    #[test]
    fn test_restricted_registry() {
        let parent = ToolRegistry::new();
        let allowed = vec!["read_file".to_string(), "write_file".to_string()];
        
        let restricted = RestrictedToolRegistry::new(&parent, Some(&allowed), None);
        
        assert!(restricted.has("read_file"));
        assert!(restricted.has("write_file"));
        assert!(!restricted.has("shell"));
        assert!(!restricted.has("web_search"));
        
        let list = restricted.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_restricted_registry_all_tools() {
        let parent = ToolRegistry::new();
        
        // No allowed list = all tools
        let restricted = RestrictedToolRegistry::new(&parent, None, None);
        
        assert!(restricted.has("read_file"));
        assert!(restricted.has("shell"));
        assert!(restricted.has("web_search"));
    }
}
