//! `mylm` - A globally available, high-performance terminal AI assistant
//!
//! This binary provides a CLI interface for interacting with LLM endpoints
//! while collecting terminal context and safely executing sysadmin tasks.

use anyhow::{Context, Result};
use clap::Parser;
use console::Style;
use std::sync::Arc;

use crate::cli::{Cli, Commands, MemoryCommand, ConfigCommand, EditCommand, hub::HubChoice, SessionCommand};
use mylm_core::config::Config;
use mylm_core::context::TerminalContext;
use mylm_core::llm::{LlmClient, LlmConfig};
use mylm_core::output::OutputFormatter;
use crate::terminal::app::App;
use std::process::Command;

mod cli;
mod terminal;

/// Main entry point for the AI assistant CLI
#[tokio::main]
async fn main() -> Result<()> {
    // Capture context IMMEDIATELY before any output to ensure we get the clean terminal state
    let initial_context = mylm_core::context::TerminalContext::collect_sync();
    
    // Parse command-line arguments
    let cli = Cli::parse();

    // Task 1: Splash Screen Animation
    // Show splash screen only if we're entering Hub (no command) or TUI
    // But NOT if we are just checking version
    if cli.command.is_none() && cli.query.is_empty() && !cli.version {
        show_splash_screen().await?;
    }

    // Handle version separately if no other args provided or explicitly requested
    if cli.version {
        let blue = Style::new().blue();
        println!(
            "{} v{}-{} ({})",
            blue.apply_to("mylm"),
            env!("CARGO_PKG_VERSION"),
            env!("BUILD_NUMBER"),
            env!("GIT_HASH")
        );
        println!("Built with Rust + Love for terminal productivity");
        return Ok(());
    }

    // Setup output formatting
    let formatter = OutputFormatter::new();

    // Load configuration
    let mut config = Config::load().context("Failed to load configuration")?;

    // Handle different commands
    match &cli.command {
        Some(Commands::Query {
            query,
            execute: _,
            force: _,
        }) => {
            handle_one_shot(&cli, query, &config, &formatter).await?;
        }

        None if !cli.query.is_empty() => {
            let query = cli.query.join(" ");
            handle_one_shot(&cli, &query, &config, &formatter).await?;
        }

        Some(Commands::Context { format: _ }) => {
            let ctx = TerminalContext::collect().await;
            formatter.print_context(&ctx);
        }

        Some(Commands::Execute {
            command,
            dry_run: _,
        }) => {
            let ctx = TerminalContext::collect().await;
            let endpoint_config = config.get_endpoint(cli.endpoint.as_deref())?;

            let llm_config = LlmConfig::new(
                endpoint_config.provider.parse().map_err(|e| anyhow::anyhow!("{}", e))?,
                endpoint_config.base_url.clone(),
                endpoint_config.model.clone(),
                Some(endpoint_config.api_key.clone()),
            )
            .with_memory(config.memory.clone());
            let client = LlmClient::new(llm_config)?;

            let prompt = format!(
                r#"You are a terminal AI assistant. A user wants to execute this command:

```
{}
```

Current system context:
- Working directory: {}
- Git branch: {}
- Recent files changed: {}

First, analyze this command:
1. Is it safe to execute? What does it do?
2. What could go wrong?
3. Suggest any improvements or safer alternatives?

Then, if it appears safe, provide the exact command to execute.
Respond in this format:
SAFETY: [SAFE|DANGEROUS]
ANALYSIS: [Your analysis]
COMMAND: [The command to execute, exactly as it should be run]"#,
                command,
                ctx.cwd().unwrap_or_else(|| "unknown".to_string()),
                ctx.git_branch().unwrap_or_else(|| "not a git repo".to_string()),
                ctx.git_status().unwrap_or_else(|| "unknown".to_string())
            );

            let response = client.complete(&prompt).await?;
            formatter.print_command_analysis(&response);
        }

        Some(Commands::Endpoints) => {
            formatter.print_endpoints(&config.endpoints);
        }

        Some(Commands::Setup { warmup }) => {
            if *warmup {
                mylm_core::memory::VectorStore::warmup().await?;
            } else {
                if config.endpoints.is_empty() {
                    println!("🤖 Welcome to mylm! Let's get you set up.");
                }
                handle_settings_dashboard(&mut config).await?;
                mylm_core::memory::VectorStore::warmup().await?;
            }
        }

        Some(Commands::System { brief }) => {
            let ctx = TerminalContext::collect().await;
            formatter.print_system_info(&ctx, *brief);
        }

        Some(Commands::Interactive) => {
            let update_available = check_for_updates_fast();
            terminal::run_tui(None, None, None, None, update_available).await?;
        }

        Some(Commands::Pop) => {
            if crate::cli::hub::is_tmux_available() {
                let context = TerminalContext::collect().await;
                let update_available = check_for_updates_fast();
                terminal::run_tui(None, None, Some(context), Some(initial_context.terminal), update_available).await?;
            } else {
                println!("\n❌ {} is required for the 'Pop Terminal' feature.", Style::new().bold().apply_to("tmux"));
                println!("   This feature uses tmux to capture your current terminal session history and provide seamless context.");
                println!("\n   Note: tmux does not run automatically; you should start your terminal inside a tmux session");
                println!("   (by running 'tmux') to take full advantage of this feature.");
                println!("\n   Please install it using your package manager:");
                println!("   - {}  : sudo apt install tmux", Style::new().cyan().apply_to("Debian/Ubuntu/Pop"));
                println!("   - {}       : sudo dnf install tmux", Style::new().cyan().apply_to("Fedora"));
                println!("   - {}         : sudo pacman -S tmux", Style::new().cyan().apply_to("Arch"));
                println!("   - {}        : brew install tmux", Style::new().cyan().apply_to("macOS"));
                println!();
            }
        }

        Some(Commands::Memory { cmd }) => {
            let data_dir = dirs::data_dir()
                .context("Could not find data directory")?
                .join("mylm")
                .join("memory");
            
            std::fs::create_dir_all(&data_dir)?;
            let store = mylm_core::memory::store::VectorStore::new(data_dir.to_str().unwrap()).await?;

            match cmd {
                MemoryCommand::Add { content } => {
                    println!("Adding to memory...");
                    store.add_memory(content).await?;
                    println!("Added successfully.");
                }
                MemoryCommand::Search { query, limit } => {
                    println!("Searching memory...");
                    let results = store.search_memory(query, *limit).await?;
                    if results.is_empty() {
                        println!("No memories found.");
                    } else {
                        for (i, res) in results.iter().enumerate() {
                            println!("{}. {}", i + 1, res);
                        }
                    }
                }
            }
        }

        Some(Commands::Config { cmd }) => {
            match cmd {
                Some(ConfigCommand::Select) => {
                    let profiles: Vec<String> = config.profiles.iter().map(|p| p.name.clone()).collect();
                    let ans = inquire::Select::new("Select Active Profile", profiles).prompt()?;
                    config.active_profile = ans;
                    config.save_to_default_location()?;
                    println!("Active profile set to {}", config.active_profile);
                }
                Some(ConfigCommand::New) => {
                    let name = inquire::Text::new("New profile name:").prompt()?;
                    if !name.trim().is_empty() {
                        config.profiles.push(mylm_core::config::Profile {
                            name: name.clone(),
                            endpoint: config.default_endpoint.clone(),
                            prompt: "default".to_string(),
                            model: None,
                        });
                        config.active_profile = name;
                        handle_settings_dashboard(&mut config).await?;
                    }
                }
                Some(ConfigCommand::Edit { cmd: edit_cmd }) => {
                    match edit_cmd {
                        Some(EditCommand::Prompt) => {
                            let profile = config.get_active_profile()
                                .map(|p| p.prompt.clone())
                                .unwrap_or_else(|| "default".to_string());
                            let path = mylm_core::config::prompt::get_prompts_dir().join(format!("{}.md", profile));
                            let _ = mylm_core::config::prompt::load_prompt(&profile)?;
                            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
                            std::process::Command::new(editor).arg(path).status()?;
                        }
                        None => {
                            handle_settings_dashboard(&mut config).await?;
                        }
                    }
                }
                None => {
                    handle_settings_dashboard(&mut config).await?;
                }
            }
        }

        Some(Commands::Session { cmd }) => {
            handle_session_command(cmd, &config).await?;
        }

        None => {
            handle_hub(&mut config, &formatter, initial_context).await?;
        }
    }

    Ok(())
}

