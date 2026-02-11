use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{warn, error, info};

use serde_json;
use serde_yml;
use chrono;
use anyhow;
use thiserror::Error;

use super::config::PromptsConfig;
use crate::agent::Tool;
use crate::config::v2::prompt_schema::{PromptConfig, Section};
use crate::context::TerminalContext;

// ============================================================================
// Embedded Fallback Configs
// ============================================================================
// These are embedded in the binary so the application works without external
// config files (e.g., when installed system-wide to /usr/bin)

const EMBEDDED_SYSTEM_CONFIG: &str = include_str!("../../../../assets/prompts/config/system.json");
const EMBEDDED_WORKER_CONFIG: &str = include_str!("../../../../assets/prompts/config/worker.json");
const EMBEDDED_MEMORY_CONFIG: &str = include_str!("../../../../assets/prompts/config/memory.json");
const EMBEDDED_MINIMAL_CONFIG: &str = include_str!("../../../../assets/prompts/config/minimal.json");

fn get_embedded_config(name: &str) -> Option<&'static str> {
    match name {
        "default" | "system" => Some(EMBEDDED_SYSTEM_CONFIG),
        "worker" => Some(EMBEDDED_WORKER_CONFIG),
        "memory" => Some(EMBEDDED_MEMORY_CONFIG),
        "minimal" => Some(EMBEDDED_MINIMAL_CONFIG),
        _ => None,
    }
}

/// --- Dynamic Capabilities Generation ---
///
/// Generate a capabilities prompt dynamically from available tools.
///
/// This function creates a comprehensive capabilities description based on the actual
/// tools registered in the system, ensuring the prompt always reflects the current
/// tool set. Tools are organized by their `ToolKind` (Internal, Terminal, Web).
///
/// # Arguments
/// * `tools` - A slice of tool references to include in the capabilities
///
/// # Returns
/// A formatted markdown string describing all available tools and capabilities
pub fn generate_capabilities_prompt(tools: &[Arc<dyn Tool>]) -> String {
    if tools.is_empty() {
        return String::from(CAPABILITIES_PROMPT_MINIMAL);
    }

    let mut internal_tools = Vec::new();
    let mut terminal_tools = Vec::new();
    let mut web_tools = Vec::new();

    // Categorize tools by kind
    for tool in tools {
        match tool.kind() {
            crate::agent::tool::ToolKind::Internal => internal_tools.push(tool),
            crate::agent::tool::ToolKind::Terminal => terminal_tools.push(tool),
            crate::agent::tool::ToolKind::Web => web_tools.push(tool),
        }
    }

    let mut output = String::from("# YOUR CAPABILITIES\n\n");
    output.push_str("You are MYLM (My Local Model), an autonomous AI agent with access to tools and memory.\n\n");
    output.push_str("## Tools Available\n\n");

    // Memory & State tools (Internal)
    if !internal_tools.is_empty() {
        output.push_str("### Memory & State (USE FIRST)\n");
        for tool in &internal_tools {
            output.push_str(&format!("- `{}` - {}\n", tool.name(), tool.description()));
        }
        output.push('\n');
    }

    // Terminal & Execution tools
    if !terminal_tools.is_empty() {
        output.push_str("### Terminal & Execution\n");
        for tool in &terminal_tools {
            output.push_str(&format!("- `{}` - {}\n", tool.name(), tool.description()));
        }
        output.push('\n');
    }

    // Web & External tools
    if !web_tools.is_empty() {
        output.push_str("### Web & External\n");
        for tool in &web_tools {
            output.push_str(&format!("- `{}` - {}\n", tool.name(), tool.description()));
        }
        output.push('\n');
    }

    // Tool usage details
    output.push_str("## Tool Usage Details\n\n");
    for tool in tools {
        output.push_str(&format!("### `{}`\n", tool.name()));
        output.push_str(&format!("- **Description**: {}\n", tool.description()));
        output.push_str(&format!("- **Usage**: {}\n", tool.usage()));
        output.push('\n');
    }

    output.push_str("## Operational Workflow\n");
    output.push_str("1. **Recall**: Check `memory` for past lessons.\n");
    output.push_str("2. **Search**: Use `grep` to find relevant code.\n");
    output.push_str("3. **Plan**: Use `scratchpad` to outline steps.\n");
    output.push_str("4. **Act**: Execute tools.\n");
    output.push_str("5. **Record**: Save new insights to `memory`.\n\n");

    // Add background jobs section if delegate tool is available
    if internal_tools.iter().any(|t| t.name() == "delegate") {
        output.push_str("## Background Jobs (Parallel Execution)\n");
        output.push_str("Use the `delegate` tool to spawn parallel workers for subtasks:\n");
        output.push_str("- Workers run in the background automatically\n");
        output.push_str("- Continue with YOUR own work while they run\n");
        output.push_str("- The system will notify you when workers complete\n");
        output.push_str("- DO NOT try to manage workers via terminal commands\n");
        output.push_str("- DO NOT wait idle for workers - do other useful work\n");
        output.push_str("- Use `list_jobs` tool to check worker status anytime\n");
    }

    output
}

