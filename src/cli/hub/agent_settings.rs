//! Agent settings - iterations, rate limits, PaCoRe, tmux autostart, permissions

use anyhow::Result;
use console::Style;
use inquire::{MultiSelect, Select as InquireSelect, Text};
use mylm_core::config::v2::{AgentPermissions, EscalationMode, WorkerShellConfig};
use mylm_core::config::{Config, ConfigUiExt};

use crate::cli::hub::menus::{
    AgentSettingsChoice, IterationsSettingsChoice, PaCoReSettingsChoice, PermissionsMenuChoice,
    RateLimitSettingsChoice, WorkerResilienceSettingsChoice, WorkerShellMenuChoice,
};

/// List of all available tools with descriptions
const ALL_TOOLS: &[(&str, &str)] = &[
    ("execute_command", "Execute shell commands"),
    ("web_search", "Search the web for information"),
    ("memory", "Store and retrieve memories"),
    ("crawl", "Crawl web pages"),
    ("read_file", "Read file contents"),
    ("write_file", "Write file contents"),
    ("git_status", "Get git repository status"),
    ("git_log", "Get git commit history"),
    ("git_diff", "Get git diff output"),
    ("global_state", "Manage global state"),
    ("system_monitor", "Monitor system resources"),
    ("terminal_sight", "Capture terminal screenshots"),
    ("wait", "Wait for a specified duration"),
    ("list_jobs", "List background jobs"),
    ("delegate", "Delegate tasks to sub-agents"),
    ("find", "Find files by pattern"),
];

/// Common safe commands for auto-approval suggestions
const COMMON_SAFE_COMMANDS: &[(&str, &str)] = &[
    ("ls *", "List directory contents"),
    ("pwd", "Print working directory"),
    ("echo *", "Print text/output"),
    ("cat *", "Display file contents"),
    ("head *", "Show first lines of files"),
    ("tail *", "Show last lines of files"),
    ("which *", "Locate command"),
    ("git status", "Git status"),
    ("git log *", "Git log"),
    ("git diff", "Git diff"),
    ("git show *", "Git show"),
    ("find *", "Find files"),
    ("grep *", "Search text"),
];

/// Common dangerous commands for forbidden suggestions
const COMMON_DANGEROUS_COMMANDS: &[(&str, &str)] = &[
    ("rm -rf *", "Recursive force delete"),
    ("dd if=*", "Disk write operations"),
    ("mkfs *", "Format filesystems"),
    ("shred *", "Secure file deletion"),
    (">: *", "Overwrite arbitrary files"),
    ("chmod -R 777 *", "Dangerous permission changes"),
    ("chown -R *", "Recursive ownership changes"),
    ("mv * /", "Move to root"),
    ("cp * /", "Copy to root"),
];

/// Show agent settings menu
pub fn show_agent_settings_menu(config: &Config) -> Result<AgentSettingsChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let resolved = config.resolve_profile();
    let pacore_status = if config.features.pacore.enabled {
        "On"
    } else {
        "Off"
    };
    let rate_limit = resolved.agent.iteration_rate_limit;

    // Get permissions status
    let perms_status = resolved
        .agent.permissions
        .as_ref()
        .map(|p| {
            let tools = p
                .allowed_tools
                .as_ref()
                .map(|t| format!("{} tools", t.len()))
                .unwrap_or_else(|| "all".to_string());
            let auto = p
                .auto_approve_commands
                .as_ref()
                .map(|a| format!("{} auto", a.len()))
                .unwrap_or_else(|| "0".to_string());
            let forbid = p
                .forbidden_commands
                .as_ref()
                .map(|f| format!("{} forbid", f.len()))
                .unwrap_or_else(|| "0".to_string());
            format!("{}/{}/{}", tools, auto, forbid)
        })
        .unwrap_or_else(|| "default".to_string());

    println!("\n⚙️  Agent Settings");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("  Current Values:");
    println!("    Max Iterations:      {}", resolved.agent.max_iterations);
    println!(
        "    Iteration Delay:     {}",
        if rate_limit == 0 {
            "0 (no delay)".to_string()
        } else {
            format!("{} ms", rate_limit)
        }
    );
    println!(
        "    Rate Limit Tier:     {}",
        resolved.agent.rate_limit_tier
    );
    println!(
        "    Max Workers:         {}",
        resolved.agent.worker_limit
    );
    println!(
        "    Main Agent RPM:      {}",
        if resolved.agent.main_rpm == 0 {
            "0 (unlimited)".to_string()
        } else {
            format!("{}", resolved.agent.main_rpm)
        }
    );
    println!(
        "    Workers RPM:         {}",
        if resolved.agent.workers_rpm == 0 {
            "0 (unlimited)".to_string()
        } else {
            format!("{}", resolved.agent.workers_rpm)
        }
    );
    println!(
        "    Tmux Autostart:      {}",
        if config.tmux_autostart { "On" } else { "Off" }
    );
    println!(
        "    PaCoRe:              {} (rounds: {})",
        pacore_status, config.features.pacore.rounds
    );
    println!("    Permissions:         {}", perms_status);
    println!(
        "    Agent Version:       {}",
        format!("{}", config.features.agent_version)
    );
    println!(
        "    Max Tool Failures:   {}",
        resolved.agent.max_tool_failures
    );
    println!();

    let options = vec![
        AgentSettingsChoice::IterationsSettings,
        AgentSettingsChoice::RateLimitSettings,
        AgentSettingsChoice::WorkerResilienceSettings,
        AgentSettingsChoice::ToggleTmuxAutostart,
        AgentSettingsChoice::ToggleAgentVersion,
        AgentSettingsChoice::PaCoReSettings,
        AgentSettingsChoice::PermissionsSettings,
        AgentSettingsChoice::Back,
    ];

    let ans: Result<AgentSettingsChoice, _> =
        InquireSelect::new("Select setting to change:", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(AgentSettingsChoice::Back),
    }
}

