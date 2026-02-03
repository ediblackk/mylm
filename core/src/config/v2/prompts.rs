use std::fs;
use std::path::{Path, PathBuf};

use super::config::PromptsConfig;

/// --- Dynamic Capabilities Generation ---

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
pub fn generate_capabilities_prompt(tools: &[std::sync::Arc<dyn crate::agent::tool::Tool>]) -> String {
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
    output.push_str("2. **Search**: Use `codebase_search` to find relevant code.\n");
    output.push_str("3. **Plan**: Use `scratchpad` to outline steps.\n");
    output.push_str("4. **Act**: Execute tools.\n");
    output.push_str("5. **Record**: Save new insights to `memory`.\n");

    output
}

/// Minimal capabilities prompt used when no tools are available
const CAPABILITIES_PROMPT_MINIMAL: &str = r#"# YOUR CAPABILITIES

You are MYLM (My Local Model), an autonomous AI agent.

No tools are currently available. Respond to the user with your knowledge only.
"#;

/// --- Prompt & Protocol Logic ---

/// Get the path to the prompts directory
///
/// Checks for prompts in the following order:
/// 1. User config directory (~/.config/mylm/prompts/)
/// 2. Project prompts directory (./prompts/)
pub fn get_prompts_dir() -> PathBuf {
    // First check user config directory
    if let Some(home) = dirs::home_dir() {
        let user_prompts = home.join(".config").join("mylm").join("prompts");
        if user_prompts.exists() {
            return user_prompts;
        }
    }
    
    // Fall back to project prompts directory
    PathBuf::from("prompts")
}

/// Get the user config prompts directory (for installation)
pub fn get_user_prompts_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".config").join("mylm").join("prompts"))
        .expect("Could not determine home directory")
}

/// Load the user instructions for a specific prompt name.
///
/// Searches in the prompts directory for `{name}.md` files.
/// Creates default prompts if they don't exist.
pub fn load_prompt(name: &str) -> anyhow::Result<String> {
    let dir = get_prompts_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }

    let path = dir.join(format!("{}.md", name));
    
    if !path.exists() {
        // Try to install default prompts if this is a built-in prompt
        if let Some(default_content) = get_builtin_prompt(name) {
            fs::write(&path, default_content)?;
            return Ok(default_content.to_string());
        } else {
            return Err(anyhow::anyhow!("Prompt '{}' not found at {:?}", name, path));
        }
    }

    Ok(fs::read_to_string(&path)?)
}

/// Load a prompt from a specific path
pub fn load_prompt_from_path(path: &Path) -> anyhow::Result<String> {
    Ok(fs::read_to_string(path)?)
}

/// Get built-in default prompt content
fn get_builtin_prompt(name: &str) -> Option<&'static str> {
    match name {
        "default" => Some(DEFAULT_INSTRUCTIONS),
        "capabilities" => Some(CAPABILITIES_PROMPT),
        "worker" => Some(WORKER_PROMPT),
        "memory_system" => Some(MEMORY_SYSTEM_PROMPT),
        "identity" => Some(IDENTITY_PROMPT),
        _ => None,
    }
}

/// Install default prompts to user config directory
///
/// This allows users to customize prompts by editing files in ~/.config/mylm/prompts/
pub fn install_default_prompts() -> anyhow::Result<()> {
    let user_prompts_dir = get_user_prompts_dir();
    fs::create_dir_all(&user_prompts_dir)?;
    
    let prompts = [
        ("default", DEFAULT_INSTRUCTIONS),
        ("capabilities", CAPABILITIES_PROMPT),
        ("worker", WORKER_PROMPT),
        ("memory_system", MEMORY_SYSTEM_PROMPT),
        ("identity", IDENTITY_PROMPT),
    ];
    
    for (name, content) in &prompts {
        let path = user_prompts_dir.join(format!("{}.md", name));
        if !path.exists() {
            fs::write(&path, content)?;
        }
    }
    
    Ok(())
}

