use std::sync::Arc;
use tokio::sync::mpsc;
use crate::config::Config;
use crate::agent::{Agent, Tool};
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
    let allowlist = CommandAllowlist::new();
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
            None
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
