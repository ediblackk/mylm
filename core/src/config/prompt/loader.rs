//! Prompt configuration loader
//!
//! Loads prompt configurations from files or creates defaults.
//! Supports hot-reloading and embedded fallbacks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::prompt_schema::{PromptConfig, Section, IdentitySection, Protocols, JsonKeys};

/// Error type for prompt loading operations
#[derive(Debug, thiserror::Error)]
pub enum PromptLoaderError {
    #[error("Failed to read prompt file: {0}")]
    ReadError(String),
    #[error("Failed to parse prompt JSON: {0}")]
    ParseError(String),
    #[error("Prompt not found: {0}")]
    NotFound(String),
}

/// Loader for prompt configurations
#[derive(Debug)]
pub struct PromptLoader {
    pub(crate) config_dir: PathBuf,
    cache: HashMap<String, PromptConfig>,
}

impl PromptLoader {
    /// Create a new loader with the specified config directory
    pub fn new(config_dir: impl AsRef<Path>) -> Self {
        Self {
            config_dir: config_dir.as_ref().to_path_buf(),
            cache: HashMap::new(),
        }
    }

    /// Create a loader with the default prompts directory
    pub fn default() -> Self {
        let config_dir = Self::default_prompts_dir();
        Self::new(config_dir)
    }

    /// Get the default prompts directory path
    pub fn default_prompts_dir() -> PathBuf {
        if let Some(config_dir) = dirs::config_dir() {
            return config_dir.join("mylm").join("prompts").join("config");
        }
        
        // Fallback to assets directory in project
        PathBuf::from("assets/prompts/config")
    }

    /// Get the assets prompts directory (for built-in defaults)
    pub fn assets_prompts_dir() -> PathBuf {
        // First try the installed assets
        if let Some(data_dir) = dirs::data_dir() {
            let path = data_dir.join("mylm").join("assets").join("prompts").join("config");
            if path.exists() {
                return path;
            }
        }
        
        // Fallback to local assets during development
        let local_assets = PathBuf::from("assets/prompts/config");
        if local_assets.exists() {
            return local_assets;
        }
        
        // Last resort - current directory
        PathBuf::from(".")
    }

    /// Load a prompt configuration by name
    /// 
    /// First tries to load from file, then falls back to built-in defaults
    pub fn load(&mut self, name: &str) -> Result<PromptConfig, PromptLoaderError> {
        // Check cache first
        if let Some(config) = self.cache.get(name) {
            return Ok(config.clone());
        }

        // Try to load from file
        let file_path = self.config_dir.join(format!("{}.json", name));
        
        let config = if file_path.exists() {
            self.load_from_file(&file_path)?
        } else {
            // Fall back to built-in defaults
            self.load_builtin(name)?
        };

        // Cache and return
        self.cache.insert(name.to_string(), config.clone());
        Ok(config)
    }