/// Show iterations settings submenu
pub fn show_iterations_settings_menu(config: &Config) -> Result<IterationsSettingsChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let resolved = config.resolve_profile();
    let rate_limit = resolved.agent.iteration_rate_limit;

    println!("\n🔁 Iterations Settings");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("  Current Values:");
    println!("    Max Iterations:      {}", resolved.agent.max_iterations);
    println!(
        "    Rate Limit:          {} ms",
        if rate_limit == 0 {
            "0 (no delay)".to_string()
        } else {
            rate_limit.to_string()
        }
    );
    println!();
    println!("  Rate Limit adds a pause between agent actions.");
    println!("  Useful for rate limiting or observing behavior.");
    println!();

    let options = vec![
        IterationsSettingsChoice::SetMaxIterations,
        IterationsSettingsChoice::SetRateLimit,
        IterationsSettingsChoice::Back,
    ];

    let ans: Result<IterationsSettingsChoice, _> =
        InquireSelect::new("Select setting to change:", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(IterationsSettingsChoice::Back),
    }
}

/// Show LLM Rate Limit settings submenu
pub fn show_rate_limit_settings_menu(config: &Config) -> Result<RateLimitSettingsChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let resolved = config.resolve_profile();

    println!("\n⏱️  Rate Limit Settings (LLM)");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("  Current Values:");
    println!(
        "    Rate Limit Tier:     {}",
        resolved.agent.rate_limit_tier
    );
    println!(
        "    Max Workers:         {}",
        resolved.agent.worker_limit
    );
    println!(
        "    Main Agent RPM:      {}",
        if resolved.agent.main_rpm == 0 {
            "0 (unlimited)".to_string()
        } else {
            format!("{} req/min", resolved.agent.main_rpm)
        }
    );
    println!(
        "    Workers RPM:         {}",
        if resolved.agent.workers_rpm == 0 {
            "0 (unlimited - shared pool)".to_string()
        } else {
            format!("{} req/min (shared pool)", resolved.agent.workers_rpm)
        }
    );
    println!();
    println!("  Rate Limit Tier:  Preset configurations based on your provider");
    println!("  Max Workers:      Maximum concurrent background jobs");
    println!("  Main Agent RPM:   Requests per minute for main agent");
    println!("  Workers RPM:      Shared pool for all background jobs");
    println!();

    let options = vec![
        RateLimitSettingsChoice::SetRateLimitTier,
        RateLimitSettingsChoice::SetWorkerLimit,
        RateLimitSettingsChoice::SetMainRpm,
        RateLimitSettingsChoice::SetWorkersRpm,
        RateLimitSettingsChoice::Back,
    ];

    let ans: Result<RateLimitSettingsChoice, _> =
        InquireSelect::new("Select setting to change:", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(RateLimitSettingsChoice::Back),
    }
}

/// Show PaCoRe settings submenu
pub fn show_pacore_settings_menu(config: &Config) -> Result<PaCoReSettingsChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let status = if config.features.pacore.enabled {
        "On"
    } else {
        "Off"
    };

    println!("\n⚡ PaCoRe Settings");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("  Current Values:");
    println!("    PaCoRe:              {}", status);
    println!("    Rounds:              {}", config.features.pacore.rounds);
    println!();
    println!("  PaCoRe uses parallel LLM calls to improve reasoning.");
    println!("  Format: comma-separated numbers (e.g., '4,1' or '16,4,1')");
    println!();

    let options = vec![
        PaCoReSettingsChoice::TogglePaCoRe,
        PaCoReSettingsChoice::SetPaCoReRounds,
        PaCoReSettingsChoice::Back,
    ];

    let ans: Result<PaCoReSettingsChoice, _> =
        InquireSelect::new("Select setting to change:", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(PaCoReSettingsChoice::Back),
    }
}

/// Show permissions settings submenu
pub fn show_permissions_menu(config: &Config) -> Result<PermissionsMenuChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let resolved = config.resolve_profile();

    println!("\n🔒 Permissions Settings");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );

    if let Some(perms) = &resolved.agent.permissions {
        let tools = perms
            .allowed_tools
            .as_ref()
            .map(|t| t.join(", "))
            .unwrap_or_else(|| "(all tools allowed)".to_string());
        let auto = perms
            .auto_approve_commands
            .as_ref()
            .map(|a| a.join(", "))
            .unwrap_or_else(|| "(none)".to_string());
        let forbid = perms
            .forbidden_commands
            .as_ref()
            .map(|f| f.join(", "))
            .unwrap_or_else(|| "(none)".to_string());

        println!("  Allowed Tools:         {}", tools);
        println!("  Auto-Approve Commands: {}", auto);
        println!("  Forbidden Commands:    {}", forbid);
    } else {
        println!("  Using default permissions (all tools allowed, no restrictions)");
    }
    println!();
    println!("  Allowed Tools: Comma-separated list of tool names.");
    println!("    Examples: shell, memory, web_search, file_read");
    println!("    Leave empty to allow all tools.");
    println!();
    println!("  Auto-Approve Commands: Glob patterns for auto-approved commands.");
    println!("    Examples: 'ls *', 'echo *', 'pwd', 'cat *'");
    println!();
    println!("  Forbidden Commands: Glob patterns for forbidden commands.");
    println!("    Examples: 'rm -rf *', 'dd if=*', 'mkfs *'");
    println!();

    let options = vec![
        PermissionsMenuChoice::SetAllowedTools,
        PermissionsMenuChoice::SetAutoApproveCommands,
        PermissionsMenuChoice::SetForbiddenCommands,
        PermissionsMenuChoice::ConfigureWorkerShell,
        PermissionsMenuChoice::Back,
    ];

    let ans: Result<PermissionsMenuChoice, _> =
        InquireSelect::new("Select setting to change:", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(PermissionsMenuChoice::Back),
    }
}

