//! Agent Session Factory
//!
//! Creates agent sessions from unified Config.
//! This is the main entry point for creating agent instances.

use std::sync::Arc;

use crate::config::{Config, BridgeError, config_to_llm_config, config_to_kernel_config};
use crate::provider::LlmClient;
use crate::agent::{
    // Session types
    runtime::orchestrator::orchestrator::AgencySession,
    runtime::orchestrator::ContractRuntime,
    runtime::capabilities::InMemoryTransport,
    tools::{ToolRegistry, DelegateTool, ChunkPool},
    runtime::core::terminal::TerminalExecutor,
    runtime::core::ApprovalCapability,
    runtime::core::LLMCapability,
    // Coordination
    runtime::orchestrator::commonbox::Commonbox,
    // Cognition
    cognition::Planner,
    cognition::prompts::system::ToolDescription,
    // Memory
    memory::AgentMemoryManager,
};

/// Factory for creating agent sessions from configuration
/// 
/// This is the main entry point for creating agent instances.
/// It wires together all components based on the unified Config.
/// 
/// # Example
/// ```
/// use mylm_core::agent::AgentSessionFactory;
/// use mylm_core::config::Config;
///
/// let config = Config::load_or_default();
/// let factory = AgentSessionFactory::new(config);
/// let session = factory.create_session("default").await?;
/// ```
/// Factory for creating agent sessions
/// 
/// Note: Clone is derived for creating child factories (e.g., for workers)
/// without sharing the commonbox (to avoid circular dependencies).
#[derive(Clone)]
pub struct AgentSessionFactory {
    config: Config,
    terminal: Option<Arc<dyn TerminalExecutor>>,
    approval: Option<Arc<dyn ApprovalCapability>>,
    /// Custom LLM capability for testing (optional)
    llm: Option<Arc<dyn LLMCapability>>,
    /// Commonbox for worker coordination (optional, enables delegate tool)
    /// Not cloned - workers get their own factory without commonbox
    #[allow(clippy::skip_vec_init)]
    commonbox: Option<Arc<Commonbox>>,
}

/// Configuration for worker session creation
#[derive(Debug, Clone)]
pub struct WorkerSessionConfig {
    /// Pre-approved tools for this worker
    pub allowed_tools: Vec<String>,
    /// Auto-approved command patterns (e.g., ["ls -la", "cargo check *"])
    pub allowed_commands: Vec<String>,
    /// Forbidden command patterns (e.g., ["rm -rf *", "sudo *"])
    pub forbidden_commands: Vec<String>,
    /// Initial scratchpad content
    pub scratchpad: Option<String>,
    /// Output channel for worker events
    pub output_tx: Option<tokio::sync::mpsc::Sender<crate::agent::runtime::orchestrator::OutputEvent>>,
    /// Worker's objective
    pub objective: String,
    /// Instructions for the worker
    pub instructions: Option<String>,
    /// Tags for categorization
    pub tags: Option<Vec<String>>,
    /// Commonbox for coordination (optional, enables commonboard tool)
    pub commonbox: Option<Arc<Commonbox>>,
}

