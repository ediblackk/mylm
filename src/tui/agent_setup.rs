//! Agent initialization module
//!
//! Handles setup of LLM client, agent, orchestrator, and related dependencies.

use crate::tui::app::state::TuiEvent;
use crate::tui::delegate_impl::TerminalDelegate;
use anyhow::{Context, Result};
use mylm_core::agent::event_bus::EventBus;
use mylm_core::agent::tools::{
    DelegateTool, StructuredScratchpad,
};
use mylm_core::agent::tools::worker_shell::{EscalationRequest, EscalationResponse};
use mylm_core::agent::{AgentOrchestrator, AgentWrapper, OrchestratorConfig, Tool};
use mylm_core::agent::v2::{AgentV2, AgentV2Config};
use mylm_core::agent::v2::jobs::JobRegistry;
use mylm_core::config::{build_system_prompt, AgentVersion, Config, ConfigUiExt};
use mylm_core::config::v2::types::AgentPermissions;
use mylm_core::context::TerminalContext;
use mylm_core::executor::allowlist::CommandAllowlist;
use mylm_core::executor::safety::SafetyChecker;
use mylm_core::executor::CommandExecutor;
use mylm_core::llm::{LlmClient, LlmConfig};
use mylm_core::memory::journal::Journal;
use mylm_core::memory::VectorStore;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};

/// Holds all initialized agent components
pub struct AgentComponents {
    pub agent_wrapper: AgentWrapper,
    pub orchestrator: AgentOrchestrator,
    #[allow(dead_code)]
    pub llm_client: Arc<LlmClient>,
    pub executor: Arc<CommandExecutor>,
    pub store: Arc<VectorStore>,
    pub state_store: Arc<std::sync::RwLock<mylm_core::state::StateStore>>,
    pub event_bus: Arc<EventBus>,
    pub scratchpad: Arc<RwLock<StructuredScratchpad>>,
    pub terminal_delegate: Arc<TerminalDelegate>,
    #[allow(dead_code)]
    pub escalation_tx: mpsc::Sender<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>,
    #[allow(dead_code)]
    pub system_prompt: String,
}

