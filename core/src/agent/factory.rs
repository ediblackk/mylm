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
    // Cognition
    cognition::LLMBasedEngine,
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
        Self { config }
    }
    
    /// Create a new session for the specified profile
    /// 
    /// # Arguments
    /// * `profile_name` - Name of the profile to use (e.g., "default", "worker")
    /// 
    /// # Returns
    /// A fully configured `AgencySession` ready to run
    pub fn create_session(
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
        let runtime = ContractRuntime::new(llm_client.clone())
            .with_output_sender(output_tx.clone());
        
        // Step 5: Create kernel config from profile
        let _kernel_config = config_to_kernel_config(&self.config, profile_name)?;
        
        // Step 6: Create cognitive engine (kernel implementation)
        let engine = LLMBasedEngine::new();
        
        // Step 7: Wrap engine in adapter to implement AgencyKernel trait
        let kernel = CognitiveEngineAdapter::new(engine);
        
        // Step 8: Create in-memory transport
        let transport = InMemoryTransport::new(100);
        
        // Step 9: Assemble the session with shared output channel
        let session = AgencySession::new_with_output(kernel, runtime, transport, output_tx);
        
        Ok(session)
    }
    
    /// Create a session for the default (main) profile
    pub fn create_default_session(
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
        self.create_session(&profile)
    }
    
    /// Create a session for the worker profile
    pub fn create_worker_session(
        &self,
    ) -> Result<
        AgencySession<
            CognitiveEngineAdapter<LLMBasedEngine>,
            ContractRuntime,
            InMemoryTransport,
        >,
        FactoryError,
    > {
        self.create_session("worker")
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
