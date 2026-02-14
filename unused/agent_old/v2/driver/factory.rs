//! Agent Factory for creating agents with different tool configurations
//! 
//! This module provides convenient ways to create agents with pre-configured
//! tool sets, supporting the orthogonal architecture where tools can be
//! mixed and matched without hardcoding dependencies.

use crate::agent_old::{Agent, ToolRegistry};
use crate::agent_old::tool::Tool;
use crate::agent_old::v2::AgentV2;
use crate::agent_old::tools;
use crate::llm::LlmClient;
use crate::config::AgentVersion;
use crate::memory::scribe::Scribe;
use std::sync::{Arc, Mutex};
use anyhow::Result;

pub enum BuiltAgent {
    V1(Agent),
    V2(AgentV2),
}

/// Builder for creating agents with different tool configurations
pub struct AgentBuilder {
    llm_client: Arc<LlmClient>,
    scribe: Option<Arc<Scribe>>,
    tool_registry: ToolRegistry,
    pending_tools: Mutex<Vec<Box<dyn Tool>>>,
    system_prompt_prefix: String,
    max_iterations: usize,
    version: AgentVersion,
    memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
    job_registry: Option<crate::agent_old::v2::jobs::JobRegistry>,
    permissions: Option<crate::config::types::AgentPermissions>,
    disable_memory: bool,
    max_actions_before_stall: usize,
    max_consecutive_messages: u32,
    max_recovery_attempts: u32,
    max_tool_failures: usize,
}

impl AgentBuilder {
    /// Create a new agent builder with the given LLM client
    pub fn new(llm_client: Arc<LlmClient>) -> Self {
        Self {
            llm_client,
            scribe: None,
            tool_registry: ToolRegistry::new(),
            pending_tools: Mutex::new(Vec::new()),
            system_prompt_prefix: "You are a helpful AI assistant.".to_string(),
            max_iterations: 50,
            version: AgentVersion::V1,
            memory_store: None,
            categorizer: None,
            job_registry: None,
            permissions: None,
            disable_memory: false,
            max_actions_before_stall: 15,
            max_consecutive_messages: 3,
            max_recovery_attempts: 3,
            max_tool_failures: 5,
        }
    }

    /// Set the scribe for V2 agents
    pub fn with_scribe(mut self, scribe: Arc<Scribe>) -> Self {
        self.scribe = Some(scribe);
        self
    }
    