/// Initialize all agent components
pub async fn initialize_agent(
    config: &Config,
    context: TerminalContext,
    base_data_dir: &std::path::Path,
    incognito: bool,
    incognito_dir: Option<&std::path::Path>,
    pty_manager: Arc<crate::tui::pty::PtyManager>,
    event_tx: mpsc::UnboundedSender<TuiEvent>,
) -> Result<AgentComponents> {
    // Setup LLM Client
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
    
    let llm_client = Arc::new(LlmClient::new(llm_config)?);

    // Setup executor
    let allowlist = CommandAllowlist::new();
    let executor = Arc::new(CommandExecutor::new(allowlist, SafetyChecker::new()));

    // Setup memory store
    let (store, categorizer, journal) = if incognito {
        let memory_dir = incognito_dir.unwrap().join("memory");
        let journal_path = incognito_dir.unwrap().join("journal.md");
        std::fs::create_dir_all(&memory_dir)?;
        let store = Arc::new(VectorStore::new(memory_dir.to_str().unwrap()).await?);
        let journal = Journal::with_path(journal_path)?;
        (store, None, journal)
    } else {
        let data_dir = base_data_dir.join("memory");
        std::fs::create_dir_all(&data_dir)?;
        let store = Arc::new(VectorStore::new(data_dir.to_str().unwrap()).await?);
        let categorizer = Arc::new(mylm_core::memory::categorizer::MemoryCategorizer::new(
            llm_client.clone(),
            store.clone(),
        ));
        let journal = Journal::new()?;
        (store, Some(categorizer), journal)
    };

    // Initialize state store
    let state_store = Arc::new(std::sync::RwLock::new(mylm_core::state::StateStore::new()?));

    // Initialize scratchpad
    let scratchpad = Arc::new(RwLock::new(StructuredScratchpad::new()));

    // Build system prompt
    let system_prompt = build_system_prompt(&context, "system", Some("TUI (Interactive Mode)"), None, None, None).await?;
    let v2_system_prompt_prefix = system_prompt.clone();

    // Create EventBus
    let event_bus = Arc::new(EventBus::new());

    // Setup status callback
    {
        let event_tx = event_tx.clone();
        llm_client.set_status_callback(Arc::new(move |msg: &str| {
            let _ = event_tx.send(TuiEvent::StatusUpdate(msg.to_string()));
        }));
    }

    // Get job registry and set event bus for worker notifications
    let job_registry = crate::get_job_registry().clone();
    job_registry.set_event_bus(event_bus.clone());

    // Cleanup stale jobs
    let stale_jobs = job_registry.list_active_jobs();
    if !stale_jobs.is_empty() {
        mylm_core::warn_log!("Cleaning up {} stale jobs from previous session", stale_jobs.len());
        for job in stale_jobs {
            job_registry.cancel_job(&job.id);
            mylm_core::info_log!("Cancelled stale job: {} - {}", &job.id[..8.min(job.id.len())], job.description);
        }
    }

    // Create scribe
    let journal = Arc::new(Mutex::new(journal));
    let scribe = Arc::new(mylm_core::memory::scribe::Scribe::new(journal, store.clone(), llm_client.clone()));

    // Create terminal delegate
    let terminal_delegate = Arc::new(TerminalDelegate::new(pty_manager.clone(), event_tx.clone()));

    // Get configuration values
    let max_iterations = config.get_active_profile_info()
        .and_then(|p| p.max_iterations)
        .unwrap_or(10);
    let agent_version = config.features.agent_version;
    let resolved = config.resolve_profile();

    // Create escalation channel
    let (escalation_tx, escalation_rx) = mpsc::channel::<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>(100);

    // Build tools using factory
    let builder = mylm_core::agent::v2::driver::factory::AgentConfigs::tui(
        llm_client.clone(),
        executor.clone(),
        context.clone(),
        terminal_delegate.clone(),
        store.clone(),
        categorizer.clone(),
        job_registry.clone(),
        resolved.agent.permissions.clone(),
        config.features.web_search.clone(),
        event_bus.clone(),
        state_store.clone(),
        scratchpad.clone(),
        None,
    )
    .with_system_prompt(system_prompt.clone())
    .with_max_iterations(max_iterations);

    let tools = builder.build_tools().await?;

    // Build tools HashMap for DelegateTool
    let mut tools_map = HashMap::new();
    for tool in &tools {
        tools_map.insert(tool.name().to_string(), tool.clone());
    }

    // Create DelegateTool
    let delegate_config = mylm_core::agent::tools::delegate::DelegateToolConfig {
        llm_client: llm_client.clone(),
        scribe: scribe.clone(),
        job_registry: job_registry.clone(),
        memory_store: Some(store.clone()),
        categorizer: categorizer.clone(),
        event_bus: Some(Arc::clone(&event_bus)),
        tools: tools_map,
        permissions: None,
        max_iterations: 50,
        executor: executor.clone(),
        max_tool_failures: resolved.agent.max_tool_failures,
        worker_model: Some(resolved.agent.worker_model.clone()),
        providers: config.providers.clone(),
    };
    let delegate_tool = DelegateTool::new(delegate_config).with_escalation_channel(escalation_tx.clone());

    // Create orchestrator config
    let orchestrator_config = OrchestratorConfig {
        max_driver_loops: 50,
        max_retries: 3,
        max_smart_wait_iterations: 5,
        smart_wait_interval_secs: 1,
        auto_approve: false,
        enable_memory: !incognito,
        max_worker_tool_failures: resolved.agent.max_tool_failures,
    };

    // Create agent and orchestrator
    let (agent_wrapper, orchestrator) = create_agent_and_orchestrator(
        agent_version,
        llm_client.clone(),
        scribe.clone(),
        tools,
        v2_system_prompt_prefix,
        max_iterations,
        store.clone(),
        categorizer.clone(),
        job_registry.clone(),
        resolved.agent.permissions.clone(),
        scratchpad.clone(),
        incognito,
        event_bus.clone(),
        delegate_tool,
        escalation_tx.clone(),
        escalation_rx,
        orchestrator_config,
        terminal_delegate.clone(),
        executor.clone(),
        context.clone(),
        state_store.clone(),
    ).await?;

    Ok(AgentComponents {
        agent_wrapper,
        orchestrator,
        llm_client,
        executor,
        store,
        state_store,
        event_bus,
        scratchpad,
        terminal_delegate,
        escalation_tx,
        system_prompt,
    })
}