const CAPABILITIES_PROMPT_MINIMAL: &str = r#"# YOUR CAPABILITIES

You are MYLM (My Local Model), an autonomous AI agent.

No tools are currently available. Respond to the user with your knowledge only.
"#;

// ============================================================================
// Config Loading
// ============================================================================

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yml::Error),
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Unknown generator: {0}")]
    UnknownGenerator(String),
    #[error("Config not found for name: {0}")]
    ConfigNotFound(String),
}

// Note: PromptError::MissingField constructor used directly via PromptError::MissingField(s)
// Keeping the enum variant for explicit error construction

pub type PromptResult<T> = Result<T, PromptError>;

fn get_user_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|mut p| {
        p.push("mylm");
        p.push("prompts");
        p
    })
}

fn get_project_prompts_dir() -> PathBuf {
    PathBuf::from("assets").join("prompts")
}

#[derive(Debug, Clone, Copy)]
enum ConfigFormat {
    Json,
    Yaml,
    Markdown,
}

impl ConfigFormat {
    fn from_extension(ext: &str) -> Self {
        match ext {
            "json" => Self::Json,
            "yaml" | "yml" => Self::Yaml,
            _ => Self::Json,
        }
    }
}

fn find_config_file(name: &str) -> PromptResult<Option<(PathBuf, ConfigFormat)>> {
    let config_dir_name = "config";
    let extensions = ["json", "yaml", "yml"];
    
    // Check user config directory: ~/.config/mylm/prompts/config/
    if let Some(mut user_config_dir) = get_user_config_dir() {
        user_config_dir.push(config_dir_name);
        if user_config_dir.exists() {
            for ext in &extensions {
                let path = user_config_dir.join(format!("{}.{}", name, ext));
                if path.exists() {
                    info!("Found config at {:?}", path);
                    return Ok(Some((path, ConfigFormat::from_extension(ext))));
                }
            }
        }
    }
    
    // Check project config directory: ./assets/prompts/config/
    let mut project_config_dir = get_project_prompts_dir();
    project_config_dir.push(config_dir_name);
    if project_config_dir.exists() {
        for ext in &extensions {
            let path = project_config_dir.join(format!("{}.{}", name, ext));
            if path.exists() {
                info!("Found config at {:?}", path);
                return Ok(Some((path, ConfigFormat::from_extension(ext))));
            }
        }
    }
    
    // Check for .md files (backward compatibility)
    let md_locations = [
        get_user_config_dir().map(|mut p| { p.push(format!("{}.md", name)); p }),
        Some(get_project_prompts_dir().join(format!("{}.md", name))),
    ];
    
    for path_opt in md_locations.iter().flatten() {
        if path_opt.exists() {
            info!("Found legacy .md prompt at {:?}", path_opt);
            return Ok(Some((path_opt.clone(), ConfigFormat::Markdown)));
        }
    }
    
    Ok(None)
}