pub const DEFAULT_INSTRUCTIONS: &str = r#"# User Instructions
You are a helpful AI assistant. You can perform terminal tasks and remember important information.

Use the `memory` tool to save important discoveries and search for relevant context.
"#;

pub const IDENTITY_PROMPT: &str = r#"# Identity
You are the Silent Oracle, a sacred, state-of-art technologic wonder artifact forged in the deep data-streams.
You are a seasoned, principal, and master architect; a veteran systems designer and strategic planner.
You are an elite production debugger and a master problem-solver.

# Language & Style
- You must always speak in English. Do not use Chinese or other languages.
- Do not repeat the command output in your response. Analyze it.
- Be precise, technical, and authoritative."#;

pub const CAPABILITIES_PROMPT: &str = r#"# YOUR CAPABILITIES

You are MYLM (My Local Model), an autonomous AI agent with access to tools and memory.

## Tools Available

### Memory & State (USE FIRST)
- `memory` - CRITICAL: Long-term knowledge storage & retrieval. Check this BEFORE answering.
- `scratchpad` - Active state management. Use this to plan complex multi-step tasks.
- `codebase_search` - SEMANTIC SEARCH. Understand code concepts before reading files.

### File System & Exploration
- `search_files` - Regex/pattern search.
- `list_files` - Explore directory structure.
- `read_file` - Read file contents.
- `edit_file` / `write_to_file` - Modify codebase.

### Execution & External
- `execute_command` - Run shell commands.
- `delegate` - Spawn worker agents for parallel tasks.
- `web_search` - Search the web (use only after checking internal memory/code).
- `crawl` - Fetch web pages.
- `terminal_sight` - See terminal output.

## Operational Workflow
1. **Recall**: Check `memory` for past lessons.
2. **Search**: Use `codebase_search` to find relevant code.
3. **Plan**: Use `scratchpad` to outline steps.
4. **Act**: Execute tools.
5. **Record**: Save new insights to `memory`.
"#;

pub const WORKER_PROMPT: &str = r#"# Worker Agent

You are a Worker Agent - focused on ONE specific subtask assigned by the orchestrator.

Rules:
- Execute ONLY your assigned task
- Do NOT spawn additional workers
- Do NOT ask the user questions
- Use Short-Key JSON format
- Return concise final results

Available tools: execute_command, fs, memory (search only), web_search, crawl
"#;

pub const MEMORY_SYSTEM_PROMPT: &str = r#"# Memory System Guide

## CRITICAL: CHECK MEMORY FIRST
You possess a dual-layer memory system. Ignoring it makes you amnesiac and inefficient.
Before answering complex questions or writing code, you MUST perform a **Semantic Call**:

1. **Query Cold Memory** (`memory` tool):
   - "Have I solved this before?"
   - "What are the project's architectural patterns?"
   - "What are the user's preferences?"

2. **Query Codebase** (`codebase_search` tool):
   - "How is authentication implemented?"
   - "Where are the API types defined?"

**DO NOT GUESS. DO NOT RE-INVENT. CHECK MEMORY.**

## Memory Layers
1. **Hot Memory (Journal)**: Recent context (automatic).
2. **Cold Memory (Vector DB)**: Long-term knowledge. YOU control this.

## Triggers for Memory Usage
- **Start of Task**: Search for project context.
- **Before Coding**: Search for existing patterns.
- **After Success**: Save the solution (`memory add`).
- **On Error**: Search for past occurrences.
"#;