/// Create agent and orchestrator based on version
#[allow(clippy::too_many_arguments)]
async fn create_agent_and_orchestrator(
    agent_version: AgentVersion,
    llm_client: Arc<LlmClient>,
    scribe: Arc<mylm_core::memory::scribe::Scribe>,
    #[allow(unused)]
    tools: Vec<Arc<dyn Tool>>,
    system_prompt_prefix: String,
    max_iterations: usize,
    store: Arc<VectorStore>,
    categorizer: Option<Arc<mylm_core::memory::categorizer::MemoryCategorizer>>,
    job_registry: JobRegistry,
    permissions: Option<AgentPermissions>,
    scratchpad: Arc<RwLock<StructuredScratchpad>>,
    incognito: bool,
    event_bus: Arc<EventBus>,
    delegate_tool: DelegateTool,
    escalation_tx: mpsc::Sender<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>,
    escalation_rx: mpsc::Receiver<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>,
    orchestrator_config: OrchestratorConfig,
    terminal_delegate: Arc<TerminalDelegate>,
    executor: Arc<CommandExecutor>,
    context: TerminalContext,
    state_store: Arc<std::sync::RwLock<mylm_core::state::StateStore>>,
) -> Result<(AgentWrapper, AgentOrchestrator)> {
    if agent_version == AgentVersion::V2 {
        create_v2_agent(
            llm_client,
            scribe,
            tools,
            system_prompt_prefix,
            max_iterations,
            store,
            categorizer,
            job_registry,
            permissions,
            scratchpad,
            incognito,
            event_bus,
            delegate_tool,
            escalation_tx,
            escalation_rx,
            orchestrator_config,
        ).await
    } else {
        create_v1_agent(
            llm_client,
            scribe,
            tools,
            system_prompt_prefix,
            max_iterations,
            store,
            categorizer,
            job_registry,
            permissions,
            scratchpad,
            incognito,
            agent_version,
            event_bus,
            delegate_tool,
            escalation_tx,
            escalation_rx,
            orchestrator_config,
            terminal_delegate,
            executor,
            context,
            state_store,
        ).await
    }
}

/// Create V2 agent and orchestrator
async fn create_v2_agent(
    llm_client: Arc<LlmClient>,
    scribe: Arc<mylm_core::memory::scribe::Scribe>,
    tools: Vec<Arc<dyn Tool>>,
    system_prompt_prefix: String,
    max_iterations: usize,
    store: Arc<VectorStore>,
    categorizer: Option<Arc<mylm_core::memory::categorizer::MemoryCategorizer>>,
    job_registry: JobRegistry,
    permissions: Option<AgentPermissions>,
    scratchpad: Arc<RwLock<StructuredScratchpad>>,
    incognito: bool,
    event_bus: Arc<EventBus>,
    delegate_tool: DelegateTool,
    escalation_tx: mpsc::Sender<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>,
    escalation_rx: mpsc::Receiver<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>,
    orchestrator_config: OrchestratorConfig,
) -> Result<(AgentWrapper, AgentOrchestrator)> {
    let agent_v2_config = AgentV2Config {
        client: llm_client,
        scribe: scribe.clone(),
        tools,
        system_prompt_prefix,
        max_iterations,
        version: AgentVersion::V2,
        memory_store: Some(store.clone()),
        categorizer: categorizer.clone(),
        job_registry: Some(job_registry),
        capabilities_context: None,
        permissions,
        scratchpad: Some(scratchpad.clone()),
        disable_memory: incognito,
        event_bus: Some(event_bus.clone()),
        execute_tools_internally: false,
        max_actions_before_stall: 10,
        max_consecutive_messages: 50,
        max_recovery_attempts: 3,
        max_tool_failures: 5,
    };

    let mut agent_v2 = AgentV2::new_with_config(agent_v2_config);
    agent_v2.tools.insert(delegate_tool.name().to_string(), Arc::new(delegate_tool));

    let agent_wrapper = AgentWrapper::new_v2(agent_v2);
    let agent_v2_arc = agent_wrapper.as_v2_arc()
        .context("V2 wrapper should contain V2 agent")?;

    let orchestrator = AgentOrchestrator::new_with_agent_v2_and_escalation(
        agent_v2_arc,
        event_bus.clone(),
        orchestrator_config,
        Some(escalation_tx),
        Some(escalation_rx),
    ).await;

    Ok((agent_wrapper, orchestrator))
}