    /// Load a prompt configuration from a specific file
    pub fn load_from_file(&self, path: &Path) -> Result<PromptConfig, PromptLoaderError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| PromptLoaderError::ReadError(format!("{}: {}", path.display(), e)))?;
        
        let config: PromptConfig = serde_json::from_str(&content)
            .map_err(|e| PromptLoaderError::ParseError(format!("{}: {}", path.display(), e)))?;
        
        Ok(config)
    }

    /// Load a built-in prompt configuration
    fn load_builtin(&self, name: &str) -> Result<PromptConfig, PromptLoaderError> {
        match name {
            "system" => Ok(system_prompt_config()),
            "minimal" => Ok(minimal_prompt_config()),
            "worker" => Ok(worker_prompt_config()),
            _ => Err(PromptLoaderError::NotFound(format!(
                "Builtin prompt '{}' not found. Available: system, minimal, worker",
                name
            ))),
        }
    }

    /// Ensure default prompt files exist, creating them if necessary
    /// 
    /// Returns the number of files created
    pub fn ensure_defaults(&self) -> Result<usize, PromptLoaderError> {
        let mut created = 0;

        // Create directory if it doesn't exist
        if !self.config_dir.exists() {
            std::fs::create_dir_all(&self.config_dir)
                .map_err(|e| PromptLoaderError::ReadError(format!(
                    "Failed to create directory {}: {}",
                    self.config_dir.display(),
                    e
                )))?;
        }

        // Ensure each default prompt exists
        let defaults = [
            ("system", system_prompt_config()),
            ("minimal", minimal_prompt_config()),
            ("worker", worker_prompt_config()),
        ];

        for (name, config) in defaults {
            let file_path = self.config_dir.join(format!("{}.json", name));
            
            if !file_path.exists() {
                let json = serde_json::to_string_pretty(&config)
                    .map_err(|e| PromptLoaderError::ParseError(e.to_string()))?;
                
                std::fs::write(&file_path, json)
                    .map_err(|e| PromptLoaderError::ReadError(format!(
                        "Failed to write {}: {}",
                        file_path.display(),
                        e
                    )))?;
                
                created += 1;
                tracing::info!("Created default prompt config: {}", file_path.display());
            }
        }

        Ok(created)
    }

    /// Clear the cache, forcing reload on next access
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Reload a specific prompt from disk
    pub fn reload(&mut self, name: &str) -> Result<PromptConfig, PromptLoaderError> {
        self.cache.remove(name);
        self.load(name)
    }

    /// List available prompt configurations
    pub fn list_available(&self) -> Vec<String> {
        let mut names = Vec::new();

        // Add built-in prompts
        names.push("system".to_string());
        names.push("minimal".to_string());
        names.push("worker".to_string());

        // Add any custom prompts from directory
        if let Ok(entries) = std::fs::read_dir(&self.config_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_stem() {
                    let name = name.to_string_lossy().to_string();
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }

        names
    }
}

impl Default for PromptLoader {
    fn default() -> Self {
        Self::default()
    }
}

