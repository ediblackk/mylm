//! Agent initialization module using new runtime architecture
//!
//! This module provides helper functions for initializing agent components
//! using the new AgentSessionFactory and contract-based architecture.

use anyhow::{Context, Result};
use mylm_core::agent::factory::AgentSessionFactory;
use mylm_core::agent::runtime::capability::ApprovalCapability;
use mylm_core::agent::runtime::terminal::TerminalExecutor;
use mylm_core::config::Config;
use mylm_core::context::TerminalContext;
use mylm_core::executor::allowlist::CommandAllowlist;
use mylm_core::executor::safety::SafetyChecker;
use mylm_core::executor::CommandExecutor;
use mylm_core::llm::{LlmClient, LlmConfig};
use mylm_core::memory::VectorStore;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::tui::app::state::TuiEvent;

/// Re-export commonly used types for convenience
pub use mylm_core::agent::contract::session::{OutputEvent, UserInput};

/// Initialize LLM client from config
pub async fn create_llm_client(config: &Config) -> Result<Arc<LlmClient>> {
    let resolved = config.resolve_profile();
    let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
    let api_key = resolved.api_key.unwrap_or_default();

    let llm_config = LlmConfig::new(
        format!("{:?}", resolved.provider).to_lowercase().parse()
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        base_url.clone(),
        resolved.model.clone(),
        Some(api_key.clone()),
        resolved.agent.max_context_tokens,
    )
    .with_memory(config.features.memory.clone());
    
    Ok(Arc::new(LlmClient::new(llm_config)?))
}

/// Initialize command executor
pub fn create_executor() -> Arc<CommandExecutor> {
    let allowlist = CommandAllowlist::new();
    Arc::new(CommandExecutor::new(allowlist, SafetyChecker::new()))
}

/// Initialize memory store
pub async fn create_memory_store(
    base_data_dir: &std::path::Path,
    incognito: bool,
    incognito_dir: Option<&std::path::Path>,
) -> Result<Arc<VectorStore>> {
    let memory_path = if incognito {
        let memory_dir = incognito_dir.unwrap().join("memory");
        std::fs::create_dir_all(&memory_dir)?;
        memory_dir
    } else {
        let data_dir = base_data_dir.join("memory");
        std::fs::create_dir_all(&data_dir)?;
        data_dir
    };
    
    Ok(Arc::new(VectorStore::new(memory_path.to_str().unwrap()).await?))
}

/// Create AgentSessionFactory from config
/// 
/// Optionally provide custom terminal executor and approval capability.
/// If approval is None, auto-approve is used (suitable for non-interactive use).
pub fn create_session_factory(
    config: &Config,
    terminal: Option<std::sync::Arc<dyn TerminalExecutor>>,
    approval: Option<std::sync::Arc<dyn ApprovalCapability>>,
) -> AgentSessionFactory {
    let mut factory = AgentSessionFactory::new(config.clone());
    
    if let Some(terminal) = terminal {
        factory = factory.with_terminal(terminal);
    }
    
    if let Some(approval) = approval {
        factory = factory.with_approval(approval);
    }
    
    factory
}

/// Setup status callback for LLM client
pub fn setup_status_callback(
    llm_client: &LlmClient,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) {
    llm_client.set_status_callback(Arc::new(move |msg: &str| {
        let _ = event_tx.send(TuiEvent::StatusUpdate(msg.to_string()));
    }));
}

/// Collect terminal context
pub async fn collect_terminal_context() -> TerminalContext {
    TerminalContext::collect().await
}