/// Handle changing max iterations
pub fn handle_max_iterations(config: &mut Config) -> Result<bool> {
    let current = config.resolve_profile().agent.max_iterations;

    let input = Text::new("Max iterations:")
        .with_initial_value(&current.to_string())
        .prompt()?;

    match input.parse::<usize>() {
        Ok(iters) if iters > 0 && iters <= 100 => {
            let profile_name = config.profile.clone();
            config.set_profile_max_iterations(&profile_name, Some(iters))?;
            config.save_to_default_location()?;
            println!("✅ Max iterations set to: {}", iters);
            Ok(true)
        }
        _ => {
            println!("⚠️  Invalid value. Must be between 1 and 100.");
            Ok(false)
        }
    }
}

/// Handle setting iteration rate limit (delay between iterations)
pub fn handle_set_rate_limit(config: &mut Config) -> Result<bool> {
    let current = config.resolve_profile().agent.iteration_rate_limit;

    let input = Text::new("Iteration delay (ms between actions):")
        .with_help_message("0 = no delay, higher values add pause between actions")
        .with_initial_value(&current.to_string())
        .prompt()?;

    match input.parse::<u64>() {
        Ok(ms) => {
            let profile_name = config.profile.clone();
            config.set_profile_iteration_rate_limit(&profile_name, Some(ms))?;
            config.save_to_default_location()?;
            if ms == 0 {
                println!("✅ Iteration delay disabled (no delay between actions)");
            } else {
                println!("✅ Iteration delay set to: {} ms between actions", ms);
            }
            Ok(true)
        }
        _ => {
            println!("⚠️  Invalid value. Must be a positive number.");
            Ok(false)
        }
    }
}

/// Handle setting main agent RPM (requests per minute)
pub fn handle_set_main_rpm(config: &mut Config) -> Result<bool> {
    let current = config.resolve_profile().agent.main_rpm;

    let input = Text::new("Main Agent Rate Limit (requests per minute):")
        .with_help_message("0 = unlimited, typical values: 60 (1/sec), 120 (2/sec)")
        .with_initial_value(&current.to_string())
        .prompt()?;

    match input.parse::<u32>() {
        Ok(rpm) => {
            let profile_name = config.profile.clone();
            config.set_profile_main_rpm(&profile_name, Some(rpm))?;
            config.save_to_default_location()?;
            if rpm == 0 {
                println!("✅ Main agent rate limit disabled (unlimited requests)");
            } else {
                println!("✅ Main agent rate limit set to: {} requests/minute", rpm);
            }
            Ok(true)
        }
        _ => {
            println!("⚠️  Invalid value. Must be a positive number or 0.");
            Ok(false)
        }
    }
}

/// Handle setting workers RPM (requests per minute, shared pool)
pub fn handle_set_workers_rpm(config: &mut Config) -> Result<bool> {
    let current = config.resolve_profile().agent.workers_rpm;

    let input = Text::new("Workers Rate Limit (requests per minute):")
        .with_help_message("0 = unlimited, recommended: 10-30 (shared across all workers)")
        .with_initial_value(&current.to_string())
        .prompt()?;

    match input.parse::<u32>() {
        Ok(rpm) => {
            let profile_name = config.profile.clone();
            config.set_profile_workers_rpm(&profile_name, Some(rpm))?;
            config.save_to_default_location()?;
            if rpm == 0 {
                println!("✅ Workers rate limit disabled (unlimited requests)");
            } else {
                println!("✅ Workers rate limit set to: {} requests/minute (shared pool)", rpm);
            }
            Ok(true)
        }
        _ => {
            println!("⚠️  Invalid value. Must be a positive number or 0.");
            Ok(false)
        }
    }
}

