//! Agent Factory for creating agents with different tool configurations
//! 
//! This module provides convenient ways to create agents with pre-configured
//! tool sets, supporting the orthogonal architecture where tools can be
//! mixed and matched without hardcoding dependencies.

use crate::agent::{Agent, ToolRegistry};
use crate::agent::tools;
use crate::llm::LlmClient;
use crate::config::AgentVersion;
use std::sync::Arc;

/// Builder for creating agents with different tool configurations
pub struct AgentBuilder {
    llm_client: Arc<LlmClient>,
    tool_registry: ToolRegistry,
    system_prompt_prefix: String,
    max_iterations: usize,
    version: AgentVersion,
    memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
}

impl AgentBuilder {
    /// Create a new agent builder with the given LLM client
    pub fn new(llm_client: Arc<LlmClient>) -> Self {
        Self {
            llm_client,
            tool_registry: ToolRegistry::new(),
            system_prompt_prefix: "You are a helpful AI assistant.".to_string(),
            max_iterations: 50,
            version: AgentVersion::V1,
            memory_store: None,
            categorizer: None,
        }
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
    
    /// Add a single tool to the registry
    pub fn with_tool(self, tool: Box<dyn crate::agent::tool::Tool>) -> Self {
        // Note: This is a simplified version. In practice, you'd need to handle
        // the async nature of tool registration. For now, we'll use a blocking approach.
        let rt = tokio::runtime::Handle::current();
        let _ = rt.block_on(self.tool_registry.register_tool(tool));
        self
    }
    
    /// Add multiple tools to the registry
    pub fn with_tools(self, tools: Vec<Box<dyn crate::agent::tool::Tool>>) -> Self {
        let rt = tokio::runtime::Handle::current();
        for tool in tools {
            let _ = rt.block_on(self.tool_registry.register_tool(tool));
        }
        self
    }
    
    /// Build the agent with the current configuration
    pub fn build(self) -> Agent {
        // Get all tools from the registry
        let rt = tokio::runtime::Handle::current();
        let tools = rt.block_on(self.tool_registry.get_all_tools());
        
        Agent::new_with_iterations(
            self.llm_client,
            tools,
            self.system_prompt_prefix,
            self.max_iterations,
            self.version,
            self.memory_store,
            self.categorizer,
        )
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
                Box::new(tools::system::SystemMonitorTool::new()),
            ])
    }
    
    /// Create a development agent with programming tools
    pub fn development(llm_client: Arc<LlmClient>) -> AgentBuilder {
        AgentBuilder::new(llm_client)
            .with_system_prompt("You are a helpful AI assistant specialized in software development.".to_string())
            .with_tools(vec![
                Box::new(tools::fs::FileReadTool),
                Box::new(tools::fs::FileWriteTool),
                Box::new(tools::git::GitStatusTool),
                Box::new(tools::git::GitLogTool),
                Box::new(tools::git::GitDiffTool),
                Box::new(tools::system::SystemMonitorTool::new()),
            ])
    }
    
    /// Create a web-enabled agent with internet access
    pub fn web_enabled(llm_client: Arc<LlmClient>) -> AgentBuilder {
        AgentBuilder::new(llm_client)
            .with_system_prompt("You are a helpful AI assistant with web access capabilities.".to_string())
            .with_tools(vec![
                Box::new(tools::fs::FileReadTool),
                Box::new(tools::fs::FileWriteTool),
                Box::new(tools::system::SystemMonitorTool::new()),
            ])
    }
    
    /// Create a memory-enabled agent with full capabilities
    pub fn full_featured(llm_client: Arc<LlmClient>) -> AgentBuilder {
        AgentBuilder::new(llm_client)
            .with_system_prompt("You are a helpful AI assistant with full system access and memory capabilities.".to_string())
            .with_tools(vec![
                Box::new(tools::fs::FileReadTool),
                Box::new(tools::fs::FileWriteTool),
                Box::new(tools::git::GitStatusTool),
                Box::new(tools::git::GitLogTool),
                Box::new(tools::git::GitDiffTool),
                Box::new(tools::system::SystemMonitorTool::new()),
                Box::new(tools::wait::WaitTool),
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
}

/// Helper function to create a basic agent quickly
pub fn create_basic_agent(llm_client: Arc<LlmClient>) -> Agent {
    AgentConfigs::basic(llm_client).build()
}

/// Helper function to create a development agent quickly
pub fn create_development_agent(llm_client: Arc<LlmClient>) -> Agent {
    AgentConfigs::development(llm_client).build()
}

/// Helper function to create a web-enabled agent quickly
pub fn create_web_agent(llm_client: Arc<LlmClient>) -> Agent {
    AgentConfigs::web_enabled(llm_client).build()
}

/// Helper function to create a full-featured agent quickly
pub fn create_full_agent(llm_client: Arc<LlmClient>) -> Agent {
    AgentConfigs::full_featured(llm_client).build()
}