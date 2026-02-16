//! Agent Session Factory
//!
//! Creates agent sessions from unified Config.
//! This is the main entry point for creating agent instances.

use std::sync::Arc;

use crate::config::{Config, BridgeError, config_to_llm_config, config_to_kernel_config};
use crate::llm::LlmClient;
use crate::agent::{
    // Contract types
    contract::session::AgencySession,
    cognition::kernel_adapter::CognitiveEngineAdapter,
    runtime::contract_runtime::ContractRuntime,
    runtime::impls::InMemoryTransport,
    runtime::terminal::TerminalExecutor,
    runtime::capability::ApprovalCapability,
    // Cognition
    cognition::LLMBasedEngine,
    // Memory
    memory::{AgentMemoryManager, AgentMemoryProvider},
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
pub struct AgentSessionFactory {
    config: Config,
    terminal: Option<Arc<dyn TerminalExecutor>>,
    approval: Option<Arc<dyn ApprovalCapability>>,
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
        }
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
            CognitiveEngineAdapter<LLMBasedEngine>,
            ContractRuntime,
            InMemoryTransport,
        >,
        FactoryError,
    > {
        // Step 1: Create LLM config from unified Config
        let llm_config = config_to_llm_config(&self.config, profile_name)?;
        
        // Step 2: Create LLM client
        let llm_client = Arc::new(LlmClient::new(llm_config)?);
        
        // Step 3: Create output channel for streaming events (shared between session and runtime)
        let (output_tx, _) = tokio::sync::broadcast::channel(100);
        
        // Step 4: Create ContractRuntime with LLM client and output sender
        let mut runtime = ContractRuntime::new(llm_client.clone())
            .with_output_sender(output_tx.clone());
        
        // Step 4b: Attach terminal executor if provided
        if let Some(ref terminal) = self.terminal {
            crate::info_log!("[FACTORY] Attaching terminal executor to runtime");
            runtime = runtime.with_terminal(Arc::clone(terminal));
        }
        
        // Step 4c: Attach approval capability if provided
        if let Some(ref approval) = self.approval {
            crate::info_log!("[FACTORY] Attaching approval capability to runtime");
            runtime = runtime.with_approval(Arc::clone(approval));
        }
        
        // Step 5: Create kernel config from profile
        let _kernel_config = config_to_kernel_config(&self.config, profile_name)?;
        
        // Step 6: Create memory manager and provider (if enabled)
        use crate::config::agent::MemoryConfig;
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
        
        // Step 7: Create cognitive engine with dynamic tools and memory provider
        let engine = if let Some(ref mm) = memory_manager {
            crate::info_log!("[FACTORY] Attaching memory provider to engine");
            let provider = Arc::new(AgentMemoryProvider::new(mm.clone()));
            LLMBasedEngine::new()
                .with_memory_provider(provider)
        } else {
            crate::info_log!("[FACTORY] Creating engine without memory provider");
            LLMBasedEngine::new()
        };
        
        // Step 8: Wrap engine in adapter to implement AgencyKernel trait
        let kernel = CognitiveEngineAdapter::new(engine);
        
        // Step 9: Create in-memory transport
        let transport = InMemoryTransport::new(100);
        
        // Step 10: Assemble the session with shared output channel and memory manager
        // CRITICAL: memory_manager is passed to session which owns it for the session lifetime
        // The engine's MemoryProvider holds a Weak reference - this ensures the Arc stays alive
        let session = AgencySession::new_with_memory(kernel, runtime, transport, output_tx, memory_manager);
        
        Ok(session)
    }
    
    /// Create a session for the default (main) profile
    pub async fn create_default_session(
        &self,
    ) -> Result<
        AgencySession<
            CognitiveEngineAdapter<LLMBasedEngine>,
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
            CognitiveEngineAdapter<LLMBasedEngine>,
            ContractRuntime,
            InMemoryTransport,
        >,
        FactoryError,
    > {
        self.create_session("worker").await
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