/// Handle setting rate limit tier
pub fn handle_set_rate_limit_tier(config: &mut Config) -> Result<bool> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let resolved = config.resolve_profile();
    let current = &resolved.agent.rate_limit_tier;

    println!("\n⚡ Rate Limit Tier Configuration");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("Current tier: {}", current);
    println!();
    println!("Select a tier based on your provider's rate limits:");
    println!();
    println!("  Conservative - Basic/free tier providers");
    println!("    Workers: 10 | Main RPM: 60 | Workers RPM: 30");
    println!();
    println!("  Standard - Most providers (default)");
    println!("    Workers: 20 | Main RPM: 120 | Workers RPM: 300");
    println!();
    println!("  High - Premium tier providers");
    println!("    Workers: 50 | Main RPM: 300 | Workers RPM: 1200");
    println!();
    println!("  Enterprise - Unlimited/custom agreements");
    println!("    Workers: 100 | Main RPM: 600 | Workers RPM: 6000");
    println!();

    let options = vec!["conservative", "standard", "high", "enterprise"];
    
    // Find starting cursor position based on current tier
    let starting_cursor = options.iter().position(|&t| t == current.as_str()).unwrap_or(1);
    
    let ans = InquireSelect::new("Select tier:", options)
        .with_starting_cursor(starting_cursor)
        .prompt();

    match ans {
        Ok(tier) => {
            let tier_str: String = tier.to_string();
            let profile_name = config.profile.clone();
            config.set_profile_rate_limit_tier(&profile_name, Some(tier_str))?;
            
            // Also apply the tier's recommended worker_limit and RPM values
            let (worker_limit, main_rpm, workers_rpm) = match tier.as_ref() {
                "conservative" => (10, 60, 30),
                "standard" => (20, 120, 300),
                "high" => (50, 300, 1200),
                "enterprise" => (100, 600, 6000),
                _ => (20, 120, 300),
            };
            
            config.set_profile_worker_limit(&profile_name, Some(worker_limit))?;
            config.set_profile_main_rpm(&profile_name, Some(main_rpm))?;
            config.set_profile_workers_rpm(&profile_name, Some(workers_rpm))?;
            
            config.save_to_default_location()?;
            println!("✅ Rate limit tier set to: {}", tier);
            println!("   Workers: {} | Main RPM: {} | Workers RPM: {}", 
                worker_limit, main_rpm, workers_rpm);
            Ok(true)
        }
        Err(_) => {
            println!("⚠️  Cancelled - no changes made");
            Ok(false)
        }
    }
}

/// Handle setting worker limit
pub fn handle_set_worker_limit(config: &mut Config) -> Result<bool> {
    let resolved = config.resolve_profile();
    let current = resolved.agent.worker_limit;

    let input = Text::new("Maximum concurrent workers:")
        .with_help_message("Higher values = more parallelism. Recommended: 10-100")
        .with_initial_value(&current.to_string())
        .prompt()?;

    match input.parse::<usize>() {
        Ok(limit) if limit > 0 && limit <= 500 => {
            let profile_name = config.profile.clone();
            config.set_profile_worker_limit(&profile_name, Some(limit))?;
            config.save_to_default_location()?;
            println!("✅ Maximum workers set to: {}", limit);
            Ok(true)
        }
        _ => {
            println!("⚠️  Invalid value. Must be between 1 and 500.");
            Ok(false)
        }
    }
}

/// Handle toggling PaCoRe
pub fn handle_toggle_pacore(config: &mut Config) -> Result<bool> {
    config.features.pacore.enabled = !config.features.pacore.enabled;
    config.save_to_default_location()?;
    let status = if config.features.pacore.enabled {
        "enabled"
    } else {
        "disabled"
    };
    println!("✅ PaCoRe {}", status);
    Ok(true)
}

/// Handle setting PaCoRe rounds
pub fn handle_set_pacore_rounds(config: &mut Config) -> Result<bool> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let current = config.features.pacore.rounds.clone();

    println!("\n📊 PaCoRe Rounds Configuration");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("Format: comma-separated numbers (e.g., '4,1' or '16,4,1')");
    println!("  - First number: parallel calls in round 1");
    println!("  - Last number: should be 1 for final synthesis");
    println!("  - Example: 4,1 = 4 parallel calls, then 1 synthesis");
    println!("  - Example: 16,4,1 = 16 calls → 4 synthesis → 1 final");
    println!();

    let input = Text::new("Rounds:")
        .with_initial_value(&current)
        .prompt()?;

    // Validate format
    let parts: Vec<&str> = input.split(',').collect();
    let mut valid = true;

    for part in &parts {
        if part.trim().parse::<usize>().is_err() {
            valid = false;
            break;
        }
    }

    if !valid || parts.is_empty() {
        println!("⚠️  Invalid format. Use comma-separated numbers (e.g., 4,1)");
        return Ok(false);
    }

    // Warn if last number isn't 1
    if let Some(last) = parts.last() {
        if last.trim() != "1" {
            println!("⚠️  Warning: Last round should be 1 for proper synthesis");
        }
    }

    config.features.pacore.rounds = input.clone();
    config.save_to_default_location()?;
    println!("✅ PaCoRe rounds set to: {}", input);
    Ok(true)
}

/// Handle toggling tmux autostart
pub fn handle_toggle_tmux_autostart(config: &mut Config) -> Result<bool> {
    config.tmux_autostart = !config.tmux_autostart;
    config.save_to_default_location()?;
    println!(
        "✅ Tmux autostart {}",
        if config.tmux_autostart {
            "enabled"
        } else {
            "disabled"
        }
    );
    Ok(true)
}

/// Handle toggling agent version (V1/V2)
pub fn handle_toggle_agent_version(config: &mut Config) -> Result<bool> {
    use mylm_core::config::AgentVersion;
    config.features.agent_version = match config.features.agent_version {
        AgentVersion::V1 => AgentVersion::V2,
        AgentVersion::V2 => AgentVersion::V1,
    };
    config.save_to_default_location()?;
    println!("✅ Agent version set to: {}", config.features.agent_version);
    Ok(true)
}

/// Get or create permissions for the current profile
fn get_or_create_permissions(config: &mut Config) -> &mut AgentPermissions {
    let profile_name = config.profile.clone();
    let profile = config.profiles.entry(profile_name).or_default();
    let agent_override = profile.agent.get_or_insert_with(Default::default);
    agent_override.permissions.get_or_insert_with(Default::default)
}

/// Tool option for MultiSelect display
#[derive(Debug, Clone, PartialEq)]
struct ToolOption {
    name: &'static str,
    description: &'static str,
}

impl std::fmt::Display for ToolOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:<20} - {}", self.name, self.description)
    }
}

