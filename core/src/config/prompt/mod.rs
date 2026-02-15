//! Prompt configuration management
//!
//! This module provides configuration-driven prompt management for the agent.
//! Prompts are loaded from JSON files and can be customized without code changes.
//!
//! # Example
//!
//! ```rust
//! use mylm_core::config::prompt::{PromptManager, RenderContext, ToolInfo, ToolCategory};
//!
//! // Create manager with default config directory
//! let mut manager = PromptManager::default();
//!
//! // Ensure default configs exist
//! manager.ensure_defaults().unwrap();
//!
//! // Load and render a prompt
//! let config = manager.load("system").unwrap();
//!
//! let context = RenderContext::new()
//!     .with_working_directory("/home/user/project")
//!     .with_mode("development")
//!     .with_tools(vec![
//!         ToolInfo {
//!             name: "read_file".to_string(),
//!             description: "Read file contents".to_string(),
//!             usage: "read_file <path>".to_string(),
//!             category: ToolCategory::Internal,
//!         },
//!     ]);
//!
//! let prompt = manager.render(&config, &context);
//! ```

mod loader;
mod renderer;

pub use loader::{PromptLoader, PromptLoaderError};
pub use renderer::{PromptRenderer, RenderContext, ToolInfo, ToolCategory};

use crate::config::prompt_schema::PromptConfig;

/// Main API for prompt management
///
/// Combines loading and rendering capabilities into a unified interface.
#[derive(Debug)]
pub struct PromptManager {
    loader: PromptLoader,
}

impl PromptManager {
    /// Create a new prompt manager with the default config directory
    pub fn new() -> Self {
        Self {
            loader: PromptLoader::default(),
        }
    }

    /// Create a prompt manager with a specific config directory
    pub fn with_config_dir(config_dir: impl AsRef<std::path::Path>) -> Self {
        Self {
            loader: PromptLoader::new(config_dir),
        }
    }

    /// Load a prompt configuration by name
    ///
    /// # Arguments
    /// * `name` - Name of the prompt (e.g., "system", "minimal", "worker")
    ///
    /// # Returns
    /// The loaded PromptConfig, or an error if not found
    ///
    /// # Example
    /// ```rust
    /// # use mylm_core::config::prompt::PromptManager;
    /// let mut manager = PromptManager::default();
    /// let config = manager.load("system");
    /// ```
    pub fn load(&mut self, name: &str) -> Result<PromptConfig, PromptLoaderError> {
        self.loader.load(name)
    }

    /// Render a prompt configuration to a string
    ///
    /// # Arguments
    /// * `config` - The PromptConfig to render
    /// * `context` - Context for template substitution and dynamic generation
    ///
    /// # Returns
    /// The rendered prompt string ready for use as a system prompt
    pub fn render(&self, config: &PromptConfig, context: &RenderContext) -> String {
        PromptRenderer::render(config, context)
    }

    /// Load and render a prompt in one step
    ///
    /// # Arguments
    /// * `name` - Name of the prompt to load
    /// * `context` - Context for rendering
    ///
    /// # Returns
    /// The rendered prompt string, or an error if loading fails
    pub fn load_and_render(
        &mut self,
        name: &str,
        context: &RenderContext,
    ) -> Result<String, PromptLoaderError> {
        let config = self.load(name)?;
        Ok(self.render(&config, context))
    }

    /// Ensure default prompt files exist
    ///
    /// Creates default prompt configs if they don't already exist.
    /// Returns the number of files created.
    ///
    /// # Example
    /// ```rust
    /// # use mylm_core::config::prompt::PromptManager;
    /// let manager = PromptManager::default();
    /// let created = manager.ensure_defaults().unwrap();
    /// println!("Created {} new prompt files", created);
    /// ```
    pub fn ensure_defaults(&self) -> Result<usize, PromptLoaderError> {
        self.loader.ensure_defaults()
    }

    /// Reload a prompt from disk
    ///
    /// Clears the cache and reloads the specified prompt from file.
    pub fn reload(&mut self, name: &str) -> Result<PromptConfig, PromptLoaderError> {
        self.loader.reload(name)
    }

    /// Clear the prompt cache
    ///
    /// Forces prompts to be reloaded from disk on next access.
    pub fn clear_cache(&mut self) {
        self.loader.clear_cache();
    }

    /// List available prompt configurations
    ///
    /// Returns a list of prompt names that can be loaded.
    pub fn list_available(&self) -> Vec<String> {
        self.loader.list_available()
    }

    /// Get the configuration directory path
    pub fn config_dir(&self) -> &std::path::Path {
        &self.loader.config_dir
    }
}

impl Default for PromptManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience functions for common use cases
pub mod util {
    use super::*;

    /// Quick render of a system prompt with tools
    ///
    /// This is a convenience function for the most common case:
    /// loading the "system" prompt and rendering it with tools.
    pub fn render_system_prompt(
        tools: Vec<ToolInfo>,
        working_directory: Option<String>,
        mode: Option<String>,
    ) -> Result<String, PromptLoaderError> {
        let mut manager = PromptManager::default();
        
        let mut context = RenderContext::new().with_tools(tools);
        
        if let Some(dir) = working_directory {
            context = context.with_working_directory(dir);
        }
        if let Some(m) = mode {
            context = context.with_mode(m);
        }
        
        manager.load_and_render("system", &context)
    }

    /// Render a minimal prompt with tools
    pub fn render_minimal_prompt(
        tools: Vec<ToolInfo>,
    ) -> Result<String, PromptLoaderError> {
        let mut manager = PromptManager::default();
        let context = RenderContext::new().with_tools(tools);
        manager.load_and_render("minimal", &context)
    }

    /// Render a worker prompt with tools
    pub fn render_worker_prompt(
        tools: Vec<ToolInfo>,
    ) -> Result<String, PromptLoaderError> {
        let mut manager = PromptManager::default();
        let context = RenderContext::new().with_tools(tools);
        manager.load_and_render("worker", &context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_load_builtin() {
        let mut manager = PromptManager::default();
        let config = manager.load("system").unwrap();
        
        assert_eq!(config.identity.name, "MYLM");
        assert!(!config.sections.is_empty());
    }

    #[test]
    fn test_manager_render() {
        let mut manager = PromptManager::default();
        let config = manager.load("minimal").unwrap();
        
        let context = RenderContext::new();
        let rendered = manager.render(&config, &context);
        
        assert!(rendered.contains("MYLM"));
        assert!(rendered.contains("autonomous AI assistant"));
    }

    #[test]
    fn test_manager_load_and_render() {
        let mut manager = PromptManager::default();
        
        let context = RenderContext::new()
            .with_mode("test");
        
        let rendered = manager.load_and_render("minimal", &context).unwrap();
        
        assert!(!rendered.is_empty());
    }

    #[test]
    fn test_list_available() {
        let manager = PromptManager::default();
        let names = manager.list_available();
        
        assert!(names.contains(&"system".to_string()));
        assert!(names.contains(&"minimal".to_string()));
        assert!(names.contains(&"worker".to_string()));
    }
}