    /// Set the system prompt prefix
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt_prefix = prompt;
        self
    }
    
    /// Set the maximum iterations
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }
    
    /// Set the agent version
    pub fn with_version(mut self, version: AgentVersion) -> Self {
        self.version = version;
        self
    }
    
    /// Set the memory store
    pub fn with_memory_store(mut self, memory_store: Arc<crate::memory::store::VectorStore>) -> Self {
        self.memory_store = Some(memory_store);
        self
    }
    
    /// Set the memory categorizer
    pub fn with_categorizer(mut self, categorizer: Arc<crate::memory::MemoryCategorizer>) -> Self {
        self.categorizer = Some(categorizer);
        self
    }

    /// Set the job registry
    pub fn with_job_registry(mut self, registry: crate::agent_old::v2::jobs::JobRegistry) -> Self {
        self.job_registry = Some(registry);
        self
    }

    /// Set the permissions for this agent
    pub fn with_permissions(mut self, permissions: Option<crate::config::types::AgentPermissions>) -> Self {
        self.permissions = permissions;
        self
    }

    /// Set the maximum tool failures before stalling
    pub fn with_max_tool_failures(mut self, max_tool_failures: usize) -> Self {
        self.max_tool_failures = max_tool_failures;
        self
    }

    /// Add a single tool to the registry
    pub fn with_tool(self, tool: Box<dyn Tool>) -> Self {
        self.pending_tools.lock().unwrap().push(tool);
        self
    }
    
    /// Add multiple tools to the registry
    pub fn with_tools(self, mut tools: Vec<Box<dyn Tool>>) -> Self {
        self.pending_tools.lock().unwrap().append(&mut tools);
        self
    }

    /// Build the tools and return them
    pub async fn build_tools(&self) -> Result<Vec<Arc<dyn Tool>>> {
        // Drain pending tools
        let mut guard = self.pending_tools.lock().unwrap();
        let to_register = guard.drain(..).collect::<Vec<_>>();
        drop(guard);

        // Register each tool
        for tool in to_register {
            self.tool_registry.register_tool(tool).await?;
        }

        // Return all tools from registry
        Ok(self.tool_registry.get_all_tools().await)
    }
    
    /// Build the agent with the current configuration
    pub async fn build(self) -> BuiltAgent {
        // Take ownership of pending tools from the mutex
        let tools_to_register = self.pending_tools.into_inner()
            .unwrap_or_else(|e| e.into_inner());

        // Register all pending tools
        for tool in tools_to_register {
            let _ = self.tool_registry.register_tool(tool).await;
        }

        // Get all tools from the registry
        let tools_list = self.tool_registry.get_all_tools().await;
        
        match self.version {
            AgentVersion::V2 => {
                let scribe = self.scribe.expect("Scribe is required for Agent V2");

                let config = crate::agent_old::v2::AgentV2Config {
                    client: self.llm_client,
                    scribe,
                    tools: tools_list,
                    system_prompt_prefix: self.system_prompt_prefix,
                    max_iterations: self.max_iterations,
                    version: self.version,
                    memory_store: self.memory_store,
                    categorizer: self.categorizer,
                    job_registry: self.job_registry,
                    capabilities_context: None,
                    permissions: self.permissions,
                    scratchpad: None,
                    disable_memory: self.disable_memory,
                    event_bus: None,
                    execute_tools_internally: true,
                    max_actions_before_stall: self.max_actions_before_stall,
                    max_consecutive_messages: self.max_consecutive_messages,
                    max_recovery_attempts: self.max_recovery_attempts,
                    max_tool_failures: self.max_tool_failures,
                };
                BuiltAgent::V2(AgentV2::new_with_config(config))
            },
            AgentVersion::V1 => {
                let config = crate::agent_old::AgentConfig {
                    client: self.llm_client,
                    tools: tools_list,
                    system_prompt_prefix: self.system_prompt_prefix,
                    max_iterations: self.max_iterations,
                    version: self.version,
                    memory_store: self.memory_store,
                    categorizer: self.categorizer,
                    job_registry: self.job_registry,
                    scratchpad: None,
                    max_tool_failures: self.max_tool_failures,
                    disable_memory: self.disable_memory,
                    permissions: self.permissions,
                    event_bus: None,
                    max_actions_before_stall: self.max_actions_before_stall,
                    max_consecutive_messages: self.max_consecutive_messages,
                    max_recovery_attempts: self.max_recovery_attempts,
                };
                BuiltAgent::V1(Agent::new_with_config(config).await)
            }
        }
    }
}

/// Pre-configured agent builders for common use cases
pub struct AgentConfigs;

impl AgentConfigs {
    /// Create a basic agent with essential tools (shell, file operations)
    pub fn basic(llm_client: Arc<LlmClient>) -> AgentBuilder {
        AgentBuilder::new(llm_client)
            .with_system_prompt("You are a helpful AI assistant with basic system access.".to_string())
            .with_tools(vec![
                Box::new(tools::fs::FileReadTool),
                Box::new(tools::fs::FileWriteTool),
                Box::new(tools::find::FindTool),
                Box::new(tools::ListFilesTool::with_cwd()),
                Box::new(tools::system::SystemMonitorTool::new()),
            ])
    }
    
    /// Create a development agent with programming tools
    pub fn development(llm_client: Arc<LlmClient>) -> AgentBuilder {
        let job_registry = crate::agent_old::v2::jobs::JobRegistry::new();

        AgentBuilder::new(llm_client)
            .with_system_prompt("You are a helpful AI assistant specialized in software development.".to_string())
            .with_job_registry(job_registry.clone())
            .with_tools(vec![
                Box::new(tools::fs::FileReadTool),
                Box::new(tools::fs::FileWriteTool),
                Box::new(tools::find::FindTool),
                Box::new(tools::ListFilesTool::with_cwd()),
                Box::new(tools::TailTool),
                Box::new(tools::WordCountTool),
                Box::new(tools::GrepTool),
                Box::new(tools::DiskUsageTool),
                Box::new(tools::git::GitStatusTool),
                Box::new(tools::git::GitLogTool),
                Box::new(tools::git::GitDiffTool),
                Box::new(tools::system::SystemMonitorTool::new()),
                Box::new(tools::wait::WaitTool),
                Box::new(tools::ListJobsTool::new(job_registry)),
            ])
    }
    