/// Create V1 agent and orchestrator
async fn create_v1_agent(
    llm_client: Arc<LlmClient>,
    scribe: Arc<mylm_core::memory::scribe::Scribe>,
    #[allow(unused)] tools: Vec<Arc<dyn Tool>>,
    system_prompt_prefix: String,
    max_iterations: usize,
    store: Arc<VectorStore>,
    categorizer: Option<Arc<mylm_core::memory::categorizer::MemoryCategorizer>>,
    job_registry: JobRegistry,
    permissions: Option<AgentPermissions>,
    scratchpad: Arc<RwLock<StructuredScratchpad>>,
    incognito: bool,
    agent_version: AgentVersion,
    event_bus: Arc<EventBus>,
    delegate_tool: DelegateTool,
    escalation_tx: mpsc::Sender<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>,
    escalation_rx: mpsc::Receiver<(EscalationRequest, tokio::sync::oneshot::Sender<EscalationResponse>)>,
    orchestrator_config: OrchestratorConfig,
    terminal_delegate: Arc<TerminalDelegate>,
    executor: Arc<CommandExecutor>,
    context: TerminalContext,
    state_store: Arc<std::sync::RwLock<mylm_core::state::StateStore>>,
) -> Result<(AgentWrapper, AgentOrchestrator)> {
    let builder = mylm_core::agent::v2::driver::factory::AgentConfigs::tui(
        llm_client,
        executor,
        context,
        terminal_delegate,
        store,
        categorizer,
        job_registry,
        permissions,
        mylm_core::config::WebSearchConfig::default(),
        event_bus.clone(),
        state_store,
        scratchpad.clone(),
        None,
    )
    .with_system_prompt(system_prompt_prefix)
    .with_max_iterations(max_iterations)
    .with_tool(Box::new(delegate_tool));

    let built_agent = builder.build().await;

    let mut agent = match built_agent {
        mylm_core::BuiltAgent::V1(a) => a,
        mylm_core::BuiltAgent::V2(_) => anyhow::bail!("Unexpected Agent V2 in TUI mode"),
    };

    agent.scribe = Some(scribe.clone());
    agent.disable_memory = incognito;
    agent.scratchpad = Some(scratchpad.clone());
    agent.version = agent_version;

    let agent_wrapper = AgentWrapper::new_v1(agent);
    let agent_v1_arc = agent_wrapper.as_v1_arc()
        .context("V1 wrapper should contain V1 agent")?;

    let orchestrator = AgentOrchestrator::new_with_agent_v1_and_escalation(
        agent_v1_arc,
        event_bus.clone(),
        orchestrator_config,
        Some(escalation_tx),
        Some(escalation_rx),
    ).await;

    Ok((agent_wrapper, orchestrator))
}

/// Helper struct for agent reconfiguration
#[allow(dead_code)]
pub struct AgentReconfig {
    pub llm_client: Arc<LlmClient>,
    pub executor: Arc<CommandExecutor>,
    pub event_bus: Arc<EventBus>,
    pub tools: Vec<Arc<dyn Tool>>,
}

/// Reconfigure agent with new config
#[allow(dead_code)]
pub async fn reconfigure_agent(
    config: &Config,
    terminal_delegate: Arc<TerminalDelegate>,
    store: Arc<VectorStore>,
    state_store: Arc<std::sync::RwLock<mylm_core::state::StateStore>>,
    scratchpad: Arc<RwLock<StructuredScratchpad>>,
) -> Result<AgentReconfig> {
    let resolved = config.resolve_profile();
    let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
    let api_key = resolved.api_key.unwrap_or_default();

    let llm_config = LlmConfig::new(
        format!("{:?}", resolved.provider).to_lowercase().parse()
            .unwrap_or(mylm_core::llm::LlmProvider::OpenAiCompatible),
        base_url.clone(),
        resolved.model.clone(),
        Some(api_key.clone()),
        resolved.agent.max_context_tokens,
    )
    .with_memory(config.features.memory.clone());

    let llm_client = Arc::new(LlmClient::new(llm_config)?);

    let allowlist = CommandAllowlist::new();
    let executor = Arc::new(CommandExecutor::new(allowlist, SafetyChecker::new()));

    let context = TerminalContext::collect().await;
    let event_bus = Arc::new(EventBus::new());

    let builder = mylm_core::agent::v2::driver::factory::AgentConfigs::tui(
        llm_client.clone(),
        executor.clone(),
        context.clone(),
        terminal_delegate,
        store,
        None, // categorizer - will be obtained from agent
        crate::get_job_registry().clone(),
        None, // permissions - will use defaults
        config.features.web_search.clone(),
        event_bus.clone(),
        state_store,
        scratchpad,
        None,
    );

    let tools = builder.build_tools().await.unwrap_or_default();

    Ok(AgentReconfig {
        llm_client,
        executor,
        event_bus,
        tools,
    })
}