/// Handle setting allowed tools with checklist
pub fn handle_set_allowed_tools(config: &mut Config) -> Result<bool> {
    let resolved = config.resolve_profile();
    let current_tools: Vec<String> = resolved
        .agent.permissions
        .as_ref()
        .and_then(|p| p.allowed_tools.clone())
        .unwrap_or_default();

    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n🔧 Allowed Tools Configuration");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(60))
    );
    println!("Select which tools the agent is allowed to use.");
    println!("If no tools are selected, ALL tools will be allowed.");
    println!();
    println!("Space to select/deselect, Enter to confirm, Esc to cancel.");
    println!();

    // Create options from ALL_TOOLS
    let options: Vec<ToolOption> = ALL_TOOLS
        .iter()
        .map(|(name, desc)| ToolOption {
            name,
            description: desc,
        })
        .collect();

    // Pre-select currently allowed tools (by index)
    let defaults: Vec<usize> = options
        .iter()
        .enumerate()
        .filter(|(_, opt)| current_tools.iter().any(|t| t == opt.name))
        .map(|(i, _)| i)
        .collect();

    let selected = MultiSelect::new("Select allowed tools:", options)
        .with_default(&defaults)
        .prompt();

    match selected {
        Ok(selections) => {
            let perms = get_or_create_permissions(config);

            if selections.is_empty() {
                perms.allowed_tools = None;
                println!("✅ All tools are now allowed");
            } else {
                let tools: Vec<String> = selections.iter().map(|s| s.name.to_string()).collect();
                perms.allowed_tools = Some(tools.clone());
                println!("✅ Allowed {} tool(s): {}", tools.len(), tools.join(", "));
            }

            config.save_to_default_location()?;
            println!("💾 Configuration saved");
            Ok(true)
        }
        Err(inquire::InquireError::OperationCanceled) => {
            println!("⚠️  Cancelled - no changes made");
            Ok(false)
        }
        Err(e) => {
            println!("⚠️  Error: {}", e);
            Ok(false)
        }
    }
}

/// Command pattern option for MultiSelect
#[derive(Debug, Clone, PartialEq)]
struct CommandPattern {
    pattern: &'static str,
    description: &'static str,
}

impl std::fmt::Display for CommandPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:<20} - {}", self.pattern, self.description)
    }
}

/// Handle setting auto-approve commands with better UX
pub fn handle_set_auto_approve_commands(config: &mut Config) -> Result<bool> {
    let resolved = config.resolve_profile();
    let current_patterns: Vec<String> = resolved
        .agent.permissions
        .as_ref()
        .and_then(|p| p.auto_approve_commands.clone())
        .unwrap_or_default();

    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n✅ Auto-Approve Commands Configuration");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(60))
    );
    println!("Commands matching these patterns will be executed WITHOUT confirmation.");
    println!();
    println!("Current patterns: {}", 
        if current_patterns.is_empty() { "(none)".to_string() } else { current_patterns.join(", ") }
    );
    println!();

    // Show options
    println!("Options:");
    println!("  [1] Select from common safe commands (ls, pwd, git status, etc.)");
    println!("  [2] Enter custom patterns");
    println!("  [3] Clear all auto-approve patterns");
    println!("  [4] Back without changes");
    println!();

    let choice = InquireSelect::new(
        "Select option:",
        vec!["Select from common commands", "Enter custom patterns", "Clear all", "Back"],
    ).prompt()?;

    let perms = get_or_create_permissions(config);

    match choice {
        "Select from common commands" => {
            let options: Vec<CommandPattern> = COMMON_SAFE_COMMANDS
                .iter()
                .map(|(pattern, desc)| CommandPattern { pattern, description: desc })
                .collect();

            // Pre-select currently enabled common commands
            let defaults: Vec<usize> = options
                .iter()
                .enumerate()
                .filter(|(_, opt)| current_patterns.iter().any(|p| p == opt.pattern))
                .map(|(i, _)| i)
                .collect();

            let selected = MultiSelect::new("Select commands to auto-approve:", options)
                .with_default(&defaults)
                .prompt();

            match selected {
                Ok(selections) => {
                    // Start with selected common commands
                    let mut new_patterns: Vec<String> = selections
                        .iter()
                        .map(|s| s.pattern.to_string())
                        .collect();
                    
                    // Add any custom patterns (those not in common list)
                    for existing in &current_patterns {
                        if !COMMON_SAFE_COMMANDS.iter().any(|(p, _)| p == existing) {
                            new_patterns.push(existing.clone());
                        }
                    }

                    if new_patterns.is_empty() {
                        perms.auto_approve_commands = None;
                        println!("✅ Auto-approve commands cleared");
                    } else {
                        new_patterns.sort();
                        new_patterns.dedup();
                        perms.auto_approve_commands = Some(new_patterns.clone());
                        println!("✅ Auto-approve patterns updated ({} total)", new_patterns.len());
                    }
                }
                Err(inquire::InquireError::OperationCanceled) => {
                    println!("⚠️  Cancelled - no changes made");
                    return Ok(false);
                }
                Err(e) => {
                    println!("⚠️  Error: {}", e);
                    return Ok(false);
                }
            }
        }
        "Enter custom patterns" => {
            let input = Text::new("Enter patterns (comma-separated, use * for wildcards):")
                .with_initial_value(&current_patterns.join(", "))
                .with_help_message("Examples: ls *, echo *, pwd, cat /etc/passwd")
                .prompt()?;

            if input.trim().is_empty() {
                perms.auto_approve_commands = None;
                println!("✅ Auto-approve commands cleared");
            } else {
                let patterns: Vec<String> = input
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                perms.auto_approve_commands = Some(patterns.clone());
                println!("✅ Auto-approve patterns updated ({} patterns)", patterns.len());
            }
        }
        "Clear all" => {
            perms.auto_approve_commands = None;
            println!("✅ Auto-approve commands cleared");
        }
        _ => return Ok(false),
    }

    config.save_to_default_location()?;
    println!("💾 Configuration saved");
    Ok(true)
}

