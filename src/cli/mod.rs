//! CLI argument parsing using clap 4.x derive macros

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// A globally available, high-performance terminal AI assistant
///
/// Works with OpenAI-compatible endpoints (Ollama, LM Studio, local models)
/// and provides terminal context collection and safe command execution.
#[derive(Parser, Debug)]
#[command(name = "ai")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// The command to execute
    #[command(subcommand)]
    pub command: Commands,

    /// Endpoint name to use (overrides default)
    #[arg(short, long)]
    pub endpoint: Option<String>,

    /// Print version information
    #[arg(long)]
    pub version: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Query the AI with terminal context
    Query {
        /// The question or task for the AI
        query: String,

        /// Execute safe commands suggested by AI
        #[arg(short, long)]
        execute: bool,

        /// Force execution of potentially dangerous commands
        #[arg(short, long, requires = "execute")]
        force: bool,
    },

    /// Show current terminal context
    Context {
        /// Output format (json, yaml, text)
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Analyze and execute a command with AI guidance
    Execute {
        /// The command to analyze/execute
        command: String,

        /// Dry run only (no execution)
        #[arg(short, long)]
        dry_run: bool,
    },

    /// List available endpoints
    Endpoints,

    /// Show system information
    System {
        /// Brief output (summary only)
        #[arg(short, long)]
        brief: bool,
    },
}
