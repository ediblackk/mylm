use std::sync::Arc;
use tokio::sync::mpsc;
use crate::config::Config;
use crate::agent::{Agent, Tool};
use crate::config::AgentVersion;
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
) -> anyhow::Result<crate::agent::factory::BuiltAgent> {
    // Resolve configuration with profile overrides
    let resolved = config.resolve_profile();
    
    let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
    let api_key = resolved.api_key.unwrap_or_default();

    let llm_config = LlmConfig::new(
        format!("{:?}", resolved.provider).to_lowercase().parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        base_url.clone(),
        resolved.model.clone(),
        Some(api_key.clone()),
    )
    .with_memory(config.features.memory.clone());
    
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
    let allowlist = CommandAllowlist::new();
    let safety_checker = SafetyChecker::new();
    let executor = Arc::new(CommandExecutor::new(allowlist, safety_checker));

    // Categorizer
    let categorizer = Arc::new(MemoryCategorizer::new(client.clone(), store.clone()));

    // Tools - convert Box to Arc for the agent
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(crate::agent::tools::shell::ShellTool::new(
            executor,
            ctx.clone(),
            event_tx.clone(),
            Some(store.clone()),
            Some(categorizer.clone()),
            None,
            None // JobRegistry not available in Agent v1
        )),
        Arc::new(crate::agent::tools::web_search::WebSearchTool::new(
            config.features.web_search.clone(), 
            event_tx.clone()
        )),
        Arc::new(crate::agent::tools::memory::MemoryTool::new(store.clone())),
        Arc::new(crate::agent::tools::crawl::CrawlTool::new(event_tx.clone())),
        Arc::new(crate::agent::tools::fs::FileReadTool),
        Arc::new(crate::agent::tools::fs::FileWriteTool),
        Arc::new(crate::agent::tools::git::GitStatusTool),
        Arc::new(crate::agent::tools::git::GitLogTool),
        Arc::new(crate::agent::tools::git::GitDiffTool),
        Arc::new(crate::agent::tools::state::StateTool::new(state_store.clone())),
        Arc::new(crate::agent::tools::system::SystemMonitorTool::new()),
        Arc::new(crate::agent::tools::terminal_sight::TerminalSightTool::new(event_tx.clone())),
        Arc::new(crate::agent::tools::wait::WaitTool),
    ];


    let system_prompt = crate::config::build_system_prompt(
        &ctx,
        "default",
        Some("WebSocket Session"),
        Some(&config.features.prompts)
    ).await?;

    // Determine agent version based on profile settings
    let agent_version = if config.profiles.get(&config.profile).and_then(|p| p.agent.as_ref()).is_some() {
        AgentVersion::V2
    } else {
        AgentVersion::V1
    };

    match agent_version {
        AgentVersion::V2 => {
            let agent = create_agent_v2_for_session(config, event_tx).await?;
            Ok(crate::agent::factory::BuiltAgent::V2(agent))
        }
        AgentVersion::V1 => {
            let agent = Agent::new_with_iterations(
                client,
                tools,
                system_prompt,
                resolved.agent.max_iterations,
                agent_version,
                Some(store),
                Some(categorizer),
                None, // job_registry
                None, // scratchpad
            ).await;
            Ok(crate::agent::factory::BuiltAgent::V1(agent))
        }
    }
}

pub async fn create_agent_v2_for_session(
    config: &Config,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) -> anyhow::Result<AgentV2> {
    // Resolve configuration with profile overrides
    let resolved = config.resolve_profile();
    
    let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
    let api_key = resolved.api_key.unwrap_or_default();

    let llm_config = LlmConfig::new(
        format!("{:?}", resolved.provider).to_lowercase().parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        base_url.clone(),
        resolved.model.clone(),
        Some(api_key.clone()),
    )
    .with_memory(config.features.memory.clone());
    
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
    let allowlist = CommandAllowlist::new();
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
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(crate::agent::tools::shell::ShellTool::new(
            executor,
            ctx.clone(),
            event_tx.clone(),
            Some(store.clone()),
            Some(categorizer.clone()),
            None,
            Some(job_registry.clone()) // Share registry with ShellTool if needed
        )),
        Arc::new(crate::agent::tools::web_search::WebSearchTool::new(
            config.features.web_search.clone(),
            event_tx.clone()
        )),
        Arc::new(crate::agent::tools::memory::MemoryTool::new(store.clone())),
        Arc::new(crate::agent::tools::crawl::CrawlTool::new(event_tx.clone())),
        Arc::new(crate::agent::tools::fs::FileReadTool),
        Arc::new(crate::agent::tools::fs::FileWriteTool),
        Arc::new(crate::agent::tools::git::GitStatusTool),
        Arc::new(crate::agent::tools::git::GitLogTool),
        Arc::new(crate::agent::tools::git::GitDiffTool),
        Arc::new(crate::agent::tools::state::StateTool::new(state_store.clone())),
        Arc::new(crate::agent::tools::system::SystemMonitorTool::new()),
        Arc::new(crate::agent::tools::terminal_sight::TerminalSightTool::new(event_tx.clone())),
        Arc::new(crate::agent::tools::wait::WaitTool),
        Arc::new(crate::agent::tools::delegate::DelegateTool::new(
            client.clone(),
            scribe.clone(),
            job_registry.clone(), // Share registry with DelegateTool
            Some(store.clone()),
            Some(categorizer.clone()),
            None // Event tx will be set by AgentV2
        )),
    ];


    let system_prompt = crate::config::build_system_prompt(
        &ctx,
        "default",
        Some("WebSocket Session"),
        Some(&config.features.prompts)
    ).await?;

    Ok(AgentV2::new_with_iterations(
        client,
        scribe,
        tools,
        system_prompt,
        resolved.agent.max_iterations,
        AgentVersion::V2,
        Some(store),
        Some(categorizer),
        Some(job_registry),
        None, // capabilities_context is already included in system_prompt
        None, // scratchpad
    ))
}