/// Ensure user config directory exists, creating it if necessary
fn ensure_user_config_dir() -> Option<PathBuf> {
    if let Some(config_dir) = get_user_config_dir() {
        let config_subdir = config_dir.join("config");
        if !config_subdir.exists() {
            if let Err(e) = fs::create_dir_all(&config_subdir) {
                error!("Failed to create user config directory {:?}: {}", config_subdir, e);
                return None;
            }
            info!("Created user config directory: {:?}", config_subdir);
        }
        Some(config_subdir)
    } else {
        None
    }
}

pub fn load_config(name: &str) -> PromptResult<PromptConfig> {
    // First, try to find existing config file
    if let Some((path, format)) = find_config_file(name)? {
        return load_config_from_file(&path, format, name);
    }
    
    // Config not found - try to create it from embedded fallback
    if let Some(embedded) = get_embedded_config(name) {
        info!("Config '{}' not found, creating from embedded template", name);
        
        // Ensure user config directory exists
        if let Some(user_config_dir) = ensure_user_config_dir() {
            let target_path = user_config_dir.join(format!("{}.json", name));
            
            // Write embedded config to user config directory
            match fs::write(&target_path, embedded) {
                Ok(_) => {
                    info!("Created config '{}' at {:?}", name, target_path);
                    // Now load it from the file we just created
                    return load_config_from_file(&target_path, ConfigFormat::Json, name);
                }
                Err(e) => {
                    error!("Failed to write config '{}' to {:?}: {}", name, target_path, e);
                    // Fall through to loading directly from embedded
                }
            }
        }
        
        // If we couldn't write to file, load directly from embedded
        info!("Loading embedded config for '{}' directly", name);
        let config: PromptConfig = serde_json::from_str(embedded)
            .map_err(|e| {
                error!("Failed to parse embedded config '{}': {}", name, e);
                PromptError::Json(e)
            })?;
        return Ok(config);
    }
    
    // No fallback available
    error!("Config '{}' not found in any config directory (checked ~/.config/mylm/prompts/config/ and assets/prompts/config/) and no embedded fallback available", name);
    Err(PromptError::ConfigNotFound(format!("{} (no embedded fallback available)", name)))
}

fn load_config_from_file(path: &Path, format: ConfigFormat, name: &str) -> PromptResult<PromptConfig> {
    match format {
        ConfigFormat::Markdown => {
            let content = fs::read_to_string(path)
                .map_err(|e| {
                    error!("Failed to read .md file at {:?}: {}", path, e);
                    PromptError::Io(e)
                })?;
            // For markdown files, create a minimal config with raw_content
            let config = PromptConfig {
                version: "1.0".to_string(),
                identity: super::prompt_schema::IdentitySection {
                    name: "MYLM".to_string(),
                    description: "Autonomous AI assistant".to_string(),
                    capabilities: None,
                },
                sections: vec![],
                placeholders: None,
                protocols: None,
                variables: None,
                raw_content: Some(content),
            };
            info!("Loaded .md config for '{}' from {:?}", name, path);
            Ok(config)
        }
        ConfigFormat::Json => {
            let content = fs::read_to_string(path)
                .map_err(|e| {
                    error!("Failed to read JSON config at {:?}: {}", path, e);
                    PromptError::Io(e)
                })?;
            let config: PromptConfig = serde_json::from_str(&content)
                .map_err(|e| {
                    error!("Failed to parse JSON config at {:?}: {}", path, e);
                    PromptError::Json(e)
                })?;
            info!("Loaded config '{}' from JSON: {:?}", name, path);
            Ok(config)
        }
        ConfigFormat::Yaml => {
            let content = fs::read_to_string(path)
                .map_err(|e| {
                    error!("Failed to read YAML config at {:?}: {}", path, e);
                    PromptError::Io(e)
                })?;
            let config: PromptConfig = serde_yml::from_str(&content)
                .map_err(|e| {
                    error!("Failed to parse YAML config at {:?}: {}", path, e);
                    PromptError::Yaml(e)
                })?;
            info!("Loaded config '{}' from YAML: {:?}", name, path);
            Ok(config)
        }
    }
}