/// Error type for factory operations
#[derive(Debug, thiserror::Error)]
pub enum FactoryError {
    #[error("Configuration error: {0}")]
    Config(#[from] BridgeError),
    
    #[error("LLM client error: {0}")]
    Llm(#[from] anyhow::Error),
    
    #[error("Session creation failed: {0}")]
    Creation(String),
}

impl AgentSessionFactory {
    /// Create a new factory with the given configuration
    pub fn new(config: Config) -> Self {
        Self { 
            config,
            terminal: None,
            approval: None,
            llm: None,
            commonbox: None,
        }
    }
    
    /// Enable worker spawning by providing a Commonbox
    /// 
    /// When set, the delegate tool will be available for spawning workers.
    pub fn with_commonbox(mut self, commonbox: Arc<Commonbox>) -> Self {
        self.commonbox = Some(commonbox);
        self
    }
    
    /// Set a custom LLM capability for testing
    /// 
    /// When set, this LLM capability will be used instead of creating
    /// a real LlmClient from configuration. Useful for testing with
    /// mock LLMs.
    pub fn with_llm(mut self, llm: Arc<dyn LLMCapability>) -> Self {
        self.llm = Some(llm);
        self
    }
    
    /// Set a custom terminal executor for the session
    /// 
    /// When running in TUI mode, this should be a TuiTerminalExecutor
    /// that shares the PTY session with the UI.
    pub fn with_terminal(mut self, terminal: Arc<dyn TerminalExecutor>) -> Self {
        self.terminal = Some(terminal);
        self
    }
    
    /// Set a custom approval capability for the session
    /// 
    /// When running in TUI mode, this should be a TuiApprovalCapability
    /// that allows interactive tool approval.
    pub fn with_approval(mut self, approval: Arc<dyn ApprovalCapability>) -> Self {
        self.approval = Some(approval);
        self
    }
    
    /// Create ContractRuntime with optional custom LLM and memory provider
    fn create_runtime(
        &self, 
        llm_client: Arc<LlmClient>, 
        tools: Arc<ToolRegistry>,
        memory_provider: Option<Arc<dyn crate::agent::memory::MemoryProvider>>,
    ) -> ContractRuntime {
        match &self.llm {
            Some(custom_llm) => {
                crate::info_log!("[FACTORY] Using custom LLM capability");
                // Custom LLM doesn't support memory injection currently
                if memory_provider.is_some() {
                    crate::warn_log!("[FACTORY] Memory provider not supported with custom LLM capability");
                }
                ContractRuntime::with_llm_capability(Arc::clone(custom_llm), tools)
            }
            None => {
                ContractRuntime::with_tools_and_memory(llm_client, tools, memory_provider)
            }
        }
    }
    
    /// Create a new session for the specified profile
    /// 
    /// # Arguments
    /// * `profile_name` - Name of the profile to use (e.g., "default", "worker")
    /// 
    /// # Returns
    /// A fully configured `AgencySession` ready to run
    pub async fn create_session(
        &self,
        profile_name: &str,
    ) -> Result<
        AgencySession<
            Planner,
            ContractRuntime,
            InMemoryTransport,
        >,
        FactoryError,
    > {
        // Step 1: Create LLM config from unified Config
        let llm_config = config_to_llm_config(&self.config, profile_name)?;
        
        // Step 2: Create LLM client
        let llm_client = Arc::new(LlmClient::new(llm_config)?);
        
        // Step 3: Create output channel for streaming events FIRST
        // (needed for both runtime and delegate tool)
        let (output_tx, _) = tokio::sync::broadcast::channel(100);
        
        // Step 4: Create ChunkPool for large file reading
        // Get max_persistent_workers from agent config (default 5)
        let agent_config = crate::config::agent::AgentConfig::load();
        let max_persistent_workers = agent_config.workers.max_persistent_workers;
        let session_id = uuid::Uuid::new_v4().to_string();
        let chunk_pool = Arc::new(ChunkPool::new(&session_id, max_persistent_workers));
        crate::info_log!("[FACTORY] Created chunk pool with max {} workers for session {}", max_persistent_workers, session_id);
        
        // Step 5: Create ToolRegistry with chunk pool
        let tool_registry = ToolRegistry::with_chunk_pool(Arc::clone(&chunk_pool));
        
        // Step 5a: Add scratchpad tool for agent-local persistent notes
        let scratchpad = crate::agent::tools::ScratchpadTool::new_standalone();
        let tool_registry = tool_registry.with_scratchpad(scratchpad);
        
        // Step 5b: Add search_files tool for full-text file search
        let tool_registry = match ToolRegistry::with_chunk_pool(Arc::clone(&chunk_pool))
            .with_scratchpad(crate::agent::tools::ScratchpadTool::new_standalone())
            .with_search_files(None) {
            Ok(registry) => {
                crate::info_log!("[FACTORY] Enabled search_files tool");
                registry
            }
            Err(e) => {
                crate::warn_log!("[FACTORY] Failed to enable search_files: {}", e);
                tool_registry
            }
        };
        
        // Step 5c: Add delegate tool if commonbox is configured (enables worker spawning)
        let tool_registry = if let Some(ref commonbox) = self.commonbox {
            crate::info_log!("[FACTORY] Enabling delegate tool for worker spawning");
            
            // Create worker factory (same config but without commonbox to avoid recursion)
            let worker_factory = Self {
                config: self.config.clone(),
                terminal: self.terminal.clone(),
                approval: self.approval.clone(),
                llm: self.llm.clone(),
                commonbox: None,
            };
            
            // Create delegate tool with output sender for worker events
            let delegate = DelegateTool::new(
                Arc::clone(commonbox),
                worker_factory,
            ).with_output_sender(crate::agent::runtime::orchestrator::OutputSender::Broadcast(output_tx.clone()));
            
            // Create new registry with delegate and scratchpad enabled (with chunk pool)
            ToolRegistry::with_chunk_pool(Arc::clone(&chunk_pool))
                .with_scratchpad(crate::agent::tools::ScratchpadTool::new_standalone())
                .with_delegate(Arc::new(delegate))
        } else {
            tool_registry
        };
        
        let tool_descriptions: Vec<ToolDescription> = tool_registry.descriptions()
            .into_iter()
            .map(|d| d.into())
            .collect();
        crate::info_log!("[FACTORY] Available tools: {:?}", tool_descriptions.iter().map(|d| &d.name).collect::<Vec<_>>());
        
        // Step 5: Create memory manager and provider (if enabled)
        use crate::config::agent::MemoryConfig;
        use crate::agent::memory::AgentMemoryProvider;
        crate::info_log!("[FACTORY] features.memory = {}", self.config.features.memory);
        let memory_config = if self.config.features.memory {
            MemoryConfig {
                enabled: true,
                ..MemoryConfig::default()
            }
        } else {
            MemoryConfig {
                enabled: false,
                ..MemoryConfig::default()
            }
        };
        let memory_manager = if memory_config.enabled {
            crate::info_log!("[FACTORY] Creating memory manager...");
            match AgentMemoryManager::new(memory_config).await {
                Ok(mm) => {
                    crate::info_log!("[FACTORY] Memory manager initialized successfully");
                    Some(Arc::new(mm))
                }
                Err(e) => {
                    crate::warn_log!("[FACTORY] Failed to initialize memory manager: {}", e);
                    None
                }
            }
        } else {
            crate::info_log!("[FACTORY] Memory is disabled in config");
            None
        };
        
        // Create memory provider wrapper if manager exists
        let memory_provider: Option<Arc<dyn crate::agent::memory::MemoryProvider>> = memory_manager
            .as_ref()
            .map(|mm| Arc::new(AgentMemoryProvider::new(Arc::clone(mm))) as Arc<dyn crate::agent::memory::MemoryProvider>);
        
        // Step 6: Create ContractRuntime with LLM client, tools, memory provider, and output sender
        let mut runtime = self.create_runtime(llm_client.clone(), Arc::new(tool_registry), memory_provider)
            .with_output_sender(output_tx.clone());
        
        // Step 7: Attach terminal executor if provided
        if let Some(ref terminal) = self.terminal {
            crate::info_log!("[FACTORY] Attaching terminal executor to runtime");
            runtime = runtime.with_terminal(Arc::clone(terminal));
        }
        
        // Step 8: Attach approval capability if provided
        if let Some(ref approval) = self.approval {
            crate::info_log!("[FACTORY] Attaching approval capability to runtime");
            runtime = runtime.with_approval(Arc::clone(approval));
        }
        
        // Step 9: Create kernel config from profile
        let _kernel_config = config_to_kernel_config(&self.config, profile_name)?;
        
        // Step 10: Create planner directly with dynamic tools
        // NOTE: Memory is now handled at runtime layer via Intent::Remember
        let kernel = Planner::new()
            .with_tool_descriptions(tool_descriptions);
        
        // Step 11: Create in-memory transport
        let transport = InMemoryTransport::new(100);
        
        // Step 12: Assemble the session with shared output channel, memory manager, and chunk pool
        // CRITICAL: memory_manager and chunk_pool are passed to session which owns them for its lifetime
        // The runtime's MemoryProvider and tools hold references - this ensures the Arcs stay alive
        let session = AgencySession::new_full(kernel, runtime, transport, output_tx, memory_manager, Some(chunk_pool));
        
        Ok(session)
    }
    
    /// Create a session for the default (main) profile
    pub async fn create_default_session(
        &self,
    ) -> Result<
        AgencySession<
            Planner,
            ContractRuntime,
            InMemoryTransport,
        >,
        FactoryError,
    > {
        let profile = self.config.active_profile.clone();
        self.create_session(&profile).await
    }
    
    /// Create a session for the worker profile
    pub async fn create_worker_session(
        &self,
    ) -> Result<
        AgencySession<
            Planner,
            ContractRuntime,
            InMemoryTransport,
        >,
        FactoryError,
    > {
        self.create_session("worker").await
    }
    
    /// Create a worker session with specific configuration
    ///
    /// Applies worker-specific settings including:
    /// - Tool restrictions (allowed_tools) - filters tool descriptions given to LLM
    /// - Worker objective and instructions
    /// - Terminal and approval capabilities from factory
    pub async fn create_configured_worker_session(
        &self,
        worker_id: &str,
        config: WorkerSessionConfig,
    ) -> Result<
        AgencySession<
            Planner,
            ContractRuntime,
            InMemoryTransport,
        >,
        FactoryError,
    > {
        crate::info_log!("[FACTORY] Creating configured worker session for {} with {} allowed tools", 
            worker_id, config.allowed_tools.len());
        
        // Step 1: Create LLM config from unified Config
        let llm_config = config_to_llm_config(&self.config, "worker")?;
        let llm_client = Arc::new(LlmClient::new(llm_config)?);
        
        // Step 2: Create tool registry with all tools + agent-local scratchpad + commonboard
        let tool_registry = ToolRegistry::new()
            .with_scratchpad(crate::agent::tools::ScratchpadTool::new_standalone());
        
        // Add commonboard if commonbox is available (for coordination)
        // Note: Use config.commonbox (passed from parent) rather than self.commonbox
        // Workers get commonbox for coordination but can't spawn more workers (factory has commonbox: None)
        let commonbox_for_coordination = config.commonbox.as_ref().or(self.commonbox.as_ref());
        let tool_registry = if let Some(commonbox) = commonbox_for_coordination {
            let commonboard = crate::agent::tools::CommonboardTool::new(Arc::clone(commonbox));
            tool_registry.with_commonboard(commonboard)
        } else {
            tool_registry
        };
        
        // Step 3: Filter tool descriptions based on allowed_tools
        let all_descriptions = tool_registry.descriptions();
        let filtered_descriptions: Vec<ToolDescription> = if config.allowed_tools.is_empty() {
            // No restrictions - use all tools
            all_descriptions.into_iter().map(|d| d.into()).collect()
        } else {
            // Filter to only allowed tools
            let allowed: std::collections::HashSet<String> = config.allowed_tools.iter().cloned().collect();
            all_descriptions
                .into_iter()
                .filter(|d| allowed.iter().any(|a| a == d.name))
                .map(|d| d.into())
                .collect()
        };
        
        crate::info_log!("[FACTORY] Worker {} allowed tools: {:?}", 
            worker_id, filtered_descriptions.iter().map(|d| &d.name).collect::<Vec<_>>());
        
        // Step 4: Use provided output channel or create new one
        // If config.output_tx is provided (mpsc from parent), use broadcast channel for session
        // and bridge events to the mpsc channel
        let (broadcast_tx, _) = tokio::sync::broadcast::channel(100);
        
        // Step 5: Create ContractRuntime (workers don't use memory)
        let mut runtime = self.create_runtime(llm_client.clone(), Arc::new(tool_registry), None)
            .with_output_sender(broadcast_tx.clone());
        
        // Step 6: Attach terminal executor if provided
        if let Some(ref terminal) = self.terminal {
            crate::info_log!("[FACTORY] Attaching terminal executor to worker runtime");
            runtime = runtime.with_terminal(Arc::clone(terminal));
        }
        
        // Step 7: Workers use restricted approval based on allowed/forbidden command patterns
        // Commands matching allowed_commands → auto-approved
        // Commands matching forbidden_commands → auto-denied
        // Everything else → auto-denied (escalation to parent can be added later)
        crate::info_log!(
            "[FACTORY] Attaching restricted approval to worker: allowed={:?}, forbidden={:?}",
            config.allowed_commands, config.forbidden_commands
        );
        runtime = runtime.with_approval(Arc::new(
            crate::agent::runtime::capabilities::WorkerRestrictedApprovalCapability::new(
                config.allowed_commands.clone(),
                config.forbidden_commands.clone(),
            )
        ));
        
        // Step 8: Create kernel with filtered tool descriptions
        crate::info_log!("[FACTORY] Creating kernel with {} tool descriptions", filtered_descriptions.len());
        let kernel = Planner::new()
            .with_tool_descriptions(filtered_descriptions);
        crate::info_log!("[FACTORY] Kernel created successfully");
        
        // Step 9: Create transport
        let transport = InMemoryTransport::new(100);
        crate::info_log!("[FACTORY] Created transport at {:p}, passing to session", &transport);
        
        // Step 10: Assemble session with broadcast channel
        let session = AgencySession::new_with_memory(kernel, runtime, transport, broadcast_tx.clone(), None);
        crate::info_log!("[FACTORY] Session created at {:p}", &session);
        
        // Step 11: If parent provided mpsc channel, spawn bridging task
        if let Some(ref mpsc_tx) = config.output_tx {
            crate::info_log!("[FACTORY] Bridging worker events to parent mpsc channel");
            let mut broadcast_rx = broadcast_tx.subscribe();
            let mpsc_tx = mpsc_tx.clone();
            tokio::spawn(async move {
                while let Ok(event) = broadcast_rx.recv().await {
                    if mpsc_tx.send(event).await.is_err() {
                        break; // Parent dropped receiver
                    }
                }
            });
        }
        
        crate::info_log!("[FACTORY] Worker session created for {}", worker_id);
        Ok(session)
    }
    
    /// Get a reference to the config
    pub fn config(&self) -> &Config {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProfileConfig, ProviderConfig, ProviderType};
    
    fn create_test_config() -> Config {
        let mut config = Config::default();
        
        // Add test provider
        config.providers.insert(
            "test-provider".to_string(),
            ProviderConfig {
                provider_type: ProviderType::OpenAi,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: Some("test-key".to_string()),
                default_model: "gpt-4o-mini".to_string(),
                models: vec!["gpt-4o-mini".to_string()],
                timeout_secs: 120,
            },
        );
        
        // Add test profile
        config.profiles.insert(
            "test".to_string(),
            ProfileConfig {
                provider: "test-provider".to_string(),
                model: Some("gpt-4o-mini".to_string()),
                max_iterations: 10,
                rate_limit_rpm: 60,
                context_window: 4096,
                temperature: 0.7,
                system_prompt: None,
                condense_threshold: None,
                input_price: None,
                output_price: None,
                tested_at: None,
                test_error: None,
                web_search: crate::config::WebSearchConfig::default(),
            },
        );
        
        config
    }
    
    #[tokio::test]
    async fn test_factory_creation() {
        let config = create_test_config();
        let factory = AgentSessionFactory::new(config);
        
        // Just verify it doesn't panic during creation
        // Actual session creation requires network/LLM
        // Config::default() creates 1 profile, plus we add 1 more = 2 total
        assert_eq!(factory.config().profiles.len(), 2);
    }
}
