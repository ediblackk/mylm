//! Prompt settings UI - customize system, worker, and memory prompts

use anyhow::Result;
use console::Style;
use inquire::{Select as InquireSelect, Text};
use mylm_core::config::Config;

/// Prompt type selection for customization
#[derive(Debug, Clone, PartialEq)]
pub enum PromptMenuChoice {
    SystemPrompt,
    WorkerPrompt,
    MemoryPrompt,
    ViewCurrentPrompts,
    Back,
}

impl std::fmt::Display for PromptMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptMenuChoice::SystemPrompt => write!(f, "🖥️  [1] System Prompt"),
            PromptMenuChoice::WorkerPrompt => write!(f, "⚡ [2] Worker Prompt"),
            PromptMenuChoice::MemoryPrompt => write!(f, "🧠 [3] Memory Prompt"),
            PromptMenuChoice::ViewCurrentPrompts => write!(f, "👁️  [4] View Current Configuration"),
            PromptMenuChoice::Back => write!(f, "⬅️  [5] Back"),
        }
    }
}

/// Show the prompt settings menu
pub fn show_prompt_settings_menu(config: &Config) -> Result<PromptMenuChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let prompts = &config.features.prompts;

    println!("\n📝 Prompt Settings");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("  Current Configuration:");
    println!(
        "    System Prompt:  {}",
        prompts.system_prompt.as_deref().unwrap_or("default (embedded)")
    );
    println!(
        "    Worker Prompt:  {}",
        prompts.worker_prompt.as_deref().unwrap_or("worker (embedded)")
    );
    println!(
        "    Memory Prompt:  {}",
        prompts.memory_prompt.as_deref().unwrap_or("memory (embedded)")
    );
    println!();
    println!("  Prompt files are loaded from:");
    println!("    ~/.config/mylm/prompts/config/<name>.json");
    println!();
    println!("  Or use CLI commands to edit:");
    println!("    ai prompts edit system");
    println!("    ai prompts edit worker");
    println!("    ai prompts edit memory");
    println!();

    let options = vec![
        PromptMenuChoice::SystemPrompt,
        PromptMenuChoice::WorkerPrompt,
        PromptMenuChoice::MemoryPrompt,
        PromptMenuChoice::ViewCurrentPrompts,
        PromptMenuChoice::Back,
    ];

    let ans: Result<PromptMenuChoice, _> =
        InquireSelect::new("Select prompt to configure:", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(PromptMenuChoice::Back),
    }
}

