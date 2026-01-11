//! `mylm` - A globally available, high-performance terminal AI assistant
//!
//! This binary provides a CLI interface for interacting with LLM endpoints
//! while collecting terminal context and safely executing sysadmin tasks.

use anyhow::{Context, Result};
use clap::Parser;
use console::Style;
use std::sync::Arc;

use crate::cli::{Cli, Commands, MemoryCommand, ConfigCommand, EditCommand, hub::{HubChoice, ConfigChoice}};
use crate::config::Config;
use crate::context::TerminalContext;
use crate::llm::{LlmClient, LlmConfig};
use crate::output::OutputFormatter;
use crate::terminal::app::App;

mod agent;
mod cli;
mod config;
mod context;
mod executor;
mod llm;
mod memory;
mod output;
mod terminal;

/// Main entry point for the AI assistant CLI
#[tokio::main]
async fn main() -> Result<()> {
    // Parse command-line arguments
    let cli = Cli::parse();

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
    let config = Config::load().context("Failed to load configuration")?;

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
            );
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
                crate::memory::VectorStore::warmup().await?;
            } else {
                Config::setup().await?;
                crate::memory::VectorStore::warmup().await?;
            }
        }

        Some(Commands::System { brief }) => {
            let ctx = TerminalContext::collect().await;
            formatter.print_system_info(&ctx, *brief);
        }

        Some(Commands::Interactive) => {
            terminal::run_tui(None).await?;
        }

        Some(Commands::Memory { cmd }) => {
            let data_dir = dirs::data_dir()
                .context("Could not find data directory")?
                .join("mylm")
                .join("memory");
            
            std::fs::create_dir_all(&data_dir)?;
            let store = memory::VectorStore::new(data_dir.to_str().unwrap()).await?;

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
                    let mut new_config = config.clone();
                    new_config.active_profile = ans;
                    if let Some(path) = crate::config::find_config_file() {
                        new_config.save(path)?;
                        println!("Active profile set to {}", new_config.active_profile);
                    }
                }
                Some(ConfigCommand::New) => {
                    println!("Use 'ai setup' to create a new configuration.");
                }
                Some(ConfigCommand::Edit { cmd: edit_cmd }) => {
                    match edit_cmd {
                        EditCommand::Prompt => {
                            let profile = config.get_active_profile()
                                .map(|p| p.prompt.clone())
                                .unwrap_or_else(|| "default".to_string());
                            let path = crate::config::prompt::get_prompts_dir().join(format!("{}.md", profile));
                            let _ = crate::config::prompt::load_prompt(&profile)?;
                            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
                            std::process::Command::new(editor).arg(path).status()?;
                        }
                    }
                }
                None => {
                    handle_config_menu(&config).await?;
                }
            }
        }

        None => {
            handle_hub(&config, &formatter).await?;
        }
    }

    Ok(())
}

/// Handle the interactive hub menu
async fn handle_hub(config: &Config, formatter: &OutputFormatter) -> Result<()> {
    loop {
        let choice = crate::cli::hub::show_hub(config).await?;
        match choice {
            HubChoice::ResumeSession => {
                match App::load_session() {
                    Ok(history) => {
                        terminal::run_tui(Some(history)).await?;
                        break;
                    }
                    Err(e) => {
                        println!("❌ Failed to load session: {}", e);
                    }
                }
            }
            HubChoice::StartTui => {
                terminal::run_tui(None).await?;
                break;
            }
            HubChoice::QuickQuery => {
                let query = inquire::Text::new("⚡ Quick Query:").prompt()?;
                if !query.trim().is_empty() {
                    handle_one_shot(&Cli::parse(), &query, config, formatter).await?;
                }
            }
            HubChoice::Configuration => {
                handle_config_menu(config).await?;
            }
            HubChoice::Exit => break,
        }
    }
    Ok(())
}

/// Handle the configuration menu
async fn handle_config_menu(config: &Config) -> Result<()> {
    loop {
        let choice = crate::cli::hub::show_config_menu().await?;
        match choice {
            ConfigChoice::SelectProfile => {
                let profiles: Vec<String> = config.profiles.iter().map(|p| p.name.clone()).collect();
                if profiles.is_empty() {
                    println!("No profiles found.");
                    continue;
                }

                if let Some(ans) = crate::cli::hub::show_profile_select(profiles)? {
                    let mut new_config = config.clone();
                    new_config.active_profile = ans;
                    if let Some(path) = crate::config::find_config_file() {
                        new_config.save(path)?;
                        println!("Active profile set to {}", new_config.active_profile);
                    }
                }
            }
            ConfigChoice::EditProfile => {
                let profile = config.get_active_profile()
                    .map(|p| p.prompt.clone())
                    .unwrap_or_else(|| "default".to_string());
                let path = crate::config::prompt::get_prompts_dir().join(format!("{}.md", profile));
                let _ = crate::config::prompt::load_prompt(&profile)?;
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
                std::process::Command::new(editor).arg(path).status()?;
            }
            ConfigChoice::NewProfile => {
                println!("Use 'ai setup' to create a new configuration.");
            }
            ConfigChoice::Back => break,
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

    // Create LLM client
    let llm_config = LlmConfig::new(
        endpoint_config.provider.parse().map_err(|e| anyhow::anyhow!("{}", e))?,
        endpoint_config.base_url.clone(),
        endpoint_config.model.clone(),
        Some(endpoint_config.api_key.clone()),
    );
    let client = Arc::new(LlmClient::new(llm_config)?);

    // Build hierarchical system prompt
    let prompt_name = config.get_active_profile()
        .map(|p| p.prompt.as_str())
        .unwrap_or("default");
    let system_prompt = crate::config::prompt::build_system_prompt(&ctx, prompt_name, Some("CLI (Single Query)")).await?;

    println!("{} Querying {}...",
        blue.apply_to("🤖"),
        green.apply_to(&endpoint_config.name)
    );

    // Initialize dependencies for tools
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<crate::terminal::app::TuiEvent>();
    let allowlist = crate::executor::allowlist::CommandAllowlist::new();
    let safety_checker = crate::executor::safety::SafetyChecker::new();
    let executor = Arc::new(crate::executor::CommandExecutor::new(
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
    let store = Arc::new(crate::memory::VectorStore::new(data_dir.to_str().unwrap()).await?);

    // Load tools
    let tools: Vec<Box<dyn crate::agent::Tool>> = vec![
        Box::new(crate::agent::tools::shell::ShellTool::new(executor, ctx.clone(), event_tx.clone())) as Box<dyn crate::agent::Tool>,
        Box::new(crate::agent::tools::web_search::WebSearchTool::new(config.web_search.clone())) as Box<dyn crate::agent::Tool>,
        Box::new(crate::agent::tools::memory::MemoryTool::new(store)) as Box<dyn crate::agent::Tool>,
    ];

    let mut agent = crate::agent::Agent::new(client, tools, system_prompt);
    
    let messages = vec![
        crate::llm::chat::ChatMessage::user(query.to_string()),
    ];

    // Dummy interrupt flag for one-shot
    let interrupt_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    match agent.run(messages, event_tx, interrupt_flag).await {
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