/// Handle setting forbidden commands with better UX
pub fn handle_set_forbidden_commands(config: &mut Config) -> Result<bool> {
    let resolved = config.resolve_profile();
    let current_patterns: Vec<String> = resolved
        .agent.permissions
        .as_ref()
        .and_then(|p| p.forbidden_commands.clone())
        .unwrap_or_default();

    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n🚫 Forbidden Commands Configuration");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(60))
    );
    println!("Commands matching these patterns will be BLOCKED from execution.");
    println!("These patterns take precedence over auto-approve.");
    println!();
    println!("Current patterns: {}", 
        if current_patterns.is_empty() { "(none)".to_string() } else { current_patterns.join(", ") }
    );
    println!();

    // Show options
    println!("Options:");
    println!("  [1] Select from common dangerous commands (rm -rf, dd, mkfs, etc.)");
    println!("  [2] Enter custom patterns");
    println!("  [3] Clear all forbidden patterns");
    println!("  [4] Back without changes");
    println!();

    let choice = InquireSelect::new(
        "Select option:",
        vec!["Select from dangerous commands", "Enter custom patterns", "Clear all", "Back"],
    ).prompt()?;

    let perms = get_or_create_permissions(config);

    match choice {
        "Select from dangerous commands" => {
            let options: Vec<CommandPattern> = COMMON_DANGEROUS_COMMANDS
                .iter()
                .map(|(pattern, desc)| CommandPattern { pattern, description: desc })
                .collect();

            // Pre-select currently enabled dangerous commands
            let defaults: Vec<usize> = options
                .iter()
                .enumerate()
                .filter(|(_, opt)| current_patterns.iter().any(|p| p == opt.pattern))
                .map(|(i, _)| i)
                .collect();

            let selected = MultiSelect::new("Select commands to forbid:", options)
                .with_default(&defaults)
                .prompt();

            match selected {
                Ok(selections) => {
                    // Start with selected dangerous commands
                    let mut new_patterns: Vec<String> = selections
                        .iter()
                        .map(|s| s.pattern.to_string())
                        .collect();
                    
                    // Add any custom patterns (those not in common list)
                    for existing in &current_patterns {
                        if !COMMON_DANGEROUS_COMMANDS.iter().any(|(p, _)| p == existing) {
                            new_patterns.push(existing.clone());
                        }
                    }

                    if new_patterns.is_empty() {
                        perms.forbidden_commands = None;
                        println!("✅ Forbidden commands cleared");
                    } else {
                        new_patterns.sort();
                        new_patterns.dedup();
                        perms.forbidden_commands = Some(new_patterns.clone());
                        println!("✅ Forbidden patterns updated ({} total)", new_patterns.len());
                    }
                }
                Err(inquire::InquireError::OperationCanceled) => {
                    println!("⚠️  Cancelled - no changes made");
                    return Ok(false);
                }
                Err(e) => {
                    println!("⚠️  Error: {}", e);
                    return Ok(false);
                }
            }
        }
        "Enter custom patterns" => {
            let input = Text::new("Enter patterns (comma-separated, use * for wildcards):")
                .with_initial_value(&current_patterns.join(", "))
                .with_help_message("Examples: rm -rf *, dd if=*, mkfs *")
                .prompt()?;

            if input.trim().is_empty() {
                perms.forbidden_commands = None;
                println!("✅ Forbidden commands cleared");
            } else {
                let patterns: Vec<String> = input
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                perms.forbidden_commands = Some(patterns.clone());
                println!("✅ Forbidden patterns updated ({} patterns)", patterns.len());
            }
        }
        "Clear all" => {
            perms.forbidden_commands = None;
            println!("✅ Forbidden commands cleared");
        }
        _ => return Ok(false),
    }

    config.save_to_default_location()?;
    println!("💾 Configuration saved");
    Ok(true)
}

/// Show worker resilience settings submenu
pub fn show_worker_resilience_menu(config: &Config) -> Result<WorkerResilienceSettingsChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let resolved = config.resolve_profile();

    println!("\n🛡️  Worker Resilience Settings");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("  Current Values:");
    println!(
        "    Max Tool Failures:   {}",
        resolved.agent.max_tool_failures
    );
    println!();
    println!("  Max Tool Failures: Number of consecutive tool failures before");
    println!("                    a worker is considered stalled.");
    println!("                    Default: 5. Set to 0 to disable stalling.");
    println!();

    let options = vec![
        WorkerResilienceSettingsChoice::SetMaxToolFailures,
        WorkerResilienceSettingsChoice::Back,
    ];

    let ans: Result<WorkerResilienceSettingsChoice, _> =
        InquireSelect::new("Select setting to change:", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(WorkerResilienceSettingsChoice::Back),
    }
}