/// Handle the interactive hub menu
async fn handle_hub(config: &mut Config, formatter: &OutputFormatter, initial_context: mylm_core::context::TerminalContext) -> Result<()> {
    loop {
        let choice = crate::cli::hub::show_hub(config).await?;
        match choice {
            HubChoice::PopTerminal => {
                let context = mylm_core::context::TerminalContext::collect().await;
                let update_available = check_for_updates_fast();
                terminal::run_tui(None, None, Some(context), Some(initial_context.terminal), update_available).await?;
                break;
            }
            HubChoice::PopTerminalMissing => {
                println!("\n❌ {} is required for the 'Pop Terminal' feature.", Style::new().bold().apply_to("tmux"));
                println!("   This feature uses tmux to capture your current terminal session history and provide seamless context.");
                println!("\n   Note: tmux does not run automatically; you should start your terminal inside a tmux session");
                println!("   (by running 'tmux') to take full advantage of this feature.");
                println!("\n   Please install it using your package manager:");
                println!("   - {}  : sudo apt install tmux", Style::new().cyan().apply_to("Debian/Ubuntu/Pop"));
                println!("   - {}       : sudo dnf install tmux", Style::new().cyan().apply_to("Fedora"));
                println!("   - {}         : sudo pacman -S tmux", Style::new().cyan().apply_to("Arch"));
                println!("   - {}        : brew install tmux", Style::new().cyan().apply_to("macOS"));
                println!();
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            }
            HubChoice::ResumeSession => {
                match App::load_session(None) {
                    Ok(session) => {
                        let update_available = check_for_updates_fast();
                        terminal::run_tui(Some(session), None, None, None, update_available).await?;
                        break;
                    }
                    Err(e) => {
                        println!("❌ Failed to load session: {}", e);
                    }
                }
            }
            HubChoice::StartTui => {
                let update_available = check_for_updates_fast();
                terminal::run_tui(None, None, None, None, update_available).await?;
                break;
            }
            HubChoice::QuickQuery => {
                let query = inquire::Text::new("⚡ Quick Query:").prompt()?;
                if !query.trim().is_empty() {
                    handle_one_shot(&Cli::parse(), &query, config, formatter).await?;
                }
            }
            HubChoice::Configuration => {
                handle_settings_dashboard(config).await?;
            }
            HubChoice::ManageSessions => {
                let sessions = list_sessions()?;
                if sessions.is_empty() {
                    println!("No saved sessions found.");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
                
                let session_options: Vec<String> = sessions.iter()
                    .map(|s| format!("{} | {} ({} msgs)", s.id, s.timestamp.format("%Y-%m-%d %H:%M"), s.metadata.message_count))
                    .collect();

                if let Some(choice) = crate::cli::hub::show_session_select(session_options)? {
                    let idx = choice.find(" | ").unwrap_or(choice.len());
                    let id = &choice[..idx];
                    match App::load_session(Some(id)) {
                        Ok(session) => {
                            let update_available = check_for_updates_fast();
                            terminal::run_tui(Some(session), None, None, None, update_available).await?;
                            break;
                        }
                        Err(e) => println!("❌ Failed to load session: {}", e),
                    }
                }
            }
            HubChoice::Exit => break,
        }
    }
    Ok(())
}

fn list_sessions() -> Result<Vec<crate::terminal::session::Session>> {
    let data_dir = dirs::data_dir()
        .context("Could not find data directory")?
        .join("mylm")
        .join("sessions");
    
    if !data_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(data_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if filename.starts_with("session_") {
                let content = std::fs::read_to_string(&path)?;
                if let Ok(session) = serde_json::from_str::<crate::terminal::session::Session>(&content) {
                    sessions.push(session);
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(sessions)
}

async fn handle_session_command(cmd: &SessionCommand, _config: &Config) -> Result<()> {
    match cmd {
        SessionCommand::List => {
            let sessions = list_sessions()?;
            if sessions.is_empty() {
                println!("No saved sessions found.");
            } else {
                let blue = Style::new().blue().bold();
                let dim = Style::new().dim();
                println!("{:<20} | {:<20} | {:<30}", blue.apply_to("ID"), "Date", "Last Message");
                println!("{}", "-".repeat(75));
                for s in sessions {
                    println!("{:<20} | {:<20} | {:<30}",
                        s.id,
                        s.timestamp.format("%Y-%m-%d %H:%M"),
                        dim.apply_to(s.metadata.last_message_preview)
                    );
                }
            }
        }
        SessionCommand::Resume { id } => {
            match App::load_session(Some(id)) {
                Ok(session) => {
                    let update_available = check_for_updates_fast();
                    terminal::run_tui(Some(session), None, None, None, update_available).await?;
                }
                Err(e) => println!("❌ Failed to load session: {}", e),
            }
        }
        SessionCommand::Delete { id } => {
            let data_dir = dirs::data_dir()
                .context("Could not find data directory")?
                .join("mylm")
                .join("sessions");
            let filename = if id.ends_with(".json") { id.to_string() } else { format!("session_{}.json", id) };
            let path = data_dir.join(filename);
            if path.exists() {
                std::fs::remove_file(path)?;
                println!("✅ Session deleted.");
            } else {
                println!("❌ Session not found.");
            }
        }
    }
    Ok(())
}

/// Handle the unified settings dashboard
async fn handle_settings_dashboard(config: &mut Config) -> Result<()> {
    loop {
        let choice = crate::cli::hub::show_settings_dashboard(config)?;
        match choice {
            crate::cli::hub::SettingsChoice::SwitchProfile => {
                loop {
                    let action = crate::cli::hub::show_profiles_submenu(config)?;
                    match action {
                        crate::cli::hub::ProfileAction::Select => {
                            let profiles: Vec<String> = config.profiles.iter().map(|p| p.name.clone()).collect();
                            if let Some(ans) = crate::cli::hub::show_profile_select(profiles)? {
                                config.active_profile = ans;
                            }
                        }
                        crate::cli::hub::ProfileAction::Create => {
                            if let Some((name, endpoint, model, prompt)) = crate::cli::hub::show_profile_wizard(config)? {
                                config.profiles.push(mylm_core::config::Profile {
                                    name: name.clone(),
                                    endpoint,
                                    prompt,
                                    model,
                                });
                                config.active_profile = name;
                            }
                        }
                        crate::cli::hub::ProfileAction::Duplicate => {
                            let profiles: Vec<String> = config.profiles.iter().map(|p| p.name.clone()).collect();
                            if let Some(source) = crate::cli::hub::show_profile_select(profiles)? {
                                if let Some((name, endpoint, model, prompt)) = crate::cli::hub::show_profile_duplicate_wizard(config, &source)? {
                                    config.profiles.push(mylm_core::config::Profile {
                                        name: name.clone(),
                                        endpoint,
                                        prompt,
                                        model,
                                    });
                                    config.active_profile = name;
                                }
                            }
                        }
                        crate::cli::hub::ProfileAction::Rename => {
                            let profiles: Vec<String> = config.profiles.iter().map(|p| p.name.clone()).collect();
                            if let Some(old_name) = crate::cli::hub::show_profile_select(profiles)? {
                                if let Some(new_name) = crate::cli::hub::show_profile_rename_wizard(config, &old_name)? {
                                    if let Some(p) = config.profiles.iter_mut().find(|p| p.name == old_name) {
                                        p.name = new_name.clone();
                                    }
                                    if config.active_profile == old_name {
                                        config.active_profile = new_name;
                                    }
                                }
                            }
                        }
                        crate::cli::hub::ProfileAction::Delete => {
                            let profiles: Vec<String> = config.profiles.iter().map(|p| p.name.clone()).collect();
                            if let Some(name) = crate::cli::hub::show_profile_select(profiles)? {
                                if config.profiles.len() > 1 {
                                    config.profiles.retain(|p| p.name != name);
                                    if config.active_profile == name {
                                        config.active_profile = config.profiles[0].name.clone();
                                    }
                                } else {
                                    println!("⚠️  Cannot delete the last profile.");
                                }
                            }
                        }
                        crate::cli::hub::ProfileAction::Back => break,
                    }
                }
            }
            crate::cli::hub::SettingsChoice::EditProvider => {
                let endpoint_name = config.get_active_profile().map(|p| p.endpoint.clone()).unwrap_or_default();
                config.edit_endpoint_provider(&endpoint_name).await?;
            }
            crate::cli::hub::SettingsChoice::EditApiUrl => {
                let endpoint_name = config.get_active_profile().map(|p| p.endpoint.clone()).unwrap_or_default();
                config.edit_endpoint_base_url(&endpoint_name)?;
            }
            crate::cli::hub::SettingsChoice::EditApiKey => {
                let endpoint_name = config.get_active_profile().map(|p| p.endpoint.clone()).unwrap_or_default();
                config.edit_endpoint_api_key(&endpoint_name)?;
            }
            crate::cli::hub::SettingsChoice::EditModel => {
                let profile_name = config.active_profile.clone();
                config.edit_profile_model(&profile_name).await?;
            }
            crate::cli::hub::SettingsChoice::EditPrompt => {
                let profile = config.get_active_profile()
                    .map(|p| p.prompt.clone())
                    .unwrap_or_else(|| "default".to_string());
                let path = mylm_core::config::prompt::get_prompts_dir().join(format!("{}.md", profile));
                let _ = mylm_core::config::prompt::load_prompt(&profile)?;
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
                std::process::Command::new(editor).arg(path).status()?;
            }
            crate::cli::hub::SettingsChoice::ManageEndpoints => {
                loop {
                    let action = crate::cli::hub::show_endpoints_submenu()?;
                    match action {
                        crate::cli::hub::EndpointAction::SwitchConnection => {
                            let endpoints: Vec<String> = config.endpoints.iter().map(|e| e.name.clone()).collect();
                            let current_endpoint = config.get_active_profile().map(|p| p.endpoint.clone()).unwrap_or_default();
                            if let Some(new_endpoint) = crate::cli::hub::show_endpoint_select(endpoints, &current_endpoint)? {
                                if let Some(p) = config.profiles.iter_mut().find(|p| p.name == config.active_profile) {
                                    p.endpoint = new_endpoint;
                                }
                            }
                        }
                        crate::cli::hub::EndpointAction::CreateNew => {
                            let name = inquire::Text::new("New connection name:").prompt()?;
                            if !name.trim().is_empty() {
                                config.edit_endpoint_details(&name).await?;
                            }
                        }
                        crate::cli::hub::EndpointAction::Delete => {
                            let endpoints: Vec<String> = config.endpoints.iter().map(|e| e.name.clone()).collect();
                             if endpoints.len() <= 1 {
                                println!("⚠️  Cannot delete the last connection.");
                            } else {
                                let del = inquire::Select::new("Delete Connection", endpoints).prompt()?;
                                let active_uses = config.get_active_profile().map(|p| p.endpoint == del).unwrap_or(false);
                                if active_uses {
                                     println!("⚠️  Cannot delete the connection used by the active profile. Switch first.");
                                } else {
                                     config.endpoints.retain(|e| e.name != del);
                                     println!("🗑️  Deleted connection '{}'.", del);
                                }
                            }
                        }
                        crate::cli::hub::EndpointAction::Back => break,
                    }
                }
            }
            crate::cli::hub::SettingsChoice::Advanced => {
                loop {
                    let adv_choice = crate::cli::hub::show_advanced_submenu()?;
                    match adv_choice {
                        crate::cli::hub::AdvancedSettingsChoice::WebSearch => config.edit_search().await?,
                        crate::cli::hub::AdvancedSettingsChoice::General => config.edit_general()?,
                        crate::cli::hub::AdvancedSettingsChoice::ShellIntegration => {
                             loop {
                                let choice = crate::cli::hub::show_shell_integration_menu()?;
                                match choice {
                                    crate::cli::hub::ShellIntegrationChoice::ToggleTmuxAutoStart => {
                                        toggle_tmux_autostart()?;
                                    }
                                    crate::cli::hub::ShellIntegrationChoice::Back => break,
                                }
                            }
                        }
                        crate::cli::hub::AdvancedSettingsChoice::Back => break,
                    }
                }
            }
            crate::cli::hub::SettingsChoice::Save => {
                config.save_to_default_location()?;
                println!("✅ Configuration saved successfully.");
                break;
            }
            crate::cli::hub::SettingsChoice::Back => break,
        }
    }
    Ok(())
}

/// Helper to handle AI queries (One-shot / Headless)
async fn handle_one_shot(
    cli: &Cli,
    query: &str,
    config: &Config,
    _formatter: &OutputFormatter,
) -> Result<()> {
    let blue = Style::new().blue();
    let green = Style::new().green();

    // Collect terminal context
    let ctx = TerminalContext::collect().await;

    // Determine which endpoint to use
    let endpoint_config = config.get_endpoint(cli.endpoint.as_deref())?;

    // Get effective model (profile override or endpoint default)
    let effective_model = if let Some(profile) = config.get_active_profile() {
        config.get_effective_model(profile)?
    } else {
        endpoint_config.model.clone()
    };

    // Create LLM client
    let llm_config = LlmConfig::new(
        endpoint_config.provider.parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        endpoint_config.base_url.clone(),
        effective_model,
        Some(endpoint_config.api_key.clone()),
    )
    .with_memory(config.memory.clone());
    let client = Arc::new(LlmClient::new(llm_config)?);

    // Build hierarchical system prompt
    let prompt_name = config.get_active_profile()
        .map(|p| p.prompt.as_str())
        .unwrap_or("default");
    let system_prompt = mylm_core::config::prompt::build_system_prompt(&ctx, prompt_name, Some("CLI (Single Query)")).await?;

    println!("{} Querying {}...",
        blue.apply_to("🤖"),
        green.apply_to(&endpoint_config.name)
    );

    // Initialize dependencies for tools
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<crate::terminal::app::TuiEvent>();
    
    // Determine auto-approve based on CLI flags
    let auto_approve = match &cli.command {
        Some(Commands::Query { execute, .. }) => *execute,
        _ => false, // Default to false for direct queries for safety
    };

    let allowlist = mylm_core::executor::allowlist::CommandAllowlist::new();
    let safety_checker = mylm_core::executor::safety::SafetyChecker::new();
    let executor = Arc::new(mylm_core::executor::CommandExecutor::new(
        allowlist,
        safety_checker,
    ));

    // Spawn a task to handle logs from the agent (Thoughts, Actions, etc.)
    let log_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                crate::terminal::app::TuiEvent::PtyWrite(data) => {
                    // Print the raw PTY data (contains ANSI codes for colors)
                    use std::io::Write;
                    let _ = std::io::stdout().write_all(&data);
                    let _ = std::io::stdout().flush();
                }
                crate::terminal::app::TuiEvent::StatusUpdate(status) => {
                    if !status.is_empty() {
                        println!("\x1b[2m[mylm]: {}\x1b[0m", status);
                    }
                }
                crate::terminal::app::TuiEvent::SuggestCommand(cmd) => {
                    println!("\n\x1b[33m[Suggestion]:\x1b[0m AI suggests running: \x1b[1m{}\x1b[0m", cmd);
                    println!("\x1b[2mRun with --execute to allow safe commands or --force to bypass safety checks.\x1b[0m");
                }
                _ => {}
            }
        }
    });

    // Initialize memory store
    let data_dir = dirs::data_dir()
        .context("Could not find data directory")?
        .join("mylm")
        .join("memory");
    std::fs::create_dir_all(&data_dir)?;
    let store = Arc::new(mylm_core::memory::VectorStore::new(data_dir.to_str().unwrap()).await?);

    // Initialize state store
    let state_store = Arc::new(std::sync::RwLock::new(mylm_core::state::StateStore::new()?));

    // Load tools
    let tools: Vec<Box<dyn mylm_core::agent::Tool>> = vec![
        Box::new(mylm_core::agent::tools::shell::ShellTool::new(executor, ctx.clone(), event_tx.clone(), Some(store.clone()), Some(Arc::new(mylm_core::memory::MemoryCategorizer::new(client.clone(), store.clone()))), None)) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::web_search::WebSearchTool::new(config.web_search.clone(), event_tx.clone())) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::memory::MemoryTool::new(store.clone())) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::crawl::CrawlTool::new(event_tx.clone())) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::fs::FileReadTool) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::fs::FileWriteTool) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::git::GitStatusTool) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::git::GitLogTool) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::git::GitDiffTool) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::state::StateTool::new(state_store.clone())) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::system::SystemMonitorTool::new()) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::terminal_sight::TerminalSightTool::new(event_tx.clone())) as Box<dyn mylm_core::agent::Tool>,
        Box::new(mylm_core::agent::tools::wait::WaitTool) as Box<dyn mylm_core::agent::Tool>,
    ];

    let categorizer = Arc::new(mylm_core::memory::categorizer::MemoryCategorizer::new(client.clone(), store.clone()));
    let mut agent = mylm_core::agent::Agent::new_with_iterations(
        client,
        tools,
        system_prompt,
        config.agent.max_iterations,
        config.agent.version,
        Some(store),
        Some(categorizer)
    );
    
    let messages = vec![
        mylm_core::llm::chat::ChatMessage::user(query.to_string()),
    ];

    // Dummy interrupt flag for one-shot
    let interrupt_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    match agent.run(messages, event_tx, interrupt_flag, auto_approve, config.agent.max_driver_loops, None).await {
        Ok((response, _usage)) => {
            // Stop the log task
            log_handle.abort();
            
            // For headless, we just print the final answer part or the whole thing if no final answer tag
            if let Some(pos) = response.find("Final Answer:") {
                let answer = &response[pos + "Final Answer:".len()..].trim();
                println!("\n{}", answer);
            } else {
                println!("\n{}", response);
            }
        }
        Err(e) => {
            log_handle.abort();
            anyhow::bail!("Agent error: {}", e);
        }
    }

    Ok(())
}