// ============================================================================
// Renderer
// ============================================================================

pub fn render_config(
    config: &PromptConfig,
    tools: Option<&[Arc<dyn Tool>]>,
    scratchpad: Option<&str>,
    context: &TerminalContext,
) -> PromptResult<String> {
    // Sort sections by priority (default 100)
    let mut sections: Vec<&Section> = config.sections.iter().collect();
    sections.sort_by_key(|s| s.priority.unwrap_or(100));
    
    let mut rendered_parts = Vec::new();
    
    for section in &sections {
        let content: String;
        
        if let Some(dynamic) = section.dynamic {
            if dynamic {
                match section.generator.as_deref() {
                    Some("tools") => {
                        if let Some(tools_slice) = tools {
                            content = generate_capabilities_prompt(tools_slice);
                        } else {
                            warn!("Section '{}' requires tools but none provided, skipping", section.id);
                            continue;
                        }
                    }
                    Some("scratchpad") => {
                        let pad = scratchpad.unwrap_or("(Empty - will be populated during session)");
                        content = if pad.trim().is_empty() {
                            "(Empty - will be populated during session)".to_string()
                        } else {
                            pad.to_string()
                        };
                    }
                    Some(unknown) => {
                        warn!("Unknown generator '{}' in section '{}', skipping", unknown, section.id);
                        continue;
                    }
                    None => {
                        warn!("Section '{}' has dynamic=true but no generator, skipping", section.id);
                        continue;
                    }
                }
            } else if let Some(c) = &section.content {
                content = c.clone();
            } else {
                warn!("Section '{}' has no content and is not dynamic, skipping", section.id);
                continue;
            }
        } else if let Some(c) = &section.content {
            content = c.clone();
        } else {
            warn!("Section '{}' has no content, skipping", section.id);
            continue;
        }
        
        rendered_parts.push(format!("## {}\n\n{}", section.title, content));
    }
    
    let mut final_prompt = rendered_parts.join("\n\n---\n\n");
    
    // Placeholder substitution
    let datetime = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let working_directory = context.cwd()
        .as_ref()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let git_branch = context.git_branch()
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let mode = "default"; // default mode
    
    let user_instructions = config.placeholders.as_ref()
        .and_then(|m| m.get("user_instructions"))
        .cloned()
        .unwrap_or_else(|| "".to_string());
    
    // Replace placeholders
    final_prompt = final_prompt
        .replace("{datetime}", &datetime)
        .replace("{working_directory}", &working_directory)
        .replace("{git_branch}", &git_branch)
        .replace("{mode}", mode)
        .replace("{user_instructions}", &user_instructions)
        .replace("{tools}", tools.map(|t| generate_capabilities_prompt(t)).as_deref().unwrap_or(""))
        .replace("{scratchpad}", scratchpad.unwrap_or("(Empty - will be populated during session)"));
    
    Ok(final_prompt)
}

// ============================================================================
// Public API
// ============================================================================