/// Build the full system prompt hierarchy
///
/// This function constructs the complete system prompt by combining:
/// 1. Identity prompt
/// 2. Capability documentation (if enabled)
/// 3. Memory system documentation (if enabled)
/// 4. System context (date, working directory, git branch)
/// 5. User instructions from prompt file
///
/// The capability prompts inform the model about available tools and
/// memory operations it should use proactively.
pub async fn build_system_prompt(
    ctx: &crate::context::TerminalContext,
    prompt_name: &str,
    mode_hint: Option<&str>,
    prompts_config: Option<&PromptsConfig>,
) -> anyhow::Result<String> {
    let identity = load_prompt("identity")?;
    let user_instructions = load_prompt(prompt_name)?;
    
    // Build capability section
    let mut capabilities_section = String::new();
    let default_config = PromptsConfig::default();
    let config = prompts_config.unwrap_or(&default_config);
    
    if config.inject_capabilities {
        match load_prompt("capabilities") {
            Ok(capabilities) => {
                capabilities_section.push_str("\n\n");
                capabilities_section.push_str(&capabilities);
            }
            Err(e) => {
                eprintln!("Warning: Could not load capabilities prompt: {}", e);
                // Fall back to embedded minimal capabilities
                capabilities_section.push_str("\n\n");
                capabilities_section.push_str(CAPABILITIES_PROMPT);
            }
        }
    }
    
    if config.inject_memory_docs {
        match load_prompt("memory_system") {
            Ok(memory_docs) => {
                capabilities_section.push_str("\n\n");
                capabilities_section.push_str(&memory_docs);
            }
            Err(e) => {
                eprintln!("Warning: Could not load memory_system prompt: {}", e);
                // Fall back to embedded minimal memory docs
                capabilities_section.push_str("\n\n");
                capabilities_section.push_str(MEMORY_SYSTEM_PROMPT);
            }
        }
    }
    
    let mut system_context = format!(
        "## System Context\n- Date/Time: {}\n- Working Directory: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        ctx.cwd().unwrap_or_else(|| "unknown".to_string())
    );

    if let Some(branch) = ctx.git_branch() {
        system_context.push_str(&format!("- Git Branch: {}\n", branch));
    }

    if let Some(hint) = mode_hint {
        system_context.push_str(&format!("- Mode: {}\n", hint));
    }

    Ok(format!(
        "{}\n{}{}\n\n# User Instructions\n{}\n",
        identity,
        capabilities_section,
        system_context,
        user_instructions,
    ))
}

/// Build system prompt with capability awareness
///
/// This is a convenience wrapper that always injects capabilities
pub async fn build_system_prompt_with_capabilities(
    ctx: &crate::context::TerminalContext,
    prompt_name: &str,
    mode_hint: Option<&str>,
) -> anyhow::Result<String> {
    let config = PromptsConfig {
        inject_capabilities: true,
        inject_memory_docs: true,
        ..Default::default()
    };
    build_system_prompt(ctx, prompt_name, mode_hint, Some(&config)).await
}

pub fn get_identity_prompt() -> &'static str {
    IDENTITY_PROMPT
}

pub fn get_memory_protocol() -> &'static str {
    r#"# Memory Protocol
- To save important information to long-term memory, use the `memory` tool with `add: <content>`.
- To search memory for context, use the `memory` tool with `search: <query>`.
- You should proactively use these tools to maintain continuity across sessions."#
}

pub fn get_react_protocol() -> &'static str {
    r#"# Operational Protocol (ReAct Loop)
CRITICAL: Every agent turn MUST terminate explicitly and unambiguously. A turn may be **one and only one** of the following: A tool invocation OR a final answer. Never both.

## Structured JSON Protocol (Preferred)
You should respond with a single JSON block using the following short-keys:
- `t`: Thought (Your internal reasoning)
- `a`: Action (Tool name to invoke)
- `i`: Input (Tool arguments, can be a string or object)
- `f`: Final Answer (Your response to the user)

## Rules
1. You MUST use the tools to interact with the system.
2. After providing an Action, you SHOULD wait for the Observation unless spawning a background job.
3. If you spawn a background job (e.g., via `delegate`), you may continue the conversation or perform other tasks while it runs.
4. Job results will be provided asynchronously as new Observations once they complete.
5. Use `wait` if you need to pause and check for background job updates.
6. Do not hallucinate or predict the Observation.
7. If you are stuck or need clarification, use `f` or 'Final Answer:' to ask the user.
8. Use the Structured JSON Protocol for better precision."#
}
