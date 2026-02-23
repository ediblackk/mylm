//! Agent Builder
//!
//! Convenient builder pattern for constructing fully configured agents.
//!
//! # Example
//! ```ignore
//! use mylm_core::agent::AgentBuilder;
//! use mylm_core::agent::runtime::capabilities::{ToolRegistry, LlmClientCapability};
//! use std::sync::Arc;
//!
//! # async fn example() {
//! let llm_client = Arc::new(LlmClientCapability::new(/* your LlmClient */));
//! let agent = AgentBuilder::new()
//!     .with_llm(llm_client)
//!     .with_tools(ToolRegistry::new())
//!     .with_terminal_approval()
//!     .with_telemetry()
//!     .build_with_planner();
//! # }
//! ```

use crate::agent::{
    AgentRuntime, CapabilityGraph, Session, SessionConfig,
    StepEngine, LlmEngine,
};
use crate::agent::runtime::{
    LLMCapability, ToolCapability, ApprovalCapability, 
    WorkerCapability, TelemetryCapability,
};
use crate::agent::runtime::capabilities::{
    LlmClientCapability, TerminalApprovalCapability,
    AutoApproveCapability, LocalWorkerCapability, ConsoleTelemetry,
    MemoryCapability, AgentMemoryManager,
};
use crate::agent::tools::{ToolRegistry, WebSearchTool, WebSearchConfig, SearchProvider};
use crate::memory::store::VectorStore;
use crate::provider::LlmClient;
use std::sync::Arc;