    /// Create a web-enabled agent with internet access
    pub fn web_enabled(llm_client: Arc<LlmClient>) -> AgentBuilder {
        AgentBuilder::new(llm_client)
            .with_system_prompt("You are a helpful AI assistant with web access capabilities.".to_string())
            .with_tools(vec![
                Box::new(tools::fs::FileReadTool),
                Box::new(tools::fs::FileWriteTool),
                Box::new(tools::find::FindTool),
                Box::new(tools::system::SystemMonitorTool::new()),
            ])
    }
    
    /// Create a memory-enabled agent with full capabilities
    pub fn full_featured(llm_client: Arc<LlmClient>) -> AgentBuilder {
        let job_registry = crate::agent_old::v2::jobs::JobRegistry::new();

        AgentBuilder::new(llm_client)
            .with_system_prompt("You are a helpful AI assistant with full system access and memory capabilities.".to_string())
            .with_job_registry(job_registry.clone())
            .with_tools(vec![
                Box::new(tools::fs::FileReadTool),
                Box::new(tools::fs::FileWriteTool),
                Box::new(tools::find::FindTool),
                Box::new(tools::ListFilesTool::with_cwd()),
                Box::new(tools::TailTool),
                Box::new(tools::WordCountTool),
                Box::new(tools::GrepTool),
                Box::new(tools::DiskUsageTool),
                Box::new(tools::git::GitStatusTool),
                Box::new(tools::git::GitLogTool),
                Box::new(tools::git::GitDiffTool),
                Box::new(tools::system::SystemMonitorTool::new()),
                Box::new(tools::wait::WaitTool),
                Box::new(tools::ListJobsTool::new(job_registry)),
            ])
    }
    
    /// Create a minimal agent with only the most essential tools
    pub fn minimal(llm_client: Arc<LlmClient>) -> AgentBuilder {
        AgentBuilder::new(llm_client)
            .with_system_prompt("You are a helpful AI assistant with minimal system access.".to_string())
            .with_tools(vec![
                Box::new(tools::fs::FileReadTool),
            ])
    }

    /// Create the one-shot agent used by the CLI (src/main.rs)
    #[allow(clippy::too_many_arguments)]
    pub fn one_shot(
        llm_client: Arc<LlmClient>,
        executor: Arc<crate::executor::CommandExecutor>,
        context: crate::context::TerminalContext,
        terminal_executor: Arc<dyn crate::agent_old::traits::TerminalExecutor>,
        store: Arc<crate::memory::store::VectorStore>,
        categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
        job_registry: crate::agent_old::v2::jobs::JobRegistry,
        permissions: Option<crate::config::types::AgentPermissions>,
        web_search_config: crate::config::WebSearchConfig,
        crawl_event_bus: Arc<crate::agent_old::event_bus::EventBus>,
        state_store: Arc<std::sync::RwLock<crate::state::StateStore>>,
    ) -> AgentBuilder {
        // Use terminal's CWD for file operations
        let base_path = context.cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        
        AgentBuilder::new(llm_client)
            .with_job_registry(job_registry.clone())
            .with_permissions(permissions.clone())
            .with_tools(vec![
                Box::new(tools::shell::ShellTool::new(
                    executor,
                    context,
                    terminal_executor.clone(),
                    Some(store.clone()),
                    categorizer,
                    None,
                    Some(job_registry.clone()),
                    permissions,
                )),
                Box::new(tools::web_search::WebSearchTool::new(web_search_config)),
                Box::new(tools::memory::MemoryTool::new(store.clone())),
                Box::new(tools::crawl::CrawlTool::new(crawl_event_bus)),
                Box::new(tools::fs::FileReadTool),
                Box::new(tools::fs::FileWriteTool),
                Box::new(tools::git::GitStatusTool),
                Box::new(tools::git::GitLogTool),
                Box::new(tools::git::GitDiffTool),
                Box::new(tools::state::StateTool::new(state_store)),
                Box::new(tools::system::SystemMonitorTool::new()),
                Box::new(tools::wait::WaitTool),
                Box::new(tools::jobs::ListJobsTool::new(job_registry)),
                Box::new(tools::terminal_sight::TerminalSightTool::new(terminal_executor)),
                Box::new(tools::ListFilesTool::new(base_path)),
            ])
    }

