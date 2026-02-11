//! `mylm` - A globally available, high-performance terminal AI assistant
//!
//! This binary provides a CLI interface for interacting with LLM endpoints
//! while collecting terminal context and safely executing sysadmin tasks.

use anyhow::{Context, Result};
use clap::Parser;
use console::Style;
use std::io::Write;
use std::sync::{Arc, OnceLock};

use crate::cli::{Cli, Commands, MemoryCommand, ConfigCommand, EditCommand, hub::HubChoice, SessionCommand, DaemonCommand, PromptsCommand};
use mylm_core::config::{Config, ConfigUiExt};
use mylm_core::context::TerminalContext;
use mylm_core::llm::{LlmClient, LlmConfig};
use mylm_core::output::OutputFormatter;
use crate::terminal::app::App;
use std::process::Command;
use mylm_core::agent::traits::TerminalExecutor;
use crate::server::HeadlessTerminalExecutor;

mod cli;
mod terminal;
mod server;

/// Global background job registry
pub static JOB_REGISTRY: OnceLock<mylm_core::agent::v2::jobs::JobRegistry> = OnceLock::new();

pub fn get_job_registry() -> &'static mylm_core::agent::v2::jobs::JobRegistry {
    JOB_REGISTRY.get_or_init(mylm_core::agent::v2::jobs::JobRegistry::new)
}