/// Handle setting max tool failures before worker stalls
pub fn handle_set_max_tool_failures(config: &mut Config) -> Result<bool> {
    let current = config.resolve_profile().agent.max_tool_failures;

    let input = Text::new("Max consecutive tool failures before stall:")
        .with_help_message("0 = disabled (never stall), typical values: 3-10")
        .with_initial_value(&current.to_string())
        .prompt()?;

    match input.parse::<usize>() {
        Ok(failures) => {
            let profile_name = config.profile.clone();
            config.set_profile_max_tool_failures(&profile_name, Some(failures))?;
            config.save_to_default_location()?;
            if failures == 0 {
                println!("✅ Max tool failures disabled (workers will never stall due to failures)");
            } else {
                println!("✅ Max tool failures set to: {} consecutive failures", failures);
            }
            Ok(true)
        }
        _ => {
            println!("⚠️  Invalid value. Must be a non-negative number.");
            Ok(false)
        }
}
}

/// Show worker shell permissions submenu
pub fn show_worker_shell_menu(config: &Config) -> Result<WorkerShellMenuChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let resolved = config.resolve_profile();

    println!("\n👷 Worker Shell Permissions");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );

    // Display current configuration - avoid temporary value drops by storing defaults first
    let default_allowed = WorkerShellConfig::default_allowed().join(", ");
    let default_restricted = WorkerShellConfig::default_restricted().join(", ");
    
    let (allowed, restricted, forbidden, escalation) = if let Some(perms) = &resolved.agent.permissions {
        if let Some(worker_shell) = &perms.worker_shell {
            (
                worker_shell.allowed_patterns.as_ref()
                    .map(|p| p.join(", "))
                    .unwrap_or_else(|| "(none)".to_string()),
                worker_shell.restricted_patterns.as_ref()
                    .map(|p| p.join(", "))
                    .unwrap_or_else(|| "(none)".to_string()),
                worker_shell.forbidden_patterns.as_ref()
                    .map(|p| p.join(", "))
                    .unwrap_or_else(|| "(none)".to_string()),
                worker_shell.escalation_mode.as_ref()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "EscalateToMain (default)".to_string())
            )
        } else {
            // No worker_shell config, show defaults
            (
                default_allowed,
                default_restricted,
                "(none)".to_string(),
                "EscalateToMain (default)".to_string()
            )
        }
    } else {
        // No permissions at all, show defaults
        (
            default_allowed,
            default_restricted,
            "(none)".to_string(),
            "EscalateToMain (default)".to_string()
        )
    };

    println!("  Current Configuration:");
    println!("    Allowed Patterns:    {}", allowed);
    println!("    Restricted Patterns: {}", restricted);
    println!("    Forbidden Patterns:  {}", forbidden);
    println!("    Escalation Mode:     {}", escalation);
    println!();
    println!("  Allowed Patterns: Commands that workers can execute WITHOUT escalation.");
    println!("  Restricted Patterns: Commands that require escalation to main agent.");
    println!("  Forbidden Patterns: Commands that are ALWAYS blocked.");
    println!("  Escalation Mode: How to handle restricted commands:");
    println!("    - EscalateToMain: Ask main agent for approval");
    println!("    - BlockRestricted: Block restricted commands entirely");
    println!("    - AllowAll: Allow all commands (debug/dangerous!)");
    println!();

    let options = vec![
        WorkerShellMenuChoice::SetAllowedPatterns,
        WorkerShellMenuChoice::SetRestrictedPatterns,
        WorkerShellMenuChoice::SetForbiddenPatterns,
        WorkerShellMenuChoice::SetEscalationMode,
        WorkerShellMenuChoice::ResetToDefaults,
        WorkerShellMenuChoice::Back,
    ];

    let ans: Result<WorkerShellMenuChoice, _> =
        InquireSelect::new("Select setting to change:", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(WorkerShellMenuChoice::Back),
    }
}

/// Get or create worker_shell config for the current profile
fn get_or_create_worker_shell(config: &mut Config) -> &mut WorkerShellConfig {
    let profile_name = config.profile.clone();
    let profile = config.profiles.entry(profile_name).or_default();
    let agent_override = profile.agent.get_or_insert_with(Default::default);
    let perms = agent_override.permissions.get_or_insert_with(Default::default);
    perms.worker_shell.get_or_insert_with(Default::default)
}

/// Handle setting allowed patterns for worker shell
pub fn handle_set_worker_allowed_commands(config: &mut Config) -> Result<bool> {
    let resolved = config.resolve_profile();
    let current = resolved.agent.permissions.as_ref()
        .and_then(|p| p.worker_shell.as_ref())
        .and_then(|ws| ws.allowed_patterns.as_ref())
        .map(|p| p.join(", "))
        .unwrap_or_else(|| WorkerShellConfig::default_allowed().join(", "));

    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n🔧 Worker Shell - Allowed Patterns");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(60))
    );
    println!("Commands matching these patterns can be executed WITHOUT escalation.");
    println!("Use * as wildcard (e.g., 'ls *', 'cat *', 'pwd').");
    println!();
    println!("Current patterns: {}", current);
    println!();

    let input = Text::new("Enter patterns (comma-separated):")
        .with_initial_value(&current)
        .with_help_message("Examples: ls *, cat *, pwd, echo *, sleep *, date")
        .prompt()?;

    let worker_shell = get_or_create_worker_shell(config);

    if input.trim().is_empty() {
        worker_shell.allowed_patterns = None;
        println!("✅ Allowed patterns cleared (using defaults)");
    } else {
        let patterns: Vec<String> = input
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        worker_shell.allowed_patterns = Some(patterns.clone());
        println!("✅ Allowed patterns updated ({} patterns)", patterns.len());
    }

    config.save_to_default_location()?;
    println!("💾 Configuration saved");
    Ok(true)
}

