//! Configuration system v2 with profile support and environment overrides.
//!
//! Provides a flexible configuration system supporting multiple profiles,
//! TOML-based configuration files, and environment variable overrides.
//!
//! # Submodules
//! - `types`: Core configuration types and structs
//! - `config`: Configuration loading, saving, and resolution logic
//! - `prompts`: Prompt templates for different agent modes
//! - `prompt_schema`: Data-driven prompt configuration schema

pub mod types;
pub mod config;
pub mod prompts;
pub mod prompt_schema;

#[cfg(test)]
mod tests;

pub use types::*;
pub use config::*;
pub use prompts::*;
pub use prompt_schema::*;
