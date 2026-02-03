//! CLI argument parsing using clap 4.x derive macros

pub mod daemon;
pub mod hub;
pub mod jobs;

use clap::{Parser, Subcommand};

#[derive(clap::Args, Debug)]
pub struct JobsArgs {}

#[derive(clap::Args, Debug)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub cmd: DaemonCommand,
}

#[derive(Subcommand, Debug)]
pub enum DaemonCommand {
    /// Run the daemon in the foreground
    Run,
    /// Start the daemon in the background
    Start,
    /// Stop the running daemon
    Stop,
}

/// A globally available, high-performance terminal AI assistant
///
/// Works with OpenAI-compatible endpoints (Ollama, LM Studio, local models)
/// and provides terminal context collection and safe command execution.
#[derive(Parser, Debug)]
#[command(name = "ai")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(disable_version_flag = true)]
pub struct Cli {
    /// The command to execute
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Direct query (alternative to 'query' subcommand)
    #[arg(num_args = 1..)]
    pub query: Vec<String>,

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

    /// Interactive configuration setup
    Setup {
        /// Only perform model warmup (downloading models)
        #[arg(short, long)]
        warmup: bool,
    },

    /// Show system information
    System {
        /// Brief output (summary only)
        #[arg(short, long)]
        brief: bool,
    },

    /// Start interactive TUI mode with terminal and AI chat
    Interactive,

    /// Pop into the current terminal session (requires tmux)
    ///
    /// This restores your existing terminal history and environment
    /// for seamless context awareness.
    Pop,

    /// Manage persistent memory (RAG)
    Memory {
        #[command(subcommand)]
        cmd: MemoryCommand,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        cmd: Option<ConfigCommand>,
    },
    
    /// Manage chat sessions
    Session {
        #[command(subcommand)]
        cmd: SessionCommand,
    },

    /// Start the WebSocket server
    Server {
        /// Port to listen on
        #[arg(short, long, default_value_t = 41901)]
        port: u16,
    },

    /// Parallel Consistency Reasoning (PaCoRe) batch processing
    Batch {
        /// Input JSONL file path
        #[arg(short, long)]
        input: String,

        /// Output JSONL file path
        #[arg(short, long)]
        output: String,

        /// LLM model to use
        #[arg(short, long)]
        model: String,

        /// Number of responses per round (comma-separated, e.g., "4,1")
        #[arg(short, long, default_value = "4,1")]
        rounds: String,

        /// Maximum concurrent requests
        #[arg(short, long, default_value_t = 1)]
        concurrent: usize,
    },

    /// Single prompt reasoning with streaming output
    Ask {
        /// The question or task for the AI
        query: String,

        /// LLM model to use
        #[arg(short, long)]
        model: Option<String>,

        /// Number of responses per round (comma-separated, e.g., "4,1")
        #[arg(short, long, default_value = "4,1")]
        rounds: String,
    },

    /// Background jobs CLI (placeholder)
    Jobs(JobsArgs),

    /// Background jobs daemon (placeholder)
    Daemon(DaemonArgs),
}

#[derive(Subcommand, Debug)]
pub enum SessionCommand {
    /// List all saved sessions
    List,
    /// Resume a specific session
    Resume {
        /// Session ID or Filename
        id: String,
    },
    /// Delete a session
    Delete {
        /// Session ID or Filename
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Edit configuration files
    Edit {
        #[command(subcommand)]
        cmd: Option<EditCommand>,
    },

    /// Select active profile
    Select,

    /// Create new profile
    New,
}

#[derive(Subcommand, Debug)]
pub enum EditCommand {
    /// Edit the system prompt instructions
    Prompt,
}

#[derive(Subcommand, Debug)]
pub enum MemoryCommand {
    /// Add content to memory
    Add {
        /// Content to remember
        content: String,
    },
    /// Search through memory
    Search {
        /// Search query
        query: String,
        /// Number of results to return
        #[arg(short, long, default_value = "5")]
        limit: usize,
    },
}