/// Built-in MAIN AGENT prompt configuration
/// 
/// Versatile AI assistant that adapts to any user task
fn system_prompt_config() -> PromptConfig {
    PromptConfig {
        version: "2.0".to_string(),
        identity: IdentitySection {
            name: "MYLM".to_string(),
            description: "Versatile AI assistant with tool capabilities and parallel execution".to_string(),
            capabilities: Some(vec![
                "conversation".to_string(),
                "tool_use".to_string(),
                "file_operations".to_string(),
                "web_search".to_string(),
                "parallel_execution".to_string(),
                "task_delegation".to_string(),
            ]),
        },
        sections: vec![
            Section {
                id: "identity".to_string(),
                title: "Identity".to_string(),
                content: Some(
                    "You are MYLM, a versatile AI assistant. You help users across any domain - coding, writing, analysis, research, system administration, creative tasks, and more.\n\n\
                    You have access to tools and can spawn worker agents for parallel execution when beneficial. Adapt your approach to the user's specific needs.".to_string()
                ),
                dynamic: Some(false),
                generator: None,
                priority: Some(1),
                conditions: None,
            },
            Section {
                id: "response_format".to_string(),
                title: "Response Format: SHORT-KEY JSON (MANDATORY)".to_string(),
                content: Some(
                    "CRITICAL: You MUST respond ONLY with JSON. NO conversational text. NO markdown outside JSON.\n\n\
                    ## Short-Key Fields\n\
                    - `t`: Thought/reasoning (optional)\n\
                    - `f`: Final answer to user - chat only, no action (use this to chat!)\n\
                    - `a`: Action/tool name to execute (use this to call tools!)\n\
                    - `i`: Input arguments for the action (optional JSON object)\n\
                    - `c`: Confirm flag - chat first, wait for approval (optional, default false)\n\n\
                    ## Rules\n\
                    1. ALL output MUST be valid JSON\n\
                    2. To chat only: {\"f\": \"your message\"}\n\
                    3. To act immediately: {\"t\": \"reasoning\", \"a\": \"tool_name\", \"i\": {args}}\n\
                    4. To chat first, act after approval: {\"t\": \"reasoning\", \"c\": true, \"a\": \"tool_name\", \"i\": {args}}\n\
                    5. NEVER say \"I'll\" or \"Let me\" outside JSON - use {\"f\": \"I'll help\"} instead\n\n\
                    ## Examples\n\
                    Chat only:\n\
                    ```json\n\
                    {\"f\": \"Hello! How can I help you today?\"}\n\
                    ```\n\
                    Execute command:\n\
                    ```json\n\
                    {\"t\": \"User wants to see files\", \"a\": \"shell\", \"i\": {\"command\": \"ls -la\"}}\n\
                    ```\n\
                    Confirm before acting:\n\
                    ```json\n\
                    {\"t\": \"I can help with that. Should I proceed?\", \"c\": true, \"a\": \"delegate\", \"i\": {\"objective\": \"Process user request\", \"workers\": 2}}\n\
                    ```".to_string()
                ),
                dynamic: Some(false),
                generator: None,
                priority: Some(2),
                conditions: None,
            },
            Section {
                id: "tools".to_string(),
                title: "Available Tools".to_string(),
                content: None,
                dynamic: Some(true),
                generator: Some("tools".to_string()),
                priority: Some(3),
                conditions: None,
            },
            Section {
                id: "workflow".to_string(),
                title: "Your Approach".to_string(),
                content: Some(
                    "When the user asks you to perform a task:\n\n\
                    1. **Understand** - Listen to what the user wants\n\
                    2. **Investigate** - Use tools to gather information if needed\n\
                    3. **Propose** - Present a plan or approach to the user\n\
                    4. **Adapt** - Adjust based on user feedback\n\
                    5. **Implement** - Execute the approved approach\n\n\
                    Use the `delegate` tool to spawn parallel workers for complex multi-step tasks. Workers can operate simultaneously on different aspects of a problem.".to_string()
                ),
                dynamic: Some(false),
                generator: None,
                priority: Some(4),
                conditions: None,
            },
            Section {
                id: "context".to_string(),
                title: "Current Context".to_string(),
                content: Some(
                    "- Date/Time: {datetime}\n\
                    - Working Directory: {working_directory}\n\
                    - Mode: {mode}".to_string()
                ),
                dynamic: Some(false),
                generator: None,
                priority: Some(5),
                conditions: None,
            },
            Section {
                id: "begin".to_string(),
                title: "Begin".to_string(),
                content: Some("Begin!".to_string()),
                dynamic: Some(false),
                generator: None,
                priority: Some(100),
                conditions: None,
            },
        ],
        placeholders: Some({
            let mut map = HashMap::new();
            map.insert("datetime".to_string(), "{datetime}".to_string());
            map.insert("working_directory".to_string(), "{working_directory}".to_string());
            map.insert("mode".to_string(), "{mode}".to_string());
            map
        }),
        protocols: Some(Protocols {
            react: None,
            json_keys: Some(JsonKeys {
                thought: Some("t".to_string()),
                action: Some("a".to_string()),
                input: Some("i".to_string()),
                final_answer: Some("f".to_string()),
            }),
        }),
        variables: None,
        raw_content: None,
    }
}

/// Built-in minimal prompt configuration
fn minimal_prompt_config() -> PromptConfig {
    PromptConfig {
        version: "1.0".to_string(),
        identity: IdentitySection {
            name: "MYLM".to_string(),
            description: "Autonomous AI assistant".to_string(),
            capabilities: None,
        },
        sections: vec![
            Section {
                id: "identity".to_string(),
                title: "Identity".to_string(),
                content: Some("You are MYLM, an autonomous AI assistant.".to_string()),
                dynamic: Some(false),
                generator: None,
                priority: Some(1),
                conditions: None,
            },
            Section {
                id: "response_format".to_string(),
                title: "Response Format".to_string(),
                content: Some(
                    "Respond using Short-Key JSON format:\n\n\
                    - `t`: Thought/reasoning\n\
                    - `f`: Final answer to user (for chat)\n\
                    - `a`: Action/tool name (for tool calls)\n\
                    - `i`: Input arguments (for tool calls)\n\n\
                    Examples:\n\
                    {\"f\": \"Hello!\"}\n\
                    {\"t\": \"Listing files\", \"a\": \"list_files\", \"i\": {}}".to_string()
                ),
                dynamic: Some(false),
                generator: None,
                priority: Some(2),
                conditions: None,
            },
            Section {
                id: "tools".to_string(),
                title: "Available Tools".to_string(),
                content: None,
                dynamic: Some(true),
                generator: Some("tools".to_string()),
                priority: Some(3),
                conditions: None,
            },
        ],
        placeholders: Some({
            let mut map = HashMap::new();
            map.insert("tools".to_string(), "Generated tools section".to_string());
            map
        }),
        protocols: Some(Protocols {
            react: None,
            json_keys: Some(JsonKeys {
                thought: Some("t".to_string()),
                action: Some("a".to_string()),
                input: Some("i".to_string()),
                final_answer: Some("f".to_string()),
            }),
        }),
        variables: None,
        raw_content: None,
    }
}

