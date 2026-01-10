//! `mylm` - A globally available, high-performance terminal AI assistant
//!
//! This binary provides a CLI interface for interacting with LLM endpoints
//! while collecting terminal context and safely executing sysadmin tasks.

use anyhow::{Context, Result};
use clap::Parser;
use console::Style;

use crate::cli::{Cli, Commands};
use crate::config::Config;
use crate::context::ContextCollector;
use crate::llm::LlmClient;
use crate::output::OutputFormatter;

mod cli;
mod config;
mod context;
mod executor;
mod llm;
mod output;

/// Main entry point for the AI assistant CLI
#[tokio::main]
async fn main() -> Result<()> {
    // Parse command-line arguments
    let cli = Cli::parse();

    // Setup output formatting
    let formatter = OutputFormatter::new();
    let blue = Style::new().blue();
    let green = Style::new().green();

    // Load configuration
    let config = Config::load().context("Failed to load configuration")?;

    // Print version if requested
    if cli.version {
        println!("{} v{}", blue.apply_to("mylm"), env!("CARGO_PKG_VERSION"));
        println!("Built with Rust + Love for terminal productivity");
        return Ok(());
    }

    // Handle different commands
    match &cli.command {
        Commands::Query {
            query,
            execute: _,
            force: _,
        } => {
            // Collect terminal context
            let ctx = ContextCollector::collect().await;

            // Determine which endpoint to use
            let endpoint_config = config.get_endpoint(cli.endpoint.as_deref())?;

            // Create LLM client
            let mut client = LlmClient::new(endpoint_config.clone())?;

            // Build the prompt with context
            let prompt = ctx.build_prompt(query);

            // Send request to LLM
            println!("{} Querying {}...", 
                blue.apply_to("🤖"),
                green.apply_to(&endpoint_config.name)
            );

            let response = client.complete(&prompt).await?;

            // Display response
            formatter.print_response(&response);
        }

        Commands::Context { format: _ } => {
            // Collect and display current context
            let ctx = ContextCollector::collect().await;
            formatter.print_context(&ctx);
        }

        Commands::Execute {
            command,
            dry_run: _,
        } => {
            // Collect context for the command
            let ctx = ContextCollector::collect().await;

            // Determine which endpoint to use
            let endpoint_config = config.get_endpoint(cli.endpoint.as_deref())?;

            // Create LLM client
            let mut client = LlmClient::new(endpoint_config.clone())?;

            // Ask LLM to analyze and potentially execute the command
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

        Commands::Endpoints => {
            // List available endpoints
            formatter.print_endpoints(&config.endpoints);
        }

        Commands::System { brief } => {
            // Display system information
            let ctx = ContextCollector::collect().await;
            formatter.print_system_info(&ctx, *brief);
        }
    }

    Ok(())
}