/// Main entry point for the AI assistant CLI
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging as early as possible
    let data_dir = dirs::data_dir()
        .context("Could not find data directory")?
        .join("mylm");
    std::fs::create_dir_all(&data_dir)?;
    mylm_core::agent::logger::init(data_dir.clone());

    // Setup panic hook to log panic info and cleanup terminal state
    // This prevents mouse tracking mode and other terminal settings from leaking to shell
    std::panic::set_hook(Box::new(|panic_info| {
        // Log panic details to the log file
        let backtrace = std::backtrace::Backtrace::force_capture();
        let _ = mylm_core::agent::logger::log(
            mylm_core::agent::logger::LogLevel::Error,
            "panic",
            &format!("PANIC: {}\nBacktrace: {:?}", panic_info, backtrace),
        );

        // Best-effort terminal cleanup - suppress all errors
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableBracketedPaste,
            crossterm::event::DisableMouseCapture,
            crossterm::cursor::Show
        );
        // Reset colors and clear any remaining mouse tracking
        let _ = std::io::stdout().write_all(b"\x1b[?1000l\x1b[?1002l\x1b[?1015l\x1b[?1006l\x1b[?25h\x1b[0m");
        let _ = std::io::stdout().flush();
    }));

    mylm_core::info_log!("mylm starting up...");
    mylm_core::debug_log!("Log level: {:?}", std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()));

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

    // Onboarding: Check for fresh install - skip if any command is specified
    if !config.is_initialized() && config.profiles.is_empty() && !cli.version && cli.command.is_none() {
        println!("\n👋 Welcome to MyLM! It looks like this is a fresh install.");
        println!("🚀 Let's get you set up.");
        
        // Launch onboarding wizard (simplified version: configure endpoint -> create profile)
        if dialoguer::Confirm::new()
            .with_prompt("Would you like to configure an LLM endpoint now?")
            .default(true)
            .interact()?
        {
             crate::cli::hub::handle_add_provider(&mut config).await?;
        } else {
             println!("⚠️  mylm requires at least one endpoint to function.");
        }
    }

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
            let resolved = config.resolve_profile();
            
            let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
            let api_key = resolved.api_key.unwrap_or_default();
            
            if api_key.is_empty() {
                println!("❌ No API key configured.");
                println!("   Run 'mylm' to open the configuration hub and set up an endpoint.");
                return Ok(());
            }

            let llm_config = LlmConfig::new(
                format!("{:?}", resolved.provider).to_lowercase().parse().map_err(|e| anyhow::anyhow!("{}", e))?,
                base_url,
                resolved.model.clone(),
                Some(api_key),
                resolved.agent.max_context_tokens,
            )
            .with_memory(config.features.memory.clone());
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
            // In V2, there's only one base endpoint
            let endpoint_info = config.get_endpoint_info();
            println!("\n📊 Endpoint Configuration");
            println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
            println!("  Provider: {}", endpoint_info.provider);
            println!("  Base URL: {}", endpoint_info.base_url);
            println!("  Model: {}", endpoint_info.model);
            println!("  API Key: {}", if endpoint_info.api_key_set { "✅ Set" } else { "❌ Not Set" });
            println!("  Timeout: {}s", endpoint_info.timeout_seconds);
            
            // Show active profile and effective config
            let effective = config.get_effective_endpoint_info();
            println!("\n  Active Profile: {}", config.profile);
            println!("  Effective Model: {}", effective.model);
            println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));
        }

        Some(Commands::Setup { warmup }) => {
            if *warmup {
                mylm_core::memory::VectorStore::warmup().await?;
            } else {
                handle_settings_dashboard(&mut config).await?;
                // Only warm up if we have a configured endpoint
                if config.is_initialized() {
                    mylm_core::memory::VectorStore::warmup().await?;
                }
            }
        }

        Some(Commands::System { brief }) => {
            let ctx = TerminalContext::collect().await;
            formatter.print_system_info(&ctx, *brief);
        }

        Some(Commands::Interactive) => {
            let update_available = check_for_updates_fast();
            let _ = terminal::run_tui(None, None, None, None, update_available, false).await?;
        }

        Some(Commands::Pop) => {
            if crate::cli::hub::is_tmux_available() {
                let context = TerminalContext::collect().await;
                let update_available = check_for_updates_fast();
                let _ = terminal::run_tui(None, None, Some(context), Some(initial_context.terminal), update_available, false).await?;
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
            let store = mylm_core::memory::store::VectorStore::new(&data_dir.to_string_lossy()).await?;

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
                MemoryCommand::Repair => {
                    println!("🔧 Repairing memory database...");
                    let report = store.repair_database().await?;
                    println!("{}", report);
                }
            }
        }

        Some(Commands::Config { cmd }) => {
            match cmd {
                Some(ConfigCommand::Select) => {
                    let profiles: Vec<String> = config.profile_names();
                    if profiles.is_empty() {
                        println!("No profiles available. Create one first.");
                    } else {
                        let ans = inquire::Select::new("Select Active Profile", profiles).prompt()?;
                        config.set_active_profile(&ans)?;
                        config.save_to_default_location()?;
                        println!("Active profile set to {}", config.profile);
                    }
                }
                Some(ConfigCommand::New) => {
                    let name = inquire::Text::new("New profile name:").prompt()?;
                    if !name.trim().is_empty() {
                        config.create_profile(&name)?;
                        config.set_active_profile(&name)?;
                        handle_settings_dashboard(&mut config).await?;
                    }
                }
                Some(ConfigCommand::Edit { cmd: Some(EditCommand::Prompt) }) => {
                    let profile = config.get_active_profile_info()
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| "default".to_string());
                    let path = mylm_core::config::get_prompts_dir().join(format!("{}.md", profile));
                    let _ = mylm_core::config::load_prompt(&profile).await?;
                    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
                    std::process::Command::new(editor).arg(path).status()?;
                }
                Some(ConfigCommand::Edit { cmd: None }) | None => {
                    handle_settings_dashboard(&mut config).await?;
                }
            }
        }

        Some(Commands::Prompts { cmd }) => {
            handle_prompts_command(cmd).await?;
        }

        Some(Commands::Session { cmd }) => {
            handle_session_command(cmd, &config).await?;
        }

        Some(Commands::Server { port }) => {
            server::start_server(*port).await?;
        }

        Some(Commands::Jobs(_)) => {
            crate::cli::jobs::handle_list_jobs(get_job_registry()).await?;
        }

        Some(Commands::Daemon(args)) => {
            match args.cmd {
                DaemonCommand::Run => crate::cli::daemon::handle_daemon_run().await?,
                DaemonCommand::Start => crate::cli::daemon::handle_daemon_start()?,
                DaemonCommand::Stop => crate::cli::daemon::handle_daemon_stop()?,
            }
        }

        Some(Commands::Batch {
            input,
            output,
            model,
            rounds,
            concurrent,
        }) => {
            handle_batch(input, output, model, rounds, *concurrent, &config).await?;
        }

        Some(Commands::Ask {
            query,
            model,
            rounds,
        }) => {
            handle_ask(query, model.as_deref(), rounds, &config).await?;
        }

        None => {
            handle_hub(&mut config, &formatter, initial_context).await?;
        }
    }

    Ok(())
}

