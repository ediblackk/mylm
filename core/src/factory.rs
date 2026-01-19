use std::sync::Arc;
use tokio::sync::mpsc;
use crate::config::Config;
use crate::agent::{Agent, Tool};
use crate::agent::v2::core::AgentV2;
use crate::llm::{LlmClient, LlmConfig};
use crate::terminal::app::TuiEvent;
use crate::memory::store::VectorStore;
use crate::memory::categorizer::MemoryCategorizer;
use crate::state::StateStore;
use crate::executor::{CommandExecutor, allowlist::CommandAllowlist, safety::SafetyChecker};
use crate::context::TerminalContext;

pub async fn create_agent_for_session(
    config: &Config,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) -> anyhow::Result<Agent> {
    let endpoint_config = config.get_endpoint(None)?;

    let llm_config = LlmConfig::new(
        endpoint_config.provider.parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        endpoint_config.base_url.clone(),
        endpoint_config.model.clone(),
        Some(endpoint_config.api_key.clone()),
    )
    .with_memory(config.memory.clone());
    
    let client = Arc::new(LlmClient::new(llm_config)?);

    // Context
    let ctx = TerminalContext::collect().await;

    // Memory Store
    let data_dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?
        .join("mylm")
        .join("memory");
    std::fs::create_dir_all(&data_dir)?;
    let store = Arc::new(VectorStore::new(data_dir.to_str().unwrap()).await?);

    // State Store
    let state_store = Arc::new(std::sync::RwLock::new(StateStore::new()?));

    // Executor
    let mut allowlist = CommandAllowlist::new();
    allowlist.apply_config(&config.commands);
    let safety_checker = SafetyChecker::new();
    let executor = Arc::new(CommandExecutor::new(allowlist, safety_checker));

    // Categorizer
    let categorizer = Arc::new(MemoryCategorizer::new(client.clone(), store.clone()));

    // Tools
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(crate::agent::tools::shell::ShellTool::new(
            executor,
            ctx.clone(),
            event_tx.clone(),
            Some(store.clone()),
            Some(categorizer.clone()),
            None,
            None // JobRegistry not available in Agent v1
        )),
        Box::new(crate::agent::tools::web_search::WebSearchTool::new(
            config.web_search.clone(), 
            event_tx.clone()
        )),
        Box::new(crate::agent::tools::memory::MemoryTool::new(store.clone())),
        Box::new(crate::agent::tools::crawl::CrawlTool::new(event_tx.clone())),
        Box::new(crate::agent::tools::fs::FileReadTool),
        Box::new(crate::agent::tools::fs::FileWriteTool),
        Box::new(crate::agent::tools::git::GitStatusTool),
        Box::new(crate::agent::tools::git::GitLogTool),
        Box::new(crate::agent::tools::git::GitDiffTool),
        Box::new(crate::agent::tools::state::StateTool::new(state_store.clone())),
        Box::new(crate::agent::tools::system::SystemMonitorTool::new()),
        Box::new(crate::agent::tools::terminal_sight::TerminalSightTool::new(event_tx.clone())),
        Box::new(crate::agent::tools::wait::WaitTool),
    ];

    let system_prompt = crate::config::prompt::build_system_prompt(&ctx, "default", Some("WebSocket Session")).await?;

    Ok(Agent::new_with_iterations(
        client,
        tools,
        system_prompt,
        config.agent.max_iterations,
        config.agent.version,
        Some(store),
        Some(categorizer),
    ))
}

pub async fn create_agent_v2_for_session(
    config: &Config,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) -> anyhow::Result<AgentV2> {
    let endpoint_config = config.get_endpoint(None)?;

    let llm_config = LlmConfig::new(
        endpoint_config.provider.parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        endpoint_config.base_url.clone(),
        endpoint_config.model.clone(),
        Some(endpoint_config.api_key.clone()),
    )
    .with_memory(config.memory.clone());
    
    let client = Arc::new(LlmClient::new(llm_config)?);

    // Context
    let ctx = TerminalContext::collect().await;

    // Memory Store
    let data_dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?
        .join("mylm")
        .join("memory");
    std::fs::create_dir_all(&data_dir)?;
    let store = Arc::new(VectorStore::new(data_dir.to_str().unwrap()).await?);

    // State Store
    let state_store = Arc::new(std::sync::RwLock::new(StateStore::new()?));

    // Executor
    let mut allowlist = CommandAllowlist::new();
    allowlist.apply_config(&config.commands);
    let safety_checker = SafetyChecker::new();
    let executor = Arc::new(CommandExecutor::new(allowlist, safety_checker));

    // Categorizer
    let categorizer = Arc::new(MemoryCategorizer::new(client.clone(), store.clone()));

    // Journal for Scribe
    let journal = Arc::new(tokio::sync::Mutex::new(crate::memory::journal::Journal::new()?));

    // Scribe for AgentV2
    let scribe = Arc::new(crate::memory::scribe::Scribe::new(journal, store.clone(), client.clone()));

    // Job Registry (Shared between Agent and Tools)
    let job_registry = crate::agent::v2::jobs::JobRegistry::new();

    // Tools for AgentV2 - includes the new Delegate tool
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(crate::agent::tools::shell::ShellTool::new(
            executor,
            ctx.clone(),
            event_tx.clone(),
            Some(store.clone()),
            Some(categorizer.clone()),
            None,
            Some(job_registry.clone()) // Share registry with ShellTool if needed
        )),
        Box::new(crate::agent::tools::web_search::WebSearchTool::new(
            config.web_search.clone(),
            event_tx.clone()
        )),
        Box::new(crate::agent::tools::memory::MemoryTool::new(store.clone())),
        Box::new(crate::agent::tools::crawl::CrawlTool::new(event_tx.clone())),
        Box::new(crate::agent::tools::fs::FileReadTool),
        Box::new(crate::agent::tools::fs::FileWriteTool),
        Box::new(crate::agent::tools::git::GitStatusTool),
        Box::new(crate::agent::tools::git::GitLogTool),
        Box::new(crate::agent::tools::git::GitDiffTool),
        Box::new(crate::agent::tools::state::StateTool::new(state_store.clone())),
        Box::new(crate::agent::tools::system::SystemMonitorTool::new()),
        Box::new(crate::agent::tools::terminal_sight::TerminalSightTool::new(event_tx.clone())),
        Box::new(crate::agent::tools::wait::WaitTool),
        Box::new(crate::agent::tools::delegate::DelegateTool::new(
            client.clone(),
            scribe.clone(),
            job_registry.clone(), // Share registry with DelegateTool
            Some(store.clone()),
            Some(categorizer.clone()),
            None // Event tx will be set by AgentV2
        )),
    ];

    let system_prompt = crate::config::prompt::build_system_prompt(&ctx, "default", Some("WebSocket Session")).await?;

    Ok(AgentV2::new_with_iterations(
        client,
        scribe,
        tools,
        system_prompt,
        config.agent.max_iterations,
        config.agent.version,
        Some(store),
        Some(categorizer),
        Some(job_registry),
    ))
}
