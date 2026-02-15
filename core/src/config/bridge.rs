//! Config Bridge
//!
//! Bridges the unified `Config` (store.rs) to agent/LLM components.
//! 
//! This module converts between:
//! - `Config` (profiles, providers) → `LlmConfig` (for LLM client)
//! - `Config` (profile settings) → `KernelConfig` (for agent kernel)
//! - `Config` (app settings) → `RuntimeConfig` (for agent runtime)

use crate::config::{Config, ProviderType};
use crate::llm::{LlmConfig, LlmProvider};
use crate::agent::contract::config::KernelConfig;
use crate::agent::contract::runtime::RuntimeConfig;

/// Error type for config bridge operations
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Profile '{0}' not found")]
    ProfileNotFound(String),
    
    #[error("Provider '{0}' not found for profile '{1}'")]
    ProviderNotFound(String, String),
    
    #[error("No model configured for profile '{0}'")]
    NoModelConfigured(String),
    
    #[error("Failed to parse provider type: {0}")]
    InvalidProvider(String),
}

/// Convert unified Config to LlmConfig for a specific profile
/// 
/// # Arguments
/// * `config` - The unified configuration
/// * `profile_name` - Name of the profile to use (e.g., "default", "worker")
/// 
/// # Returns
/// `LlmConfig` ready to use with `LlmClient::new()`
pub fn config_to_llm_config(
    config: &Config,
    profile_name: &str,
) -> Result<LlmConfig, BridgeError> {
    // Get the profile
    let profile = config.profiles.get(profile_name)
        .ok_or_else(|| BridgeError::ProfileNotFound(profile_name.to_string()))?;
    
    // Get the provider config
    let provider_cfg = config.providers.get(&profile.provider)
        .ok_or_else(|| BridgeError::ProviderNotFound(
            profile.provider.clone(),
            profile_name.to_string()
        ))?;
    
    // Get model (profile override or provider default)
    let model = profile.model.clone()
        .or_else(|| config.providers.get(&profile.provider).map(|p| p.default_model.clone()))
        .ok_or_else(|| BridgeError::NoModelConfigured(profile_name.to_string()))?;
    
    // Convert provider type
    let provider = provider_type_to_llm_provider(&provider_cfg.provider_type)?;
    
    // Build base LlmConfig
    let mut llm_config = LlmConfig::new(
        provider,
        provider_cfg.base_url.clone(),
        model,
        provider_cfg.api_key.clone(),
        profile.context_window,
    )
    .with_temperature(profile.temperature)
    .with_max_tokens(profile.context_window.min(u32::MAX as usize) as u32)
    .with_context_management(
        profile.context_window,
        profile.condense_threshold.map(|t| t as f64 / profile.context_window as f64)
            .unwrap_or(0.8),
    );
    
    // Add system prompt if configured
    if let Some(ref prompt) = profile.system_prompt {
        llm_config = llm_config.with_system_prompt(prompt.clone());
    }
    
    // Add pricing if configured
    if let (Some(input), Some(output)) = (profile.input_price, profile.output_price) {
        llm_config = llm_config.with_pricing(input, output);
    }
    
    // Store original provider type in extra_params for provider-specific header handling
    llm_config.extra_params.insert(
        "provider_type".to_string(),
        format!("{:?}", provider_cfg.provider_type).to_lowercase(),
    );
    
    // Enable web search if configured for this profile
    llm_config.web_search_enabled = profile.web_search.enabled;
    
    Ok(llm_config)
}

/// Convert ProviderType to LlmProvider
fn provider_type_to_llm_provider(pt: &ProviderType) -> Result<LlmProvider, BridgeError> {
    match pt {
        ProviderType::OpenAi => Ok(LlmProvider::OpenAiCompatible),
        ProviderType::Google => Ok(LlmProvider::GoogleGenerativeAi),
        ProviderType::Ollama => Ok(LlmProvider::OpenAiCompatible),
        ProviderType::OpenRouter => Ok(LlmProvider::OpenAiCompatible),
        ProviderType::Kimi => Ok(LlmProvider::MoonshotKimi),
        ProviderType::Custom => Ok(LlmProvider::OpenAiCompatible),
    }
}