    /// Create the TUI agent used by the terminal (src/terminal/mod.rs)
    #[allow(clippy::too_many_arguments)]
    pub fn tui(
        llm_client: Arc<LlmClient>,
        executor: Arc<crate::executor::CommandExecutor>,
        context: crate::context::TerminalContext,
        terminal_executor: Arc<dyn crate::agent_old::traits::TerminalExecutor>,
        store: Arc<crate::memory::store::VectorStore>,
        categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
        job_registry: crate::agent_old::v2::jobs::JobRegistry,
        permissions: Option<crate::config::types::AgentPermissions>,
        web_search_config: crate::config::WebSearchConfig,
        crawl_event_bus: Arc<crate::agent_old::event_bus::EventBus>,
        state_store: Arc<std::sync::RwLock<crate::state::StateStore>>,
        scratchpad: Arc<tokio::sync::RwLock<tools::scratchpad::StructuredScratchpad>>,
        session_id: Option<String>,
    ) -> AgentBuilder {
        // Use terminal's CWD for file operations
        let base_path = context.cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        
        AgentBuilder::new(llm_client)
            .with_job_registry(job_registry.clone())
            .with_permissions(permissions.clone())
            .with_tools(vec![
                Box::new(tools::shell::ShellTool::new(
                    executor,
                    context,
                    terminal_executor.clone(),
                    Some(store.clone()),
                    categorizer,
                    session_id,
                    Some(job_registry),
                    permissions,
                )),
                Box::new(tools::memory::MemoryTool::new(store)),
                Box::new(tools::web_search::WebSearchTool::new(web_search_config)),
                Box::new(tools::crawl::CrawlTool::new(crawl_event_bus)),
                Box::new(tools::state::StateTool::new(state_store)),
                Box::new(tools::system::SystemMonitorTool::new()),
                Box::new(tools::wait::WaitTool),
                Box::new(tools::terminal_sight::TerminalSightTool::new(terminal_executor)),
                Box::new(tools::scratchpad::ScratchpadTool::new(scratchpad)),
                Box::new(tools::fs::FileReadTool),
                Box::new(tools::fs::FileWriteTool),
                Box::new(tools::git::GitStatusTool),
                Box::new(tools::git::GitLogTool),
                Box::new(tools::git::GitDiffTool),
                Box::new(tools::ListFilesTool::new(base_path)),
                Box::new(tools::TailTool),
                Box::new(tools::WordCountTool),
                Box::new(tools::GrepTool),
                Box::new(tools::DiskUsageTool),
            ])
    }
}

/// Helper function to create a basic agent quickly
pub async fn create_basic_agent(llm_client: Arc<LlmClient>) -> BuiltAgent {
    AgentConfigs::basic(llm_client).build().await
}

/// Helper function to create a development agent quickly
pub async fn create_development_agent(llm_client: Arc<LlmClient>) -> BuiltAgent {
    AgentConfigs::development(llm_client).build().await
}

/// Helper function to create a web-enabled agent quickly
pub async fn create_web_agent(llm_client: Arc<LlmClient>) -> BuiltAgent {
    AgentConfigs::web_enabled(llm_client).build().await
}

/// Helper function to create a full-featured agent quickly
pub async fn create_full_agent(llm_client: Arc<LlmClient>) -> BuiltAgent {
    AgentConfigs::full_featured(llm_client).build().await
}
