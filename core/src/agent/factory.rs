//! Agent Factory for creating agents with different tool configurations
//! 
//! This module provides convenient ways to create agents with pre-configured
//! tool sets, supporting the orthogonal architecture where tools can be
//! mixed and matched without hardcoding dependencies.

use crate::agent::{Agent, ToolRegistry};
use crate::agent::v2::AgentV2;
use crate::agent::tools;
use crate::llm::LlmClient;
use crate::config::AgentVersion;
use crate::memory::scribe::Scribe;
use std::sync::Arc;

pub enum BuiltAgent {
    V1(Agent),
    V2(AgentV2),
}

/// Builder for creating agents with different tool configurations
pub struct AgentBuilder {
    llm_client: Arc<LlmClient>,
    scribe: Option<Arc<Scribe>>,
    tool_registry: ToolRegistry,
    pending_tools: Vec<Box<dyn crate::agent::tool::Tool>>,
    system_prompt_prefix: String,
    max_iterations: usize,
    version: AgentVersion,
    memory_store: Option<Arc<crate::memory::store::VectorStore>>,
    categorizer: Option<Arc<crate::memory::MemoryCategorizer>>,
    job_registry: Option<crate::agent::v2::jobs::JobRegistry>,
    disable_memory: bool,
}

impl AgentBuilder {
    /// Create a new agent builder with the given LLM client
    pub fn new(llm_client: Arc<LlmClient>) -> Self {
        Self {
            llm_client,
            scribe: None,
            tool_registry: ToolRegistry::new(),
            pending_tools: Vec::new(),
            system_prompt_prefix: "You are a helpful AI assistant.".to_string(),
            max_iterations: 50,
            version: AgentVersion::V1,
            memory_store: None,
            categorizer: None,
            job_registry: None,
            disable_memory: false,
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
    pub fn with_job_registry(mut self, registry: crate::agent::v2::jobs::JobRegistry) -> Self {
        self.job_registry = Some(registry);
        self
    }
    
    /// Add a single tool to the registry
    pub fn with_tool(mut self, tool: Box<dyn crate::agent::tool::Tool>) -> Self {
        self.pending_tools.push(tool);
        self
    }
    
    /// Add multiple tools to the registry
    pub fn with_tools(mut self, mut tools: Vec<Box<dyn crate::agent::tool::Tool>>) -> Self {
        self.pending_tools.append(&mut tools);
        self
    }
    
    /// Build the agent with the current configuration
    pub async fn build(self) -> BuiltAgent {
        // Register all pending tools
        for tool in self.pending_tools {
            let _ = self.tool_registry.register_tool(tool).await;
        }

        // Get all tools from the registry
        let tools_list = self.tool_registry.get_all_tools().await;
        
        match self.version {
            AgentVersion::V2 => {
                let scribe = self.scribe.expect("Scribe is required for Agent V2");

                BuiltAgent::V2(AgentV2::new_with_iterations(
                    self.llm_client,
                    scribe,
                    tools_list,
                    self.system_prompt_prefix,
                    self.max_iterations,
                    self.version,
                    self.memory_store,
                    self.categorizer,
                    self.job_registry, // JobRegistry
                    None, // capabilities_context
                    None, // scratchpad
                    self.disable_memory,
                ))
            },
            AgentVersion::V1 => {
                BuiltAgent::V1(Agent::new_with_iterations(
                    self.llm_client,
                    tools_list,
                    self.system_prompt_prefix,
                    self.max_iterations,
                    self.version,
                    self.memory_store,
                    self.categorizer,
                    self.job_registry, // job_registry
                    None, // scratchpad
                    self.disable_memory,
                ).await)
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