/// Handle setting restricted patterns for worker shell
pub fn handle_set_worker_restricted_commands(config: &mut Config) -> Result<bool> {
    let resolved = config.resolve_profile();
    let current = resolved.agent.permissions.as_ref()
        .and_then(|p| p.worker_shell.as_ref())
        .and_then(|ws| ws.restricted_patterns.as_ref())
        .map(|p| p.join(", "))
        .unwrap_or_else(|| WorkerShellConfig::default_restricted().join(", "));

    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n⚠️  Worker Shell - Restricted Patterns");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(60))
    );
    println!("Commands matching these patterns require escalation to main agent.");
    println!("Use * as wildcard (e.g., 'rm *', 'curl *', 'ssh *').");
    println!();
    println!("Current patterns: {}", current);
    println!();

    let input = Text::new("Enter patterns (comma-separated):")
        .with_initial_value(&current)
        .with_help_message("Examples: rm *, mv *, cp *, curl *, wget *, ssh *")
        .prompt()?;

    let worker_shell = get_or_create_worker_shell(config);

    if input.trim().is_empty() {
        worker_shell.restricted_patterns = None;
        println!("✅ Restricted patterns cleared (using defaults)");
    } else {
        let patterns: Vec<String> = input
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        worker_shell.restricted_patterns = Some(patterns.clone());
        println!("✅ Restricted patterns updated ({} patterns)", patterns.len());
    }

    config.save_to_default_location()?;
    println!("💾 Configuration saved");
    Ok(true)
}

/// Handle setting forbidden patterns for worker shell
pub fn handle_set_worker_forbidden_commands(config: &mut Config) -> Result<bool> {
    let resolved = config.resolve_profile();
    let current = resolved.agent.permissions.as_ref()
        .and_then(|p| p.worker_shell.as_ref())
        .and_then(|ws| ws.forbidden_patterns.as_ref())
        .map(|p| p.join(", "))
        .unwrap_or_else(|| "(none)".to_string());

    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n🚫 Worker Shell - Forbidden Patterns");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(60))
    );
    println!("Commands matching these patterns are ALWAYS blocked.");
    println!("These take precedence over allowed/restricted patterns.");
    println!("Use * as wildcard (e.g., 'sudo *', 'rm -rf /').");
    println!();
    println!("Current patterns: {}", current);
    println!();

    let input = Text::new("Enter patterns (comma-separated):")
        .with_initial_value(&current)
        .with_help_message("Examples: sudo *, rm -rf /, dd if=*, mkfs *")
        .prompt()?;

    let worker_shell = get_or_create_worker_shell(config);

    if input.trim().is_empty() {
        worker_shell.forbidden_patterns = None;
        println!("✅ Forbidden patterns cleared");
    } else {
        let patterns: Vec<String> = input
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        worker_shell.forbidden_patterns = Some(patterns.clone());
        println!("✅ Forbidden patterns updated ({} patterns)", patterns.len());
    }

    config.save_to_default_location()?;
    println!("💾 Configuration saved");
    Ok(true)
}

/// Handle setting escalation mode for worker shell
pub fn handle_set_worker_escalation_mode(config: &mut Config) -> Result<bool> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let resolved = config.resolve_profile();
    let current = resolved.agent.permissions.as_ref()
        .and_then(|p| p.worker_shell.as_ref())
        .and_then(|ws| ws.escalation_mode.clone())
        .unwrap_or(EscalationMode::EscalateToMain);

    println!("\n⚙️  Worker Shell - Escalation Mode");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("Current mode: {}", current);
    println!();
    println!("How to handle restricted commands:");
    println!("  EscalateToMain - Send to main agent for approval (safe)");
    println!("  BlockRestricted - Block restricted commands entirely");
    println!("  AllowAll - Allow all commands (debug only - DANGEROUS!)");
    println!();

    let options = vec![
        EscalationMode::EscalateToMain,
        EscalationMode::BlockRestricted,
        EscalationMode::AllowAll,
    ];

    let ans = InquireSelect::new("Select escalation mode:", options)
        .with_starting_cursor(
            match current {
                EscalationMode::EscalateToMain => 0,
                EscalationMode::BlockRestricted => 1,
                EscalationMode::AllowAll => 2,
            }
        )
        .prompt();

    match ans {
        Ok(mode) => {
            let worker_shell = get_or_create_worker_shell(config);
            worker_shell.escalation_mode = Some(mode.clone());
            config.save_to_default_location()?;
            println!("✅ Escalation mode set to: {}", mode);
            Ok(true)
        }
        Err(_) => {
            println!("⚠️  Cancelled - no changes made");
            Ok(false)
        }
    }
}

/// Handle resetting worker shell to defaults
pub fn handle_reset_worker_shell_defaults(config: &mut Config) -> Result<bool> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    println!("\n🔄 Reset Worker Shell to Defaults");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("This will reset all worker shell settings to their default values.");
    println!("Default allowed: ls *, cat *, head *, tail *, pwd, echo *, sleep *, date, which *, git status*, ...");
    println!("Default restricted: rm *, mv *, cp *, curl *, wget *, ssh *, git push*, ...");
    println!("Default forbidden: (none)");
    println!("Default escalation: EscalateToMain");
    println!();

    let confirm = inquire::Confirm::new("Are you sure you want to reset?")
        .with_default(false)
        .prompt()?;

    if confirm {
        let worker_shell = get_or_create_worker_shell(config);
        *worker_shell = WorkerShellConfig::default();
        config.save_to_default_location()?;
        println!("✅ Worker shell settings reset to defaults");
        Ok(true)
    } else {
        println!("⚠️  Cancelled - no changes made");
        Ok(false)
    }
}
