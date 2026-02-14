//! CLI Commands for mylm V3

use clap::{Parser, Subcommand};

/// mylm - Terminal AI Assistant (V3)
#[derive(Parser)]
#[command(name = "mylm")]
#[command(about = "Terminal AI Assistant - V3 Architecture")]
#[command(version)]
pub struct Cli {
    /// Optional query to send directly
    pub query: Vec<String>,
    
    /// Subcommand
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Show configuration menu
    Config,
    /// Run setup wizard
    Setup,
}
