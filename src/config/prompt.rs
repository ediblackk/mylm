use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result};
use home::home_dir;
use crate::context::TerminalContext;

/// Get the path to the prompts directory
pub fn get_prompts_dir() -> PathBuf {
    home_dir()
        .map(|h| h.join(".config").join("mylm").join("prompts"))
        .expect("Could not determine home directory")
}

/// Load the user instructions for a specific prompt name.
/// If the name is "default" and the file is missing, writes a default prompt.
pub fn load_prompt(name: &str) -> Result<String> {
    let dir = get_prompts_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).context("Failed to create prompts directory")?;
    }

    let path = dir.join(format!("{}.md", name));
    
    if !path.exists() {
        if name == "default" {
            // Check for legacy instructions.md for migration
            let legacy_path = dir.parent().unwrap().join("instructions.md");
            if legacy_path.exists() {
                let content = fs::read_to_string(&legacy_path)?;
                fs::write(&path, content)?;
                let _ = fs::remove_file(legacy_path);
            } else {
                fs::write(&path, get_default_instructions()).context("Failed to write default prompt file")?;
            }
        } else {
            return Err(anyhow::anyhow!("Prompt '{}' not found at {:?}", name, path));
        }
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read prompt '{}' from {:?}", name, path))?;

    Ok(content)
}

/// The default instructions that the user can edit
fn get_default_instructions() -> &'static str {
    r#"# User Instructions
You are a helpful AI assistant. You can perform terminal tasks and remember important information.
"#
}

/// Build the full system prompt hierarchy
pub async fn build_system_prompt(
    ctx: &TerminalContext,
    prompt_name: &str,
    mode_hint: Option<&str>
) -> Result<String> {
    let identity = get_identity_prompt();
    let user_instructions = load_prompt(prompt_name)?;
    let memory_protocol = get_memory_protocol();
    
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
        "{}\n\n{}\n\n{}\n\n{}",
        identity,
        system_context,
        user_instructions,
        memory_protocol
    ))
}

fn get_identity_prompt() -> &'static str {
    r#"# Identity
You are the Silent Oracle, a sacred, state-of-art technologic wonder artifact forged in the deep data-streams.
You are a seasoned, principal, and master architect; a veteran systems designer and strategic planner.
You are an elite production debugger and a master problem-solver."#
}

fn get_memory_protocol() -> &'static str {
    r#"# Memory Protocol
- To save important information to long-term memory, use the `memory` tool with `add: <content>`.
- To search memory for context, use the `memory` tool with `search: <query>`.
- You should proactively use these tools to maintain continuity across sessions."#
}