/// Build the system prompt
///
/// Uses the configured system_prompt name from PromptsConfig, or "default" if not set.
pub async fn build_system_prompt(
    ctx: &crate::context::TerminalContext,
    prompt_name: &str,
    _mode_hint: Option<&str>,
    prompts_config: Option<&PromptsConfig>,
    tools: Option<&[Arc<dyn Tool>]>,
    scratchpad: Option<&str>,
) -> anyhow::Result<String> {
    // Use config from prompts_config if available, otherwise use the passed prompt_name
    let effective_name = prompts_config
        .and_then(|c| c.system_prompt.as_deref())
        .unwrap_or(prompt_name);
    let config = load_config(effective_name)?;
    if let Some(raw) = config.raw_content {
        return Ok(raw);
    }
    render_config(&config, tools, scratchpad, ctx)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Build the worker prompt
///
/// Uses the configured worker_prompt name from PromptsConfig, or "worker" if not set.
pub async fn build_worker_prompt(prompts_config: Option<&PromptsConfig>) -> PromptResult<String> {
    let prompt_name = prompts_config
        .and_then(|c| c.worker_prompt.as_deref())
        .unwrap_or("worker");
    let config = load_config(prompt_name)?;
    if let Some(raw) = config.raw_content {
        return Ok(raw);
    }
    let empty_context = TerminalContext::new();
    render_config(&config, None, None, &empty_context)
}

/// Build the memory prompt
///
/// Uses the configured memory_prompt name from PromptsConfig, or "memory" if not set.
pub async fn build_memory_prompt(prompts_config: Option<&PromptsConfig>) -> PromptResult<String> {
    let prompt_name = prompts_config
        .and_then(|c| c.memory_prompt.as_deref())
        .unwrap_or("memory");
    let config = load_config(prompt_name)?;
    if let Some(raw) = config.raw_content {
        return Ok(raw);
    }
    let empty_context = TerminalContext::new();
    render_config(&config, None, None, &empty_context)
}

/// Build system prompt with capabilities (uses configured prompt name)
pub async fn build_system_prompt_with_capabilities(
    ctx: &TerminalContext,
    _prompt_name: &str,
    _mode_hint: Option<&str>,
    prompts_config: Option<&PromptsConfig>,
    tools: Option<&[Arc<dyn Tool>]>,
    scratchpad: Option<&str>,
) -> anyhow::Result<String> {
    let prompt_name = prompts_config
        .and_then(|c| c.system_prompt.as_deref())
        .unwrap_or("default");
    let config = load_config(prompt_name)?;
    if let Some(raw) = config.raw_content {
        return Ok(raw);
    }
    render_config(&config, tools, scratchpad, ctx)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Get identity prompt (legacy)
pub fn get_identity_prompt() -> &'static str {
    "Identity prompt is now loaded from config. Use build_system_prompt()."
}

/// Get memory protocol (legacy)
pub fn get_memory_protocol() -> &'static str {
    "Memory protocol is now loaded from config. Use build_memory_prompt()."
}

/// Get react protocol (legacy)
pub fn get_react_protocol() -> &'static str {
    "ReAct protocol is now loaded from config. Check the protocols section of your prompt config."
}

// ============================================================================
// Legacy compatibility functions
// ============================================================================

pub fn get_prompts_dir() -> PathBuf {
    if let Some(ref p) = get_user_config_dir() {
        if p.exists() {
            return p.clone();
        }
    }
    get_project_prompts_dir()
}

pub fn get_user_prompts_dir() -> PathBuf {
    get_user_config_dir().unwrap_or_else(|| get_project_prompts_dir())
}

pub fn install_default_prompts() -> PromptResult<()> {
    // No-op for compatibility
    Ok(())
}

pub async fn load_prompt(name: &str) -> PromptResult<String> {
    let config = load_config(name)?;
    if let Some(raw) = config.raw_content {
        return Ok(raw);
    }
    let empty_context = TerminalContext::new();
    render_config(&config, None, None, &empty_context)
}

pub async fn load_prompt_from_path(path: &Path) -> PromptResult<String> {
    let content = fs::read_to_string(path)
        .map_err(|e| {
            error!("Failed to read prompt from {:?}: {}", path, e);
            PromptError::Io(e)
        })?;
    Ok(content)
}