/// Convert Config to KernelConfig for a specific profile
/// 
/// # Arguments
/// * `config` - The unified configuration  
/// * `profile_name` - Name of the profile to use
/// 
/// # Returns
/// `KernelConfig` for initializing the agent kernel
pub fn config_to_kernel_config(
    config: &Config,
    profile_name: &str,
) -> Result<KernelConfig, BridgeError> {
    use crate::agent::contract::config::PromptConfig;
    
    let profile = config.profiles.get(profile_name)
        .ok_or_else(|| BridgeError::ProfileNotFound(profile_name.to_string()))?;
    
    // Build prompt config with profile settings
    let prompt_config = PromptConfig {
        system_prefix: profile.system_prompt.clone()
            .unwrap_or_else(|| "You are a helpful AI assistant.".to_string()),
        include_tool_descriptions: true,
        include_examples: true,
        tool_format: crate::agent::contract::config::ToolFormat::Json,
        max_context_length: profile.context_window,
    };
    
    // Build kernel config from profile settings
    let mut kernel_config = KernelConfig::new()
        .with_max_steps(profile.max_iterations);
    
    kernel_config.prompt_config = prompt_config;
    
    Ok(kernel_config)
}

/// Convert Config to RuntimeConfig
/// 
/// # Returns
/// `RuntimeConfig` for the agent runtime
pub fn config_to_runtime_config(_config: &Config) -> RuntimeConfig {
    // Default runtime config - can be extended with app settings later
    RuntimeConfig::default()
}

/// Helper to get the "default" profile's LLM config
/// 
/// This is the main entry point for creating an LLM client
/// for the primary agent.
pub fn default_llm_config(config: &Config) -> Result<LlmConfig, BridgeError> {
    config_to_llm_config(config, &config.active_profile)
}

/// Helper to get the "worker" profile's LLM config
/// 
/// Used for worker/agentic tasks that may use a different model.
pub fn worker_llm_config(config: &Config) -> Result<LlmConfig, BridgeError> {
    config_to_llm_config(config, "worker")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProfileConfig, ProviderConfig};
    
    fn create_test_config() -> Config {
        let mut config = Config::default();
        
        // Add a test provider
        config.providers.insert(
            "openai".to_string(),
            ProviderConfig {
                provider_type: ProviderType::OpenAi,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: Some("test-key".to_string()),
                default_model: "gpt-4o".to_string(),
                models: vec!["gpt-4o".to_string()],
                timeout_secs: 120,
            },
        );
        
        // Add a test profile
        config.profiles.insert(
            "test".to_string(),
            ProfileConfig {
                provider: "openai".to_string(),
                model: Some("gpt-4o-mini".to_string()),
                max_iterations: 100,
                rate_limit_rpm: 60,
                context_window: 8192,
                temperature: 0.5,
                system_prompt: Some("You are a test assistant.".to_string()),
                condense_threshold: Some(6000),
                input_price: Some(0.5),
                output_price: Some(1.5),
                tested_at: None,
                test_error: None,
                web_search: crate::config::WebSearchConfig::default(),
            },
        );
        
        config
    }
    
    #[test]
    fn test_config_to_llm_config() {
        let config = create_test_config();
        let llm_config = config_to_llm_config(&config, "test").unwrap();
        
        assert_eq!(llm_config.model, "gpt-4o-mini");
        assert_eq!(llm_config.base_url, "https://api.openai.com/v1");
        assert_eq!(llm_config.api_key, Some("test-key".to_string()));
        assert_eq!(llm_config.max_context_tokens, 8192);
        assert_eq!(llm_config.temperature, Some(0.5));
        assert_eq!(llm_config.system_prompt, Some("You are a test assistant.".to_string()));
        assert_eq!(llm_config.input_price_per_1m, 0.5);
        assert_eq!(llm_config.output_price_per_1m, 1.5);
    }
    
    #[test]
    fn test_profile_not_found() {
        let config = create_test_config();
        let result = config_to_llm_config(&config, "nonexistent");
        assert!(matches!(result, Err(BridgeError::ProfileNotFound(_))));
    }
    
    #[test]
    fn test_provider_not_found() {
        let mut config = create_test_config();
        // Modify profile to use non-existent provider
        if let Some(profile) = config.profiles.get_mut("test") {
            profile.provider = "nonexistent".to_string();
        }
        
        let result = config_to_llm_config(&config, "test");
        assert!(matches!(result, Err(BridgeError::ProviderNotFound(_, _))));
    }
    
    #[test]
    fn test_uses_provider_default_model() {
        let mut config = create_test_config();
        // Remove model from profile, should fall back to provider default
        if let Some(profile) = config.profiles.get_mut("test") {
            profile.model = None;
        }
        
        let llm_config = config_to_llm_config(&config, "test").unwrap();
        assert_eq!(llm_config.model, "gpt-4o"); // Provider's default
    }
}