/// Fast check for updates by comparing local HEAD with origin/main
/// This is intended to be called during splash screen or before TUI start
fn check_for_updates_fast() -> bool {
    // Check if we are in a git repo
    if !std::path::Path::new(".git").exists() {
        return false;
    }

    // Try to fetch in background with a timeout
    // In a real scenario, we might want to do this more robustly
    // For now, we assume origin/main exists
    let output = Command::new("git")
        .args(["rev-parse", "HEAD", "origin/main"])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let hashes = String::from_utf8_lossy(&out.stdout);
            let lines: Vec<&str> = hashes.lines().collect();
            if lines.len() >= 2 {
                return lines[0] != lines[1];
            }
        }
    }
    false
}

/// Show a brief loading animation
async fn show_splash_screen() -> Result<()> {
    use std::io::{Write, stdout};

    // Start git fetch in background while we show the animation
    let fetch_handle = std::thread::spawn(|| {
        let _ = Command::new("git").arg("fetch").arg("--quiet").output();
    });
    use tokio::time::{sleep, Duration};

    let blue = Style::new().blue().bold();
    let frames = ["|", "/", "-", "\\"];
    let start = std::time::Instant::now();
    let duration = Duration::from_millis(400);

    let mut i = 0;
    while start.elapsed() < duration {
        let frame = frames[i % frames.len()];
        let progress = (start.elapsed().as_millis() as f64 / duration.as_millis() as f64 * 20.0) as usize;
        let bar = "=".repeat(progress);
        let spaces = " ".repeat(20 - progress);

        print!(
            "\r{} {} [{}={}{}]",
            blue.apply_to("==== LOADING MYLM HUB"),
            frame,
            bar,
            if progress < 20 { ">" } else { "=" },
            spaces
        );
        stdout().flush()?;
        sleep(Duration::from_millis(40)).await;
        i += 1;
    }
    println!("\r{} [====================] ====", blue.apply_to("==== LOADING MYLM HUB"));
    
    // Wait for fetch to complete (it should be fast, but we don't want to block too long if it hangs)
    // Actually, thread::spawn doesn't have a timeout easily, but git fetch --quiet usually finishes in < 1s
    let _ = fetch_handle.join();
    
    Ok(())
}