// TODO: PaCoRe module is temporarily disabled - pacore module doesn't exist
// async fn handle_batch(
//     input: &str,
//     output: &str,
//     model: &str,
//     rounds: &str,
//     concurrent: usize,
//     config: &Config,
// ) -> Result<()> {
//     use mylm_core::pacore::{Exp, ChatClient, load_jsonl, save_jsonl};
// 
//     println!("🚀 Starting PaCoRe Batch Process...");
//     println!("📂 Input: {}", input);
//     println!("📂 Output: {}", output);
// 
//     let resolved = config.resolve_profile();
//     let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
//     let api_key = resolved.api_key.ok_or_else(|| anyhow::anyhow!("No API key configured"))?;
// 
//     let client = ChatClient::new(base_url, api_key);
//     
//     let num_responses_per_round: Vec<usize> = rounds
//         .split(',')
//         .map(|s| s.trim().parse().unwrap_or(1))
//         .collect();
// 
//     let exp = Exp::new(
//         model.to_string(),
//         num_responses_per_round,
//         10, // max_concurrent_per_request (parallel calls)
//         client,
//     );
// 
//     let dataset = load_jsonl(input).await.map_err(|e| anyhow::anyhow!("Load error: {}", e))?;
//     println!("📊 Loaded {} items.", dataset.len());
// 
//     let results = exp.run_batch(dataset, concurrent).await;
//     
//     save_jsonl(output, &results).await.map_err(|e| anyhow::anyhow!("Save error: {}", e))?;
//     println!("✅ Batch complete. Results saved to {}", output);
// 
//     Ok(())
// }

async fn handle_batch(
    _input: &str,
    _output: &str,
    _model: &str,
    _rounds: &str,
    _concurrent: usize,
    _config: &Config,
) -> Result<()> {
    println!("⚠️  PaCoRe batch processing is temporarily unavailable.");
    println!("   The pacore module is being refactored.");
    Ok(())
}

// TODO: PaCoRe module is temporarily disabled - pacore module doesn't exist
// async fn handle_ask(
//     query: &str,
//     model: Option<&str>,
//     rounds: &str,
//     config: &Config,
// ) -> Result<()> {
//     use mylm_core::pacore::{Exp, ChatClient, Message};
//     use futures_util::StreamExt;
//     use std::io::{Write, stdout};
// 
//     let resolved = config.resolve_profile();
//     let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
//     let api_key = resolved.api_key.ok_or_else(|| anyhow::anyhow!("No API key configured"))?;
// 
//     let client = ChatClient::new(base_url, api_key);
//     
//     let num_responses_per_round: Vec<usize> = rounds
//         .split(',')
//         .map(|s| s.trim().parse().unwrap_or(1))
//         .collect();
// 
//     let model_name = model.unwrap_or(&resolved.model).to_string();
// 
//     let exp = Exp::new(
//         model_name.clone(),
//         num_responses_per_round,
//         10,
//         client,
//     );
// 
//     let messages = vec![Message::user(query)];
// 
//     println!("🤖 Thinking (using {} with rounds {})...", model_name, rounds);
// 
//     let mut stream = exp.process_single_stream(messages, "ask").await?;
// 
//     println!("\nFinal Answer:");
//     while let Some(chunk_result) = stream.next().await {
//         let chunk = chunk_result?;
//         if let Some(choice) = chunk.choices.first() {
//             if let Some(delta) = &choice.delta {
//                 print!("{}", delta.content);
//                 stdout().flush()?;
//             } else if let Some(message) = &choice.message {
//                 print!("{}", message.content);
//                 stdout().flush()?;
//             }
//         }
//     }
//     println!("\n");
// 
//     Ok(())
// }

