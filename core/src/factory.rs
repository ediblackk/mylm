use std::sync::Arc;
use tokio::sync::RwLock;
use crate::config::ConfigV2 as Config;
use crate::agent_old::{Agent, Tool};
use crate::config::AgentVersion;
use crate::agent_old::v2::core::AgentV2;
use crate::llm::{LlmClient, LlmConfig};
use crate::memory::store::VectorStore;
use crate::memory::categorizer::MemoryCategorizer;
use crate::state::StateStore;
use crate::executor::{CommandExecutor, allowlist::CommandAllowlist, safety::SafetyChecker};
use crate::context::TerminalContext;
use crate::rate_limiter::{RateLimiter, RateLimitConfig};
use crate::agent_old::tools::StructuredScratchpad;
use crate::agent_old::traits::TerminalExecutor;
use crate::agent_old::event_bus::EventBus;
use crate::agent_old::tools::worker_shell::{EscalationRequest, EscalationResponse};

pub async fn create_agent_for_session(
    config: &Config,
    event_bus: Arc<EventBus>,
    terminal: Arc<dyn TerminalExecutor>,
    _escalation_tx: Option<tokio::sync::mpsc::Sender<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>>,
) -> anyhow::Result<crate::agent_old::v2::driver::factory::BuiltAgent> {
    // Resolve configuration with profile overrides
    let resolved = config.resolve_profile();
    
    let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
    let api_key = resolved.api_key.unwrap_or_default();

    let llm_config = LlmConfig::new(
        format!("{:?}", resolved.provider).to_lowercase().parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        base_url.clone(),
        resolved.model.clone(),
        Some(api_key.clone()),
        resolved.agent.max_context_tokens,
    )
    .with_memory(config.features.memory.clone());
    
    // Create rate limiter from config
    let rate_limit_config = RateLimitConfig::from_settings(
        Some(resolved.agent.main_rpm),
        Some(resolved.agent.workers_rpm),
    );
    let rate_limiter = Arc::new(RateLimiter::new(rate_limit_config));
    
    let client = Arc::new(LlmClient::new(llm_config)?.with_rate_limiter(rate_limiter.clone()));

    // Context
    let ctx = TerminalContext::collect().await;

    // Memory Store
    let data_dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?
        .join("mylm")
        .join("memory");
    std::fs::create_dir_all(&data_dir)?;
    let store = Arc::new(VectorStore::new(data_dir.to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid data directory path"))?).await?);

    // State Store
    let state_store = Arc::new(std::sync::RwLock::new(StateStore::new()?)); // StateStore uses std::sync

    // Executor
    let allowlist = CommandAllowlist::new();
    let safety_checker = SafetyChecker::new();
    let executor = Arc::new(CommandExecutor::new(allowlist, safety_checker));

    // Categorizer
    let categorizer = Arc::new(MemoryCategorizer::new(client.clone(), store.clone()));

    // Tools - convert Box to Arc for the agent
    let shell_config = crate::agent_old::tools::shell::ShellToolConfig {
        executor,
        context: ctx.clone(),
        terminal: terminal.clone(),
        memory_store: Some(store.clone()),
        categorizer: Some(categorizer.clone()),
        session_id: None,
        job_registry: None, // JobRegistry not available in Agent v1
        permissions: None, // permissions - V1 agents don't use permissions
    };
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(crate::agent_old::tools::shell::ShellTool::new_with_config(shell_config)),
        Arc::new(crate::agent_old::tools::web_search::WebSearchTool::new(
            config.features.web_search.clone()
        )),
        Arc::new(crate::agent_old::tools::memory::MemoryTool::new(store.clone())),
        Arc::new(crate::agent_old::tools::crawl::CrawlTool::new(Arc::clone(&event_bus))),
        Arc::new(crate::agent_old::tools::fs::FileReadTool),
        Arc::new(crate::agent_old::tools::fs::FileWriteTool),
        Arc::new(crate::agent_old::tools::list_files::ListFilesTool::with_cwd()),
        Arc::new(crate::agent_old::tools::git::GitStatusTool),
        Arc::new(crate::agent_old::tools::git::GitLogTool),
        Arc::new(crate::agent_old::tools::git::GitDiffTool),
        Arc::new(crate::agent_old::tools::state::StateTool::new(state_store.clone())),
        Arc::new(crate::agent_old::tools::system::SystemMonitorTool::new()),
        Arc::new(crate::agent_old::tools::terminal_sight::TerminalSightTool::new(terminal.clone())),
        Arc::new(crate::agent_old::tools::wait::WaitTool),
    ];


    let system_prompt = crate::config::build_system_prompt(
        &ctx,
        "default",
        Some("WebSocket Session"),
        Some(&config.features.prompts),
        Some(tools.as_slice()),
        None
    ).await?;

    // Determine agent version based on profile settings
    let agent_version = if config.profiles.get(&config.profile).and_then(|p| p.agent.as_ref()).is_some() {
        AgentVersion::V2
    } else {
        AgentVersion::V1
    };

    match agent_version {
        AgentVersion::V2 => {
            let agent = create_agent_v2_for_session(config, event_bus, terminal, None).await?;
            Ok(crate::agent_old::v2::driver::factory::BuiltAgent::V2(agent))
        }
        AgentVersion::V1 => {
            let config = crate::agent_old::AgentConfig {
                client,
                tools,
                system_prompt_prefix: system_prompt,
                max_iterations: resolved.agent.max_iterations,
                version: agent_version,
                memory_store: Some(store),
                categorizer: Some(categorizer),
                job_registry: None,
                scratchpad: None,
                disable_memory: false,
                permissions: resolved.agent.permissions.clone(),
                event_bus: Some(event_bus),
                max_actions_before_stall: resolved.agent.max_actions_before_stall,
                max_consecutive_messages: resolved.agent.max_consecutive_messages,
                max_recovery_attempts: resolved.agent.max_recovery_attempts,
                max_tool_failures: resolved.agent.max_tool_failures,
            };
            let agent = Agent::new_with_config(config).await;
            Ok(crate::agent_old::v2::driver::factory::BuiltAgent::V1(agent))
        }
    }
}

pub async fn create_agent_v2_for_session(
    config: &Config,
    event_bus: Arc<EventBus>,
    terminal: Arc<dyn TerminalExecutor>,
    escalation_tx: Option<tokio::sync::mpsc::Sender<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>>,
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
        resolved.agent.max_context_tokens,
    )
    .with_memory(config.features.memory.clone());
    
    // Create rate limiter from config
    let rate_limit_config = RateLimitConfig::from_settings(
        Some(resolved.agent.main_rpm),
        Some(resolved.agent.workers_rpm),
    );
    let rate_limiter = Arc::new(RateLimiter::new(rate_limit_config));
    
    let client = Arc::new(LlmClient::new(llm_config)?.with_rate_limiter(rate_limiter.clone()));

    // Context
    let ctx = TerminalContext::collect().await;

    // Memory Store
    let data_dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?
        .join("mylm")
        .join("memory");
    std::fs::create_dir_all(&data_dir)?;
    let store = Arc::new(VectorStore::new(data_dir.to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid data directory path"))?).await?);

    // State Store
    let state_store = Arc::new(std::sync::RwLock::new(StateStore::new()?)); // StateStore uses std::sync

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
    let job_registry = crate::agent_old::v2::jobs::JobRegistry::new();
    job_registry.set_event_bus(event_bus.clone());
    
    // Shared scratchpad for main agent and workers
    let shared_scratchpad = Arc::new(RwLock::new(StructuredScratchpad::new())); // Use tokio::sync::RwLock

    // Tools for AgentV2 - includes the new Delegate tool
    let shell_config = crate::agent_old::tools::shell::ShellToolConfig {
        executor: executor.clone(),
        context: ctx.clone(),
        terminal: terminal.clone(),
        memory_store: Some(store.clone()),
        categorizer: Some(categorizer.clone()),
        session_id: None,
        job_registry: Some(job_registry.clone()), // Share registry with ShellTool if needed
        permissions: resolved.agent.permissions.clone(), // Pass permissions from config
    };
    let mut tools_vec: Vec<Arc<dyn Tool>> = vec![
        Arc::new(crate::agent_old::tools::shell::ShellTool::new_with_config(shell_config)),
        Arc::new(crate::agent_old::tools::web_search::WebSearchTool::new(
            config.features.web_search.clone()
        )),
        Arc::new(crate::agent_old::tools::memory::MemoryTool::new(store.clone())),
        Arc::new(crate::agent_old::tools::crawl::CrawlTool::new(Arc::clone(&event_bus))),
        Arc::new(crate::agent_old::tools::fs::FileReadTool),
        Arc::new(crate::agent_old::tools::fs::FileWriteTool),
        Arc::new(crate::agent_old::tools::list_files::ListFilesTool::with_cwd()),
        Arc::new(crate::agent_old::tools::git::GitStatusTool),
        Arc::new(crate::agent_old::tools::git::GitLogTool),
        Arc::new(crate::agent_old::tools::git::GitDiffTool),
        Arc::new(crate::agent_old::tools::state::StateTool::new(state_store.clone())),
        Arc::new(crate::agent_old::tools::system::SystemMonitorTool::new()),
        Arc::new(crate::agent_old::tools::terminal_sight::TerminalSightTool::new(terminal.clone())),
        Arc::new(crate::agent_old::tools::wait::WaitTool),
    ];
    
    // Build tools HashMap for DelegateTool (convert Vec to HashMap)
    let mut tools_map = std::collections::HashMap::new();
    for tool in &tools_vec {
        tools_map.insert(tool.name().to_string(), tool.clone());
    }
    
    // Create shared workspace for worker awareness
    let _shared_workspace = crate::agent_old::workspace::SharedWorkspace::new(job_registry.clone())
        .with_vector_store(store.clone());
    
    // Add DelegateTool with access to parent tools and permissions
    let delegate_config = crate::agent_old::tools::delegate::DelegateToolConfig {
        llm_client: client.clone(),
        scribe: scribe.clone(),
        job_registry: job_registry.clone(),
        memory_store: Some(store.clone()),
        categorizer: Some(categorizer.clone()),
        event_bus: Some(event_bus.clone()),
        tools: tools_map,
        permissions: resolved.agent.permissions.clone(),
        max_iterations: resolved.agent.max_iterations,
        executor: executor.clone(), // For WorkerShellTool
        max_tool_failures: resolved.agent.max_tool_failures,
        worker_model: Some(resolved.agent.worker_model.clone()), // Use configured worker_model
        providers: config.providers.clone(), // Pass providers for worker model lookup
    };
    let delegate_tool = crate::agent_old::tools::delegate::DelegateTool::new(delegate_config)
        .with_rate_limiter(rate_limiter);
    // Add escalation channel if provided
    let delegate_tool = if let Some(tx) = escalation_tx {
        delegate_tool.with_escalation_channel(tx)
    } else {
        delegate_tool
    };
    tools_vec.push(Arc::new(delegate_tool));
    
    let tools = tools_vec;


    let system_prompt = crate::config::build_system_prompt(
        &ctx,
        "default",
        Some("WebSocket Session"),
        Some(&config.features.prompts),
        Some(tools.as_slice()),
        None
    ).await?;

    let config = crate::agent_old::v2::AgentV2Config {
        client,
        scribe,
        tools,
        system_prompt_prefix: system_prompt,
        max_iterations: resolved.agent.max_iterations,
        version: AgentVersion::V2,
        memory_store: Some(store),
        categorizer: Some(categorizer),
        job_registry: Some(job_registry),
        capabilities_context: None, // capabilities_context is already included in system_prompt
        permissions: resolved.agent.permissions.clone(), // Pass permissions from config
        scratchpad: Some(shared_scratchpad), // Use shared scratchpad that workers can also write to
        disable_memory: false,
        event_bus: Some(event_bus),
        execute_tools_internally: true,
        max_actions_before_stall: resolved.agent.max_actions_before_stall,
        max_consecutive_messages: resolved.agent.max_consecutive_messages,
        max_recovery_attempts: resolved.agent.max_recovery_attempts,
        max_tool_failures: resolved.agent.max_tool_failures,
    };
    Ok(AgentV2::new_with_config(config))
}