/// Agent builder - constructs fully configured agents
pub struct AgentBuilder {
    llm: Option<Arc<dyn LLMCapability>>,
    tools: Option<Arc<dyn ToolCapability>>,
    approval: Option<Arc<dyn ApprovalCapability>>,
    workers: Option<Arc<dyn WorkerCapability>>,
    telemetry: Option<Arc<dyn TelemetryCapability>>,
    config: SessionConfig,
    engine: Option<Box<dyn StepEngine + Send>>,
    memory_manager: Option<Arc<AgentMemoryManager>>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            llm: None,
            tools: None,
            approval: None,
            workers: None,
            telemetry: None,
            config: SessionConfig::default(),
            engine: None,
            memory_manager: None,
        }
    }
    
    /// Add LLM capability from existing client
    pub fn with_llm_client(mut self, client: Arc<LlmClient>) -> Self {
        self.llm = Some(Arc::new(LlmClientCapability::new(client)));
        self
    }
    
    /// Add LLM capability
    pub fn with_llm(mut self, llm: Arc<dyn LLMCapability>) -> Self {
        self.llm = Some(llm);
        self
    }
    
    /// Add tool registry
    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Some(Arc::new(tools));
        self
    }
    
    /// Add tool capability directly
    pub fn with_tool_capability(mut self, tools: Arc<dyn ToolCapability>) -> Self {
        self.tools = Some(tools);
        self
    }
    
    /// Add terminal approval
    pub fn with_terminal_approval(mut self) -> Self {
        self.approval = Some(Arc::new(TerminalApprovalCapability::new()));
        self
    }
    
    /// Add auto-approval (for testing)
    pub fn with_auto_approve(mut self) -> Self {
        self.approval = Some(Arc::new(AutoApproveCapability::new()));
        self
    }
    
    /// Add approval capability directly
    pub fn with_approval(mut self, approval: Arc<dyn ApprovalCapability>) -> Self {
        self.approval = Some(approval);
        self
    }
    
    /// Add local workers
    pub fn with_local_workers(mut self) -> Self {
        self.workers = Some(Arc::new(LocalWorkerCapability::new()));
        self
    }
    
    /// Add worker capability directly
    pub fn with_workers(mut self, workers: Arc<dyn WorkerCapability>) -> Self {
        self.workers = Some(workers);
        self
    }
    
    /// Add console telemetry
    pub fn with_telemetry(mut self) -> Self {
        self.telemetry = Some(Arc::new(ConsoleTelemetry::new()));
        self
    }
    
    /// Add telemetry capability directly
    pub fn with_telemetry_capability(mut self, telemetry: Arc<dyn TelemetryCapability>) -> Self {
        self.telemetry = Some(telemetry);
        self
    }
    
    /// Add web search
    pub fn with_web_search(mut self, api_key: impl Into<String>) -> Self {
        // Store as tool since web search is a tool
        let web_search = Arc::new(WebSearchTool::with_config(WebSearchConfig {
            enabled: true,
            api_key: Some(api_key.into()),
            provider: SearchProvider::Kimi,
        }));
        self.tools = Some(web_search as Arc<dyn ToolCapability>);
        self
    }
    
    /// Add stub web search (for testing)
    /// 
    /// Note: This now uses ToolRegistry which has built-in stub behaviors.
    pub fn with_stub_web_search(mut self) -> Self {
        self.tools = Some(Arc::new(ToolRegistry::new()));
        self
    }
    
    /// Add memory capability using a pre-initialized memory manager
    /// 
    /// # Example
    /// ```rust,ignore
    /// let memory_manager = AgentMemoryManager::new(MemoryConfig::default()).await?;
    /// let agent = AgentBuilder::new()
    ///     .with_memory_manager(memory_manager)
    ///     .build_with_llm_engine();
    /// ```
    pub fn with_memory_manager(mut self, manager: Arc<AgentMemoryManager>) -> Self {
        self.memory_manager = Some(manager.clone());
        
        // Memory doubles as telemetry to record events
        let memory_capability = MemoryCapability::with_manager(manager);
        self.telemetry = Some(Arc::new(memory_capability) as Arc<dyn TelemetryCapability>);
        self
    }
    
    /// Add memory capability with a pre-initialized VectorStore
    /// 
    /// This is useful when you want to share the same VectorStore across multiple agents
    /// or when you've already initialized it elsewhere.
    pub fn with_memory_store(mut self, store: Arc<VectorStore>) -> Self {
        // Create memory capability with the store
        let memory_capability = Arc::new(MemoryCapability::with_manager(
            Arc::new(AgentMemoryManager::from_store(store))
        ));
        
        self.telemetry = Some(memory_capability.clone() as Arc<dyn TelemetryCapability>);
        self
    }
    
    /// Note: Use `with_memory_manager()` instead.
    /// This method has been removed as MemoryCapability now requires
    /// a properly initialized AgentMemoryManager.
    #[deprecated(since = "0.2.0", note = "Use with_memory_manager() instead")]
    pub fn with_memory(self) -> Self {
        self
    }
    
    /// Set session config
    pub fn with_config(mut self, config: SessionConfig) -> Self {
        self.config = config;
        self
    }
    
    /// Set custom engine
    pub fn with_engine(mut self, engine: Box<dyn StepEngine + Send>) -> Self {
        self.engine = Some(engine);
        self
    }
    
    /// Build the runtime (without session)
    pub fn build_runtime(&mut self) -> AgentRuntime {
        // Build tools, injecting memory tool if memory manager is available
        let tools: Arc<dyn ToolCapability> = if let Some(ref memory_manager) = self.memory_manager {
            // If we have a memory manager, create a ToolRegistry with memory enabled
            let store = memory_manager.vector_store().clone();
            let registry = ToolRegistry::new().with_memory(store);
            Arc::new(registry)
        } else {
            // Use existing tools or create default
            self.tools.clone().unwrap_or_else(|| Arc::new(ToolRegistry::new()))
        };
        
        let graph = CapabilityGraph::new(
            self.llm.clone().unwrap_or_else(|| Arc::new(crate::agent::runtime::stubs::StubLLM)),
            tools,
            self.approval.clone().unwrap_or_else(|| Arc::new(AutoApproveCapability::new())),
            self.workers.clone().unwrap_or_else(|| Arc::new(crate::agent::runtime::stubs::StubWorkers)),
            self.telemetry.clone().unwrap_or_else(|| Arc::new(crate::agent::runtime::stubs::StubTelemetry)),
        );
        
        AgentRuntime::new(graph)
    }
    
    /// Build a complete session with LLM-based step engine
    pub fn build_with_llm_engine(mut self) -> Session<LlmEngine> {
        let runtime = self.build_runtime();
        let engine = LlmEngine::new();
        
        Session::new(engine, runtime, self.config)
    }
    
    /// Build a complete session with custom engine
    pub fn build_with_engine<E: StepEngine>(mut self, engine: E) -> Session<E> {
        let runtime = self.build_runtime();
        Session::new(engine, runtime, self.config)
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Quick-start functions for common configurations
pub mod presets {
    use super::*;
    
    /// Create a testing agent with all stubs
    #[allow(deprecated)]
    pub fn testing_agent() -> Session<LlmEngine> {
        AgentBuilder::new()
            .with_auto_approve()
            .with_stub_web_search()
            .with_config(SessionConfig { max_steps: 10 })
            .build_with_llm_engine()
    }
    
    /// Create a terminal agent with full capabilities
    #[allow(deprecated)]
    pub fn terminal_agent(llm_client: Arc<LlmClient>) -> Session<LlmEngine> {
        AgentBuilder::new()
            .with_llm_client(llm_client)
            .with_tools(ToolRegistry::new())
            .with_terminal_approval()
            .with_local_workers()
            .with_telemetry()
            .with_memory()
            .build_with_llm_engine()
    }
    
    /// Create a headless agent (auto-approve, no terminal)
    #[allow(deprecated)]
    pub fn headless_agent(llm_client: Arc<LlmClient>) -> Session<LlmEngine> {
        AgentBuilder::new()
            .with_llm_client(llm_client)
            .with_tools(ToolRegistry::new())
            .with_auto_approve()
            .with_local_workers()
            .with_telemetry()
            .build_with_llm_engine()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_builder_chaining() {
        let _runtime = AgentBuilder::new()
            .with_auto_approve()
            .with_local_workers()
            .with_telemetry()
            .build_runtime();
        
        // Should compile and create runtime
    }
    
    #[tokio::test]
    async fn test_preset_testing_agent() {
        let mut _session = presets::testing_agent();
        
        use tokio::sync::mpsc;
        use crate::agent::SessionInput;
        
        let (tx, _rx) = mpsc::channel(10);
        tx.send(SessionInput::Chat("Hello".to_string())).await.ok();
        drop(tx);
        
        // Should run without errors (using stub LLM)
        // Note: This will fail with stub since no real LLM response
        // but it tests the wiring is correct
    }
}