async fn handle_ask(
    query: &str,
    _model: Option<&str>,
    _rounds: &str,
    config: &Config,
) -> Result<()> {
    println!("⚠️  PaCoRe ask is temporarily unavailable. Using standard query instead.");
    
    // Fallback to standard one-shot query
    let formatter = OutputFormatter::new();
    let cli = Cli::parse();
    handle_one_shot(&cli, query, config, &formatter).await
}

/// Handle the interactive hub menu
async fn handle_hub(config: &mut Config, formatter: &OutputFormatter, initial_context: mylm_core::context::TerminalContext) -> Result<()> {
    loop {
        let choice = crate::cli::hub::show_hub(config).await?;
        match choice {
            HubChoice::PopTerminal => {
                let context = mylm_core::context::TerminalContext::collect().await;
                let update_available = check_for_updates_fast();
                let _ = terminal::run_tui(None, None, Some(context), Some(initial_context.terminal.clone()), update_available, false).await?;
                continue;
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
                match App::load_session(None).await {
                    Ok(session) => {
                        let update_available = check_for_updates_fast();
                        let _ = terminal::run_tui(Some(session), None, None, None, update_available, false).await?;
                        continue;
                    }
                    Err(e) => {
                        println!("❌ Failed to load session: {}", e);
                    }
                }
            }
            HubChoice::StartTui => {
                let update_available = check_for_updates_fast();
                let _ = terminal::run_tui(None, None, None, None, update_available, false).await?;
                continue;
            }
            HubChoice::StartIncognito => {
                let update_available = check_for_updates_fast();
                let _ = terminal::run_tui(None, None, None, None, update_available, true).await?;
                continue;
            }
            HubChoice::QuickQuery => {
                let query = inquire::Text::new("⚡ Quick Query:").prompt()?;
                if !query.trim().is_empty() {
                    handle_one_shot(&Cli::parse(), &query, config, formatter).await?;
                }
            }

            HubChoice::BackgroundJobs => {
                crate::cli::jobs::handle_jobs_dashboard(config, get_job_registry()).await?;
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
                    match App::load_session(Some(id)).await {
                        Ok(session) => {
                            let update_available = check_for_updates_fast();
                            let _ = terminal::run_tui(Some(session), None, None, None, update_available, false).await?;
                            continue;
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
            match App::load_session(Some(id)).await {
                Ok(session) => {
                    let update_available = check_for_updates_fast();
                    let _ = terminal::run_tui(Some(session), None, None, None, update_available, false).await?;
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

/// Handle prompts commands (edit, reset, validate, list)
async fn handle_prompts_command(cmd: &PromptsCommand) -> Result<()> {
    use crate::cli::PromptType;
    
    let config_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join("mylm")
        .join("prompts");
    
    fn get_prompt_path(config_dir: &std::path::Path, prompt_type: &PromptType) -> std::path::PathBuf {
        match prompt_type {
            PromptType::System => config_dir.join("system.md"),
            PromptType::Worker => config_dir.join("worker.md"),
            PromptType::Memory => config_dir.join("memory.md"),
        }
    }
    
    fn get_prompt_name(prompt_type: &PromptType) -> &'static str {
        match prompt_type {
            PromptType::System => "default",
            PromptType::Worker => "worker",
            PromptType::Memory => "memory",
        }
    }
    
    match cmd {
        PromptsCommand::Edit { prompt_type } => {
            let prompt_path = get_prompt_path(&config_dir, prompt_type);
            let prompt_name = get_prompt_name(prompt_type);
            
            // Create directory if it doesn't exist
            std::fs::create_dir_all(&config_dir).context("Failed to create prompts directory")?;
            
            // If file doesn't exist, write default content
            if !prompt_path.exists() {
                let default_prompt = mylm_core::config::load_prompt(prompt_name).await
                    .context("Failed to load default prompt")?;
                std::fs::write(&prompt_path, default_prompt).context("Failed to write default prompt file")?;
            }
            
            // Open in editor
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
            let status = std::process::Command::new(editor)
                .arg(&prompt_path)
                .status()
                .context("Failed to launch editor")?;
            
            if status.success() {
                println!("✅ Prompts updated. Restart mylm to apply changes.");
            } else {
                anyhow::bail!("Editor exited with error");
            }
        }
        PromptsCommand::Reset { prompt_type } => {
            let prompt_path = get_prompt_path(&config_dir, prompt_type);
            
            if prompt_path.exists() {
                // Remove the custom file to fall back to compiled default
                std::fs::remove_file(&prompt_path).context("Failed to remove custom prompt file")?;
                println!("✅ Prompts reset to default. Restart mylm to apply changes.");
            } else {
                println!("ℹ️  No custom prompts found. System is using the default.");
            }
        }
        PromptsCommand::Validate => {
            println!("🔍 Validating prompt configurations...");
            
            for prompt_type in [PromptType::System, PromptType::Worker, PromptType::Memory] {
                let prompt_name = get_prompt_name(&prompt_type);
                match mylm_core::config::load_prompt(prompt_name).await {
                    Ok(_) => println!("  ✅ {} prompt is valid", prompt_name),
                    Err(e) => println!("  ❌ {} prompt error: {}", prompt_name, e),
                }
            }
        }
        PromptsCommand::List => {
            println!("📋 Available prompts:");
            
            for prompt_type in [PromptType::System, PromptType::Worker, PromptType::Memory] {
                let prompt_path = get_prompt_path(&config_dir, &prompt_type);
                let prompt_name = get_prompt_name(&prompt_type);
                let type_name = format!("{:?}", prompt_type).to_lowercase();
                
                if prompt_path.exists() {
                    println!("  {} {} (custom)", type_name, prompt_path.display());
                } else {
                    println!("  {} {} (default/embedded)", type_name, prompt_name);
                }
            }
        }
    }
    
    Ok(())
}

/// Handle the unified settings dashboard - Multi-Provider Menu System
async fn handle_settings_dashboard(config: &mut Config) -> Result<()> {
    // Ensure we have at least one profile if none exist
    if config.profiles.is_empty() {
        config.create_profile("default")?;
        config.set_active_profile("default")?;
    }

    // Ensure active profile is valid
    if !config.profiles.contains_key(&config.profile) {
        if let Some(first) = config.profile_names().first() {
            config.profile = first.clone();
        }
    }

    loop {
        let choice = crate::cli::hub::show_settings_dashboard(config)?;

        match choice {
            crate::cli::hub::SettingsMenuChoice::ManageProviders => {
                loop {
                    let action = crate::cli::hub::show_provider_menu()?;
                    match action {
                        crate::cli::hub::ProviderMenuChoice::AddProvider => {
                            crate::cli::hub::handle_add_provider(config).await?;
                        }
                        crate::cli::hub::ProviderMenuChoice::EditProvider => {
                            crate::cli::hub::handle_edit_provider(config).await?;
                        }
                        crate::cli::hub::ProviderMenuChoice::RemoveProvider => {
                            crate::cli::hub::handle_remove_provider(config)?;
                        }
                        crate::cli::hub::ProviderMenuChoice::Back => break,
                    }
                }
            }

            crate::cli::hub::SettingsMenuChoice::SelectMainModel => {
                crate::cli::hub::handle_select_main_model(config).await?;
            }

            crate::cli::hub::SettingsMenuChoice::SelectWorkerModel => {
                crate::cli::hub::handle_select_worker_model(config).await?;
            }

            crate::cli::hub::SettingsMenuChoice::WebSearchSettings => {
                crate::cli::hub::handle_web_search_settings(config).await?;
            }

            crate::cli::hub::SettingsMenuChoice::AgentSettings => {
                loop {
                    let action = crate::cli::hub::show_agent_settings_menu(config)?;
                    match action {
                        crate::cli::hub::AgentSettingsChoice::IterationsSettings => {
                            loop {
                                let iter_action = crate::cli::hub::show_iterations_settings_menu(config)?;
                                match iter_action {
                                    crate::cli::hub::IterationsSettingsChoice::SetMaxIterations => {
                                        crate::cli::hub::handle_max_iterations(config)?;
                                    }
                                    crate::cli::hub::IterationsSettingsChoice::SetRateLimit => {
                                        crate::cli::hub::handle_set_rate_limit(config)?;
                                    }
                                    crate::cli::hub::IterationsSettingsChoice::Back => break,
                                }
                            }
                        }
                        crate::cli::hub::AgentSettingsChoice::RateLimitSettings => {
                            loop {
                                let rl_action = crate::cli::hub::show_rate_limit_settings_menu(config)?;
                                match rl_action {
                                    crate::cli::hub::RateLimitSettingsChoice::SetRateLimitTier => {
                                        crate::cli::hub::handle_set_rate_limit_tier(config)?;
                                    }
                                    crate::cli::hub::RateLimitSettingsChoice::SetWorkerLimit => {
                                        crate::cli::hub::handle_set_worker_limit(config)?;
                                    }
                                    crate::cli::hub::RateLimitSettingsChoice::SetMainRpm => {
                                        crate::cli::hub::handle_set_main_rpm(config)?;
                                    }
                                    crate::cli::hub::RateLimitSettingsChoice::SetWorkersRpm => {
                                        crate::cli::hub::handle_set_workers_rpm(config)?;
                                    }
                                    crate::cli::hub::RateLimitSettingsChoice::Back => break,
                                }
                            }
                        }
                        crate::cli::hub::AgentSettingsChoice::ToggleTmuxAutostart => {
                            crate::cli::hub::handle_toggle_tmux_autostart(config)?;
                        }
                        crate::cli::hub::AgentSettingsChoice::ToggleAgentVersion => {
                            crate::cli::hub::handle_toggle_agent_version(config)?;
                        }
                        crate::cli::hub::AgentSettingsChoice::PaCoReSettings => {
                            loop {
                                let pacore_action = crate::cli::hub::show_pacore_settings_menu(config)?;
                                match pacore_action {
                                    crate::cli::hub::PaCoReSettingsChoice::TogglePaCoRe => {
                                        crate::cli::hub::handle_toggle_pacore(config)?;
                                    }
                                    crate::cli::hub::PaCoReSettingsChoice::SetPaCoReRounds => {
                                        crate::cli::hub::handle_set_pacore_rounds(config)?;
                                    }
                                    crate::cli::hub::PaCoReSettingsChoice::Back => break,
                                }
                            }
                        }
                        crate::cli::hub::AgentSettingsChoice::PermissionsSettings => {
                            loop {
                                let perms_action = crate::cli::hub::show_permissions_menu(config)?;
                                match perms_action {
                                    crate::cli::hub::PermissionsMenuChoice::SetAllowedTools => {
                                        crate::cli::hub::handle_set_allowed_tools(config)?;
                                    }
                                    crate::cli::hub::PermissionsMenuChoice::SetAutoApproveCommands => {
                                        crate::cli::hub::handle_set_auto_approve_commands(config)?;
                                    }
                                    crate::cli::hub::PermissionsMenuChoice::SetForbiddenCommands => {
                                        crate::cli::hub::handle_set_forbidden_commands(config)?;
                                    }
                                    crate::cli::hub::PermissionsMenuChoice::ConfigureWorkerShell => {
                                        loop {
                                            let worker_action = crate::cli::hub::show_worker_shell_menu(config)?;
                                            match worker_action {
                                                crate::cli::hub::WorkerShellMenuChoice::SetAllowedPatterns => {
                                                    crate::cli::hub::handle_set_worker_allowed_commands(config)?;
                                                }
                                                crate::cli::hub::WorkerShellMenuChoice::SetRestrictedPatterns => {
                                                    crate::cli::hub::handle_set_worker_restricted_commands(config)?;
                                                }
                                                crate::cli::hub::WorkerShellMenuChoice::SetForbiddenPatterns => {
                                                    crate::cli::hub::handle_set_worker_forbidden_commands(config)?;
                                                }
                                                crate::cli::hub::WorkerShellMenuChoice::SetEscalationMode => {
                                                    crate::cli::hub::handle_set_worker_escalation_mode(config)?;
                                                }
                                                crate::cli::hub::WorkerShellMenuChoice::ResetToDefaults => {
                                                    crate::cli::hub::handle_reset_worker_shell_defaults(config)?;
                                                }
                                                crate::cli::hub::WorkerShellMenuChoice::Back => break,
                                            }
                                        }
                                    }
                                    crate::cli::hub::PermissionsMenuChoice::Back => break,
                                }
                            }
                        }
                        crate::cli::hub::AgentSettingsChoice::WorkerResilienceSettings => {
                            loop {
                                let resilience_action = crate::cli::hub::show_worker_resilience_menu(config)?;
                                match resilience_action {
                                    crate::cli::hub::WorkerResilienceSettingsChoice::SetMaxToolFailures => {
                                        crate::cli::hub::handle_set_max_tool_failures(config)?;
                                    }
                                    crate::cli::hub::WorkerResilienceSettingsChoice::Back => break,
                                }
                            }
                        }
                        crate::cli::hub::AgentSettingsChoice::Back => break,
                    }
                }
            }

            crate::cli::hub::SettingsMenuChoice::PromptSettings => {
                loop {
                    let action = crate::cli::hub::show_prompt_settings_menu(config)?;
                    match action {
                        crate::cli::hub::PromptMenuChoice::SystemPrompt => {
                            crate::cli::hub::handle_system_prompt(config)?;
                        }
                        crate::cli::hub::PromptMenuChoice::WorkerPrompt => {
                            crate::cli::hub::handle_worker_prompt(config)?;
                        }
                        crate::cli::hub::PromptMenuChoice::MemoryPrompt => {
                            crate::cli::hub::handle_memory_prompt(config)?;
                        }
                        crate::cli::hub::PromptMenuChoice::ViewCurrentPrompts => {
                            crate::cli::hub::view_current_prompts(config)?;
                        }
                        crate::cli::hub::PromptMenuChoice::Back => break,
                    }
                }
            }

            crate::cli::hub::SettingsMenuChoice::Back => break,
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

    // Resolve configuration for active profile
    let resolved = config.resolve_profile();
    
    let base_url = resolved.base_url.unwrap_or_else(|| resolved.provider.default_url());
    let api_key = resolved.api_key.unwrap_or_default();
    
    if api_key.is_empty() {
        println!("❌ No API key configured.");
        println!("   Run 'mylm' to open the configuration hub and set up an endpoint.");
        return Ok(());
    }

    // Create LLM client
            let llm_config = LlmConfig::new(
                format!("{:?}", resolved.provider).to_lowercase().parse().map_err(|e| anyhow::anyhow!("{}", e))?,
                base_url,
                resolved.model.clone(),
                Some(api_key),
                resolved.agent.max_context_tokens,
            )
            .with_memory(config.features.memory.clone());
    let client = Arc::new(LlmClient::new(llm_config)?);

    // Build hierarchical system prompt
    let system_prompt = mylm_core::config::build_system_prompt(&ctx, "default", Some("CLI (Single Query)"), None, None, None).await?;

    println!("{} Querying {} with model {}...",
        blue.apply_to("🤖"),
        green.apply_to(format!("{:?}", resolved.provider).to_lowercase()),
        green.apply_to(&resolved.model)
    );

    // Initialize dependencies for tools
    let (_event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<crate::terminal::app::TuiEvent>();
    
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
    let store = Arc::new(mylm_core::memory::VectorStore::new(&data_dir.to_string_lossy()).await?);

    // Initialize state store
    let state_store = Arc::new(std::sync::RwLock::new(mylm_core::state::StateStore::new()?));

    // Load tools
    let job_registry = get_job_registry().clone();
    let scribe = Arc::new(mylm_core::memory::scribe::Scribe::new(
        Arc::new(tokio::sync::Mutex::new(mylm_core::memory::journal::Journal::new().unwrap())),
        store.clone(),
        client.clone()
    ));

    // Create builder with tools
    let resolved = config.resolve_profile();
    let terminal_executor: Arc<dyn TerminalExecutor> = Arc::new(HeadlessTerminalExecutor);
    let crawl_event_bus = Arc::new(mylm_core::agent::event_bus::EventBus::new());
    let categorizer = Arc::new(mylm_core::memory::MemoryCategorizer::new(client.clone(), store.clone()));
    
    let max_iterations = config.get_active_profile_info()
        .and_then(|p| p.max_iterations)
        .unwrap_or(10);

    let builder = mylm_core::agent::v2::driver::factory::AgentConfigs::one_shot(
        client.clone(),
        executor.clone(),
        ctx.clone(),
        terminal_executor.clone(),
        store.clone(),
        Some(categorizer.clone()),
        job_registry.clone(),
        resolved.agent.permissions.clone(),
        config.features.web_search.clone(),
        crawl_event_bus.clone(),
        state_store.clone(),
    )
    .with_system_prompt(system_prompt)
    .with_max_iterations(max_iterations);

    // Extract tools to build map for delegate
    let tools = builder.build_tools().await?;
    
    // Build tools HashMap for DelegateTool
    let mut tools_map = std::collections::HashMap::new();
    for tool in &tools {
        tools_map.insert(tool.name().to_string(), tool.clone());
    }

    // Create executor for DelegateTool
    use mylm_core::executor::{CommandExecutor, allowlist::CommandAllowlist, safety::SafetyChecker};
    let allowlist = CommandAllowlist::new();
    let safety_checker = SafetyChecker::new();
    let executor = Arc::new(CommandExecutor::new(allowlist, safety_checker));

    // Create DelegateTool with access to all tools
    let delegate_config = mylm_core::agent::tools::delegate::DelegateToolConfig {
        llm_client: client.clone(),
        scribe,
        job_registry: job_registry.clone(),
        memory_store: Some(store.clone()),
        categorizer: Some(categorizer.clone()),
        event_bus: None,
        tools: tools_map,
        permissions: None,
        max_iterations: 50,
        executor,
        max_tool_failures: 5,
        worker_model: Some(resolved.agent.worker_model.clone()),
        providers: config.providers.clone(),
    };
    let delegate_tool = mylm_core::agent::tools::delegate::DelegateTool::new(delegate_config);

    // Add DelegateTool to builder and build agent
    let builder = builder.with_tool(Box::new(delegate_tool));
    let built_agent = builder.build().await;
    
    let mut agent = match built_agent {
        mylm_core::BuiltAgent::V1(a) => a,
        mylm_core::BuiltAgent::V2(_) => anyhow::bail!("Unexpected Agent V2 in one-shot mode"),
    };
    
    let messages = vec![
        mylm_core::llm::chat::ChatMessage::user(query.to_string()),
    ];

    // Dummy interrupt flag for one-shot
    let interrupt_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let run_event_bus = Arc::new(mylm_core::agent::event_bus::EventBus::new());
    match agent.run(messages, run_event_bus, interrupt_flag, auto_approve, 30, None).await {
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
fn check_for_updates_fast() -> bool {
    // Check if we are in a git repo
    if !std::path::Path::new(".git").exists() {
        return false;
    }

    // Try to fetch in background with a timeout
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
    let _ = fetch_handle.join();
    
    Ok(())
}

pub fn toggle_tmux_autostart() -> Result<()> {
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