/// Handle setting system prompt config name
pub fn handle_system_prompt(config: &mut Config) -> Result<bool> {
    let current = config
        .features
        .prompts
        .system_prompt
        .as_deref()
        .unwrap_or("default");

    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n🖥️  System Prompt Configuration");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("The system prompt defines the AI's identity and capabilities.");
    println!();
    println!("Current: {}", current);
    println!();
    println!("Options:");
    println!("  [1] Use default embedded prompt");
    println!("  [2] Use custom prompt config");
    println!("  [3] Edit prompt file in external editor");
    println!("  [4] Back");
    println!();

    let choice = InquireSelect::new(
        "Select option:",
        vec![
            "Use default",
            "Set custom config name",
            "Edit in external editor",
            "Back",
        ],
    )
    .prompt()?;

    match choice {
        "Use default" => {
            config.features.prompts.system_prompt = None;
            config.save_to_default_location()?;
            println!("✅ System prompt set to default (embedded)");
            Ok(true)
        }
        "Set custom config name" => {
            let input = Text::new("Enter prompt config name:")
                .with_help_message("Name of JSON config file in ~/.config/mylm/prompts/config/")
                .with_initial_value(current)
                .prompt()?;

            if input.trim().is_empty() {
                config.features.prompts.system_prompt = None;
                println!("✅ System prompt set to default");
            } else {
                config.features.prompts.system_prompt = Some(input.trim().to_string());
                println!("✅ System prompt set to: {}", input.trim());
            }
            config.save_to_default_location()?;
            Ok(true)
        }
        "Edit in external editor" => {
            // Launch external editor via CLI command
            println!("Launching external editor...");
            println!("Use: ai prompts edit system");
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// Handle setting worker prompt config name
pub fn handle_worker_prompt(config: &mut Config) -> Result<bool> {
    let current = config
        .features
        .prompts
        .worker_prompt
        .as_deref()
        .unwrap_or("worker");

    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n⚡ Worker Prompt Configuration");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("The worker prompt is used for sub-agent task execution.");
    println!();
    println!("Current: {}", current);
    println!();

    let input = Text::new("Enter worker prompt config name:")
        .with_help_message("Name of JSON config file (default: worker)")
        .with_initial_value(current)
        .prompt()?;

    if input.trim().is_empty() || input.trim() == "worker" {
        config.features.prompts.worker_prompt = None;
        println!("✅ Worker prompt set to default (worker)");
    } else {
        config.features.prompts.worker_prompt = Some(input.trim().to_string());
        println!("✅ Worker prompt set to: {}", input.trim());
    }
    config.save_to_default_location()?;
    Ok(true)
}

/// Handle setting memory prompt config name
pub fn handle_memory_prompt(config: &mut Config) -> Result<bool> {
    let current = config
        .features
        .prompts
        .memory_prompt
        .as_deref()
        .unwrap_or("memory");

    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n🧠 Memory Prompt Configuration");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("The memory prompt is used for memory/RAG operations.");
    println!();
    println!("Current: {}", current);
    println!();

    let input = Text::new("Enter memory prompt config name:")
        .with_help_message("Name of JSON config file (default: memory)")
        .with_initial_value(current)
        .prompt()?;

    if input.trim().is_empty() || input.trim() == "memory" {
        config.features.prompts.memory_prompt = None;
        println!("✅ Memory prompt set to default (memory)");
    } else {
        config.features.prompts.memory_prompt = Some(input.trim().to_string());
        println!("✅ Memory prompt set to: {}", input.trim());
    }
    config.save_to_default_location()?;
    Ok(true)
}

/// Display current prompt configuration details
pub fn view_current_prompts(config: &Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let prompts = &config.features.prompts;

    println!("\n👁️  Current Prompt Configuration");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(60))
    );

    // System prompt
    println!("\n🖥️  System Prompt:");
    match &prompts.system_prompt {
        Some(name) => {
            println!("  Config: {}", name);
            check_prompt_file(name);
        }
        None => {
            println!("  Config: default (embedded)");
            println!("  Status: ✅ Using embedded default configuration");
        }
    }

    // Worker prompt
    println!("\n⚡ Worker Prompt:");
    match &prompts.worker_prompt {
        Some(name) => {
            println!("  Config: {}", name);
            check_prompt_file(name);
        }
        None => {
            println!("  Config: worker (embedded)");
            println!("  Status: ✅ Using embedded worker configuration");
        }
    }

    // Memory prompt
    println!("\n🧠 Memory Prompt:");
    match &prompts.memory_prompt {
        Some(name) => {
            println!("  Config: {}", name);
            check_prompt_file(name);
        }
        None => {
            println!("  Config: memory (embedded)");
            println!("  Status: ✅ Using embedded memory configuration");
        }
    }

    println!();
    println!("Prompt Config Locations (checked in order):");
    println!("  1. ~/.config/mylm/prompts/config/<name>.json");
    println!("  2. ~/.config/mylm/prompts/config/<name>.yaml");
    println!("  3. ~/.config/mylm/prompts/<name>.md (legacy)");
    println!("  4. ./assets/prompts/config/<name>.json");
    println!("  5. Embedded fallback");

    println!();
    println!("Press Enter to continue...");
    let _ = std::io::stdin().read_line(&mut String::new());

    Ok(false)
}

/// Check if a prompt file exists and print status
fn check_prompt_file(name: &str) {
    let config_dir = dirs::config_dir()
        .map(|d| d.join("mylm").join("prompts").join("config"))
        .unwrap_or_default();

    let json_path = config_dir.join(format!("{}.json", name));
    let yaml_path = config_dir.join(format!("{}.yaml", name));
    let yml_path = config_dir.join(format!("{}.yml", name));
    let md_path = config_dir
        .parent()
        .map(|p| p.join(format!("{}.md", name)))
        .unwrap_or_default();

    if json_path.exists() {
        println!("  Status: ✅ Found at {}", json_path.display());
    } else if yaml_path.exists() {
        println!("  Status: ✅ Found at {}", yaml_path.display());
    } else if yml_path.exists() {
        println!("  Status: ✅ Found at {}", yml_path.display());
    } else if md_path.exists() {
        println!("  Status: ✅ Found legacy .md at {}", md_path.display());
    } else {
        println!("  Status: ⚠️  Not found - will use embedded fallback");
        println!("  Expected: {}", json_path.display());
    }
}