fn toggle_tmux_autostart() -> Result<()> {
    let home = dirs::home_dir().context("Could not find home directory")?;
    let shells = vec![".bashrc", ".zshrc"];
    let snippet_start = "# --- mylm tmux auto-start ---";
    let snippet_end = "# --- end mylm tmux auto-start ---";
    
    let mut modified = false;
    let mut enabled = false;

    for shell in shells {
        let path = home.join(shell);
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            if content.contains(snippet_start) {
                // Remove snippet
                let lines: Vec<&str> = content.lines().collect();
                let mut new_lines = Vec::new();
                let mut in_snippet = false;
                for line in lines {
                    if line.contains(snippet_start) {
                        in_snippet = true;
                        continue;
                    }
                    if line.contains(snippet_end) {
                        in_snippet = false;
                        continue;
                    }
                    if !in_snippet {
                        new_lines.push(line);
                    }
                }
                std::fs::write(&path, new_lines.join("\n"))?;
                modified = true;
                enabled = false;
            } else {
                // Add snippet
                let mut new_content = content.clone();
                if !new_content.ends_with('\n') {
                    new_content.push('\n');
                }
                new_content.push('\n');
                new_content.push_str(snippet_start);
                new_content.push('\n');
                new_content.push_str("if command -v tmux &> /dev/null && [ -z \"$TMUX\" ] && [ -n \"$PS1\" ]; then\n");
                new_content.push_str("    tmux new-session -s \"mylm-$(date +%s)-$$-$RANDOM\"\n");
                new_content.push_str("fi\n");
                new_content.push_str(snippet_end);
                new_content.push('\n');
                
                std::fs::write(&path, new_content)?;
                modified = true;
                enabled = true;
            }
        }
    }

    if modified {
        if enabled {
            println!("✅ tmux auto-start enabled in your shell configuration.");
            println!("💡 Please restart your terminal for changes to take effect.");
        } else {
            println!("✅ tmux auto-start disabled (removed from shell configuration).");
        }
    } else {
        println!("⚠️  Could not find .bashrc or .zshrc to modify.");
    }

    Ok(())
}