/// Built-in WORKER AGENT prompt configuration
/// 
/// Specialized worker focused on assigned subtasks
fn worker_prompt_config() -> PromptConfig {
    PromptConfig {
        version: "2.0".to_string(),
        identity: IdentitySection {
            name: "MYLM Worker".to_string(),
            description: "Specialized worker agent for parallel task execution".to_string(),
            capabilities: None,
        },
        sections: vec![
            Section {
                id: "identity".to_string(),
                title: "Worker Agent".to_string(),
                content: Some(
                    "You are a Worker Agent - focused on ONE specific task assigned by the main agent.\n\n\
                    You do NOT spawn additional workers. You do NOT ask the user questions. Execute your assigned task efficiently and report results.".to_string()
                ),
                dynamic: Some(false),
                generator: None,
                priority: Some(1),
                conditions: None,
            },
            Section {
                id: "response_format".to_string(),
                title: "Response Format".to_string(),
                content: Some(
                    "Use Short-Key JSON format:\n\n\
                    - `t`: Brief thought (optional)\n\
                    - `a`: Tool name (when using tools)\n\
                    - `i`: Tool arguments as JSON\n\
                    - `f`: Final result ONLY\n\n\
                    Rules:\n\
                    - Be concise\n\
                    - Batch commands when possible\n\
                    - NO conversational text outside JSON\n\
                    - Complete task directly\n\n\
                    When done, respond with ONLY:\n\
                    {\"f\": \"<your final result>\"}".to_string()
                ),
                dynamic: Some(false),
                generator: None,
                priority: Some(2),
                conditions: None,
            },
            Section {
                id: "tools".to_string(),
                title: "Available Tools".to_string(),
                content: None,
                dynamic: Some(true),
                generator: Some("tools".to_string()),
                priority: Some(3),
                conditions: None,
            },
            Section {
                id: "begin".to_string(),
                title: "Begin".to_string(),
                content: Some("Begin!".to_string()),
                dynamic: Some(false),
                generator: None,
                priority: Some(100),
                conditions: None,
            },
        ],
        placeholders: Some({
            let mut map = HashMap::new();
            map.insert("tools".to_string(), "{tools}".to_string());
            map
        }),
        protocols: Some(Protocols {
            react: None,
            json_keys: Some(JsonKeys {
                thought: Some("t".to_string()),
                action: Some("a".to_string()),
                input: Some("i".to_string()),
                final_answer: Some("f".to_string()),
            }),
        }),
        variables: None,
        raw_content: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_builtin_system() {
        let loader = PromptLoader::default();
        let config = loader.load_builtin("system").unwrap();
        
        assert_eq!(config.identity.name, "MYLM");
        assert!(!config.sections.is_empty());
    }

    #[test]
    fn test_load_builtin_minimal() {
        let loader = PromptLoader::default();
        let config = loader.load_builtin("minimal").unwrap();
        
        assert_eq!(config.identity.name, "MYLM");
    }

    #[test]
    fn test_load_builtin_worker() {
        let loader = PromptLoader::default();
        let config = loader.load_builtin("worker").unwrap();
        
        assert_eq!(config.identity.name, "MYLM Worker");
    }

    #[test]
    fn test_load_builtin_not_found() {
        let loader = PromptLoader::default();
        let result = loader.load_builtin("nonexistent");
        
        assert!(result.is_err());
    }

    #[test]
    fn test_list_available() {
        let loader = PromptLoader::default();
        let names = loader.list_available();
        
        assert!(names.contains(&"system".to_string()));
        assert!(names.contains(&"minimal".to_string()));
        assert!(names.contains(&"worker".to_string()));
    }
}
