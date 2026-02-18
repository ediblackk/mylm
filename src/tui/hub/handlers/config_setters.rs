//! Configuration setting handlers

use anyhow::Result;
use console::Style;
use dialoguer::{Input, MultiSelect, Select as DialogSelect};
use mylm_core::config::types::AgentPermissions;
use mylm_core::config::{Config, ProfileConfig};

/// Set max context tokens
pub fn set_max_tokens(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main {
        config.active_profile.clone()
    } else {
        "worker".to_string()
    };
    let current = config
        .profiles
        .get(&profile_name)
        .map(|p| p.context_window)
        .unwrap_or(8192);

    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Max Context Tokens"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    println!("\n  Current value: {}", Style::new().green().apply_to(current));
    println!(
        "  {}\n",
        Style::new()
            .dim()
            .apply_to("(Size of the context window in tokens)")
    );

    let new_value: usize = Input::new()
        .with_prompt("New value (e.g., 4096, 8192, 32768, 128000)")
        .default(current.to_string())
        .interact()?
        .parse()
        .unwrap_or(current);

    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
            permissions: None,
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }

    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.context_window = new_value;
        config.save_default()?;
        println!("\n✅ Max context tokens set to {}", new_value);
    }
    Ok(true)
}

/// Set condense threshold
pub fn set_condense_threshold(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main {
        config.active_profile.clone()
    } else {
        "worker".to_string()
    };

    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
            permissions: None,
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }

    let current = config
        .profiles
        .get(&profile_name)
        .and_then(|p| p.condense_threshold)
        .unwrap_or(0);

    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Condense Threshold"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    let current_display = if current == 0 {
        "Disabled".to_string()
    } else {
        current.to_string()
    };
    println!(
        "\n  Current value: {}",
        Style::new().green().apply_to(&current_display)
    );
    println!(
        "  {}\n",
        Style::new()
            .dim()
            .apply_to("(When to condense conversation history, 0 = disabled)")
    );

    let input: String = Input::new()
        .with_prompt("New value (0 = disabled, e.g., 4000)")
        .default(current.to_string())
        .interact()?;

    let new_value: usize = input.parse().unwrap_or(0);

    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.condense_threshold = if new_value == 0 {
            None
        } else {
            Some(new_value)
        };
        config.save_default()?;
        if new_value == 0 {
            println!("\n✅ Condense threshold disabled");
        } else {
            println!("\n✅ Condense threshold set to {}", new_value);
        }
    }
    Ok(true)
}

/// Set input price
pub fn set_input_price(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main {
        config.active_profile.clone()
    } else {
        "worker".to_string()
    };

    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
            permissions: None,
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }

    let current = config
        .profiles
        .get(&profile_name)
        .and_then(|p| p.input_price)
        .unwrap_or(0.0);

    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Input Price"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    let current_display = if current == 0.0 {
        "Not set".to_string()
    } else {
        format!("${:.2}", current)
    };
    println!(
        "\n  Current value: {}",
        Style::new().green().apply_to(current_display)
    );
    println!(
        "  {}\n",
        Style::new()
            .dim()
            .apply_to("(Cost per 1 million input tokens in USD)")
    );

    let input: String = Input::new()
        .with_prompt("New value (USD per 1M tokens, e.g., 0.50, 3.00, 0 = not set)")
        .default(current.to_string())
        .interact()?;

    let new_value: f64 = input.parse().unwrap_or(0.0);

    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.input_price = if new_value == 0.0 {
            None
        } else {
            Some(new_value)
        };
        config.save_default()?;
        println!("\n✅ Input price set to ${:.2} per 1M tokens", new_value);
    }
    Ok(true)
}

/// Set output price
pub fn set_output_price(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main {
        config.active_profile.clone()
    } else {
        "worker".to_string()
    };

    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
            permissions: None,
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }

    let current = config
        .profiles
        .get(&profile_name)
        .and_then(|p| p.output_price)
        .unwrap_or(0.0);

    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Output Price"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    let current_display = if current == 0.0 {
        "Not set".to_string()
    } else {
        format!("${:.2}", current)
    };
    println!(
        "\n  Current value: {}",
        Style::new().green().apply_to(current_display)
    );
    println!(
        "  {}\n",
        Style::new()
            .dim()
            .apply_to("(Cost per 1 million output tokens in USD)")
    );

    let input: String = Input::new()
        .with_prompt("New value (USD per 1M tokens, e.g., 1.50, 15.00, 0 = not set)")
        .default(current.to_string())
        .interact()?;

    let new_value: f64 = input.parse().unwrap_or(0.0);

    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.output_price = if new_value == 0.0 {
            None
        } else {
            Some(new_value)
        };
        config.save_default()?;
        println!("\n✅ Output price set to ${:.2} per 1M tokens", new_value);
    }
    Ok(true)
}

/// Set rate limit (RPM)
pub fn set_rate_limit_rpm(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main {
        config.active_profile.clone()
    } else {
        "worker".to_string()
    };

    // Ensure profile exists (create worker profile if needed)
    if !config.profiles.contains_key(&profile_name) {
        let new_profile = ProfileConfig {
            provider: config.active_profile().provider.clone(),
            model: None,
            max_iterations: 50,
            rate_limit_rpm: 60,
            context_window: 8192,
            temperature: 0.7,
            system_prompt: None,
            condense_threshold: None,
            tested_at: None,
            test_error: None,
            input_price: None,
            output_price: None,
            web_search: Default::default(),
            permissions: None,
        };
        config.profiles.insert(profile_name.clone(), new_profile);
    }

    let current = config
        .profiles
        .get(&profile_name)
        .map(|p| p.rate_limit_rpm)
        .unwrap_or(60);

    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Rate Limit (RPM)"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    let current_display = if current == 0 {
        "Unlimited".to_string()
    } else {
        format!("{} RPM", current)
    };
    println!(
        "\n  Current value: {}",
        Style::new().green().apply_to(current_display)
    );
    println!(
        "  {}\n",
        Style::new()
            .dim()
            .apply_to("(Maximum API requests per minute, 0 = unlimited)")
    );

    let input: String = Input::new()
        .with_prompt("New value (requests per minute, 0 = unlimited)")
        .default(current.to_string())
        .interact()?;

    let new_value: u32 = input.parse().unwrap_or(60);

    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.rate_limit_rpm = new_value;
        config.save_default()?;
        if new_value == 0 {
            println!("\n✅ Rate limit disabled (unlimited)");
        } else {
            println!("\n✅ Rate limit set to {} requests per minute", new_value);
        }
    }
    Ok(true)
}

/// Set always allowed commands/tools
pub fn set_allowed_commands(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { "default" } else { "worker" };

    // Ensure profile exists with permissions
    if !config.profiles.contains_key(profile_name) {
        let new_profile = ProfileConfig {
            provider: "openai".to_string(),
            permissions: Some(AgentPermissions::default()),
            ..ProfileConfig::default()
        };
        config
            .profiles
            .insert(profile_name.to_string(), new_profile);
    }

    // Get current allowed tools
    let profile = config.profiles.get(profile_name).unwrap();
    let permissions = profile.permissions.clone().unwrap_or_default();
    let current_allowed = permissions.main_agent.always_allow.clone();

    // Available tools to select from
    let available_tools = vec![
        ("read_file", "Read files (safe)"),
        ("cat", "Cat files (safe)"),
        ("list_files", "List directory contents (safe)"),
        ("ls", "List files (safe)"),
        ("git_status", "Git status (safe)"),
        ("git_log", "Git log (safe)"),
        ("git_diff", "Git diff (safe)"),
        ("git_branch", "Git branch (safe)"),
        ("shell", "Shell commands (dangerous)"),
        ("write_file", "Write files (dangerous)"),
        ("web_search", "Web search (external)"),
        ("memory", "Memory/recall"),
    ];

    let tool_names: Vec<&str> = available_tools.iter().map(|(name, _)| *name).collect();
    let tool_descriptions: Vec<String> = available_tools
        .iter()
        .map(|(name, desc)| format!("{} - {}", name, desc))
        .collect();

    // Pre-select current allowed tools
    let defaults: Vec<bool> = tool_names
        .iter()
        .map(|name| current_allowed.contains(&name.to_string()))
        .collect();

    print!("\x1B[2J\x1B[1;1H");
    let title = if is_main { "Main LLM" } else { "Worker LLM" };
    println!(
        "\n{}",
        Style::new()
            .bold()
            .apply_to(format!("{} - Always Allowed Tools", title))
    );
    println!(
        "{}",
        Style::new()
            .dim()
            .apply_to("Tools that don't require approval even when auto-approve is OFF")
    );
    println!("{}", Style::new().dim().apply_to("─".repeat(60)));

    let selection = MultiSelect::new()
        .with_prompt("Select tools to always allow (Space to toggle, Enter to confirm)")
        .items(&tool_descriptions)
        .defaults(&defaults)
        .interact()?;

    // Build new allowed list
    let new_allowed: Vec<String> = selection
        .iter()
        .map(|&idx| tool_names[idx].to_string())
        .collect();

    // Update config
    let profile = config.profiles.get_mut(profile_name).unwrap();
    let perms = profile
        .permissions
        .get_or_insert_with(AgentPermissions::default);
    if is_main {
        perms.main_agent.always_allow = new_allowed.clone();
    } else {
        perms.worker.always_allow = new_allowed.clone();
    }

    config.save_default()?;

    println!(
        "\n✅ Always allowed tools updated: {} tools",
        new_allowed.len()
    );
    std::thread::sleep(std::time::Duration::from_millis(800));
    Ok(true)
}

/// Set always restricted commands/tools
pub fn set_restricted_commands(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { "default" } else { "worker" };

    // Ensure profile exists with permissions
    if !config.profiles.contains_key(profile_name) {
        let new_profile = ProfileConfig {
            provider: "openai".to_string(),
            permissions: Some(AgentPermissions::default()),
            ..ProfileConfig::default()
        };
        config
            .profiles
            .insert(profile_name.to_string(), new_profile);
    }

    // Get current restricted tools
    let profile = config.profiles.get(profile_name).unwrap();
    let permissions = profile.permissions.clone().unwrap_or_default();
    let current_restricted = if is_main {
        permissions.main_agent.always_restrict.clone()
    } else {
        permissions.worker.always_restrict.clone()
    };

    // Available tools to restrict
    let available_tools = vec![
        ("shell", "Shell commands (always require approval)"),
        ("write_file", "Write files (always require approval)"),
        ("web_search", "Web search (always require approval)"),
        ("memory", "Memory/recall (always require approval)"),
    ];

    let tool_names: Vec<&str> = available_tools.iter().map(|(name, _)| *name).collect();
    let tool_descriptions: Vec<String> = available_tools
        .iter()
        .map(|(name, desc)| format!("{} - {}", name, desc))
        .collect();

    // Pre-select current restricted tools
    let defaults: Vec<bool> = tool_names
        .iter()
        .map(|name| current_restricted.contains(&name.to_string()))
        .collect();

    print!("\x1B[2J\x1B[1;1H");
    let title = if is_main { "Main LLM" } else { "Worker LLM" };
    println!(
        "\n{}",
        Style::new()
            .bold()
            .apply_to(format!("{} - Always Restricted Tools", title))
    );
    println!(
        "{}",
        Style::new()
            .dim()
            .apply_to("Tools that ALWAYS require approval even when auto-approve is ON")
    );
    println!("{}", Style::new().dim().apply_to("─".repeat(60)));

    let selection = MultiSelect::new()
        .with_prompt("Select tools to always restrict (Space to toggle, Enter to confirm)")
        .items(&tool_descriptions)
        .defaults(&defaults)
        .interact()?;

    // Build new restricted list
    let new_restricted: Vec<String> = selection
        .iter()
        .map(|&idx| tool_names[idx].to_string())
        .collect();

    // Update config
    let profile = config.profiles.get_mut(profile_name).unwrap();
    let perms = profile
        .permissions
        .get_or_insert_with(AgentPermissions::default);
    if is_main {
        perms.main_agent.always_restrict = new_restricted.clone();
    } else {
        perms.worker.always_restrict = new_restricted.clone();
    }

    config.save_default()?;

    println!(
        "\n✅ Always restricted tools updated: {} tools",
        new_restricted.len()
    );
    std::thread::sleep(std::time::Duration::from_millis(800));
    Ok(true)
}

/// Set shell command patterns that are auto-approved
pub fn set_shell_approved_patterns(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { "default" } else { "worker" };

    // Ensure profile exists with permissions
    if !config.profiles.contains_key(profile_name) {
        let new_profile = ProfileConfig {
            provider: "openai".to_string(),
            permissions: Some(AgentPermissions::default()),
            ..ProfileConfig::default()
        };
        config
            .profiles
            .insert(profile_name.to_string(), new_profile);
    }

    loop {
        // Get current patterns
        let profile = config.profiles.get(profile_name).unwrap();
        let permissions = profile.permissions.clone().unwrap_or_default();
        let current_patterns = permissions.auto_approve_commands.unwrap_or_default();

        print!("\x1B[2J\x1B[1;1H");
        let title = if is_main { "Main LLM" } else { "Worker LLM" };
        println!(
            "\n{}",
            Style::new()
                .bold()
                .apply_to(format!("{} - Shell: Approved Patterns", title))
        );
        println!(
            "{}",
            Style::new()
                .dim()
                .apply_to("Shell commands matching these patterns are auto-approved")
        );
        println!(
            "{}",
            Style::new().dim().apply_to("Use * as wildcard (e.g., 'ls *', 'cat *')")
        );
        println!("{}", Style::new().dim().apply_to("─".repeat(60)));

        if current_patterns.is_empty() {
            println!("No patterns configured.");
        } else {
            println!("Current patterns:");
            for (i, pattern) in current_patterns.iter().enumerate() {
                println!("  {}. {}", i + 1, pattern);
            }
        }
        println!();

        let options = vec!["➕ Add pattern", "🗑️  Remove pattern", "⬅️  Back"];

        let choice = DialogSelect::new()
            .with_prompt("Select action")
            .items(&options)
            .default(0)
            .interact()?;

        match choice {
            0 => {
                // Add pattern
                let new_pattern: String = Input::new()
                    .with_prompt("Enter pattern (e.g., 'ls *', 'cat *')")
                    .interact_text()?;

                if !new_pattern.trim().is_empty() {
                    let profile = config.profiles.get_mut(profile_name).unwrap();
                    let perms = profile
                        .permissions
                        .get_or_insert_with(AgentPermissions::default);
                    let mut patterns = perms.auto_approve_commands.clone().unwrap_or_default();
                    if !patterns.contains(&new_pattern) {
                        patterns.push(new_pattern.clone());
                        perms.auto_approve_commands = Some(patterns);
                        config.save_default()?;
                        println!("\n✅ Added pattern: {}", new_pattern);
                    } else {
                        println!("\n⚠️ Pattern already exists");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
            1 => {
                // Remove pattern
                if current_patterns.is_empty() {
                    println!("\nNo patterns to remove");
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }

                let items: Vec<String> =
                    current_patterns.iter().map(|p| format!("{}", p)).collect();

                let selection = DialogSelect::new()
                    .with_prompt("Select pattern to remove")
                    .items(&items)
                    .interact()?;

                let profile = config.profiles.get_mut(profile_name).unwrap();
                let perms = profile
                    .permissions
                    .get_or_insert_with(AgentPermissions::default);
                let mut patterns = perms.auto_approve_commands.clone().unwrap_or_default();
                let removed = patterns.remove(selection);
                perms.auto_approve_commands = Some(patterns);
                config.save_default()?;
                println!("\n✅ Removed pattern: {}", removed);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            _ => break,
        }
    }

    Ok(true)
}

/// Set shell command patterns that are forbidden
pub fn set_shell_forbidden_patterns(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { "default" } else { "worker" };

    // Ensure profile exists with permissions
    if !config.profiles.contains_key(profile_name) {
        let new_profile = ProfileConfig {
            provider: "openai".to_string(),
            permissions: Some(AgentPermissions::default()),
            ..ProfileConfig::default()
        };
        config
            .profiles
            .insert(profile_name.to_string(), new_profile);
    }

    loop {
        // Get current patterns
        let profile = config.profiles.get(profile_name).unwrap();
        let permissions = profile.permissions.clone().unwrap_or_default();
        let current_patterns = permissions.forbidden_commands.unwrap_or_default();

        print!("\x1B[2J\x1B[1;1H");
        let title = if is_main { "Main LLM" } else { "Worker LLM" };
        println!(
            "\n{}",
            Style::new()
                .bold()
                .apply_to(format!("{} - Shell: Forbidden Patterns", title))
        );
        println!(
            "{}",
            Style::new()
                .dim()
                .apply_to("Shell commands matching these patterns are ALWAYS forbidden")
        );
        println!(
            "{}",
            Style::new().dim().apply_to("Use * as wildcard (e.g., 'rm -rf *', 'sudo *')")
        );
        println!("{}", Style::new().dim().apply_to("─".repeat(60)));

        if current_patterns.is_empty() {
            println!("No patterns configured.");
        } else {
            println!("Current patterns:");
            for (i, pattern) in current_patterns.iter().enumerate() {
                println!("  {}. {}", i + 1, pattern);
            }
        }
        println!();

        let options = vec!["➕ Add pattern", "🗑️  Remove pattern", "⬅️  Back"];

        let choice = DialogSelect::new()
            .with_prompt("Select action")
            .items(&options)
            .default(0)
            .interact()?;

        match choice {
            0 => {
                // Add pattern
                let new_pattern: String = Input::new()
                    .with_prompt("Enter pattern (e.g., 'rm -rf *', 'sudo *')")
                    .interact_text()?;

                if !new_pattern.trim().is_empty() {
                    let profile = config.profiles.get_mut(profile_name).unwrap();
                    let perms = profile
                        .permissions
                        .get_or_insert_with(AgentPermissions::default);
                    let mut patterns = perms.forbidden_commands.clone().unwrap_or_default();
                    if !patterns.contains(&new_pattern) {
                        patterns.push(new_pattern.clone());
                        perms.forbidden_commands = Some(patterns);
                        config.save_default()?;
                        println!("\n✅ Added pattern: {}", new_pattern);
                    } else {
                        println!("\n⚠️ Pattern already exists");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
            1 => {
                // Remove pattern
                if current_patterns.is_empty() {
                    println!("\nNo patterns to remove");
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }

                let items: Vec<String> =
                    current_patterns.iter().map(|p| format!("{}", p)).collect();

                let selection = DialogSelect::new()
                    .with_prompt("Select pattern to remove")
                    .items(&items)
                    .interact()?;

                let profile = config.profiles.get_mut(profile_name).unwrap();
                let perms = profile
                    .permissions
                    .get_or_insert_with(AgentPermissions::default);
                let mut patterns = perms.forbidden_commands.clone().unwrap_or_default();
                let removed = patterns.remove(selection);
                perms.forbidden_commands = Some(patterns);
                config.save_default()?;
                println!("\n✅ Removed pattern: {}", removed);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            _ => break,
        }
    }

    Ok(true)
}

/// Toggle PaCoRe enabled
pub fn toggle_pacore_enabled(_config: &mut Config, _is_main: bool) -> Result<bool> {
    println!("\n[STUB] toggle_pacore_enabled - to be implemented\n");
    Ok(true)
}

/// Set PaCoRe rounds
pub fn set_pacore_rounds(_config: &mut Config, _is_main: bool) -> Result<bool> {
    println!("\n[STUB] set_pacore_rounds - to be implemented\n");
    Ok(true)
}

/// Set max actions before stall
pub fn set_max_actions_before_stall(config: &mut Config, is_main: bool) -> Result<bool> {
    let profile_name = if is_main { "default" } else { "worker" };

    // Ensure profile exists with permissions
    if !config.profiles.contains_key(profile_name) {
        let new_profile = ProfileConfig {
            provider: "openai".to_string(),
            permissions: Some(AgentPermissions::default()),
            ..ProfileConfig::default()
        };
        config
            .profiles
            .insert(profile_name.to_string(), new_profile);
    }

    // Get current value
    let profile = config.profiles.get(profile_name).unwrap();
    let permissions = profile.permissions.clone().unwrap_or_default();
    let current_value = if is_main {
        permissions.main_agent.max_actions_before_stall
    } else {
        permissions.worker.max_actions_before_stall
    };

    print!("\x1B[2J\x1B[1;1H");
    let title = if is_main { "Main LLM" } else { "Worker LLM" };
    println!(
        "\n{}",
        Style::new()
            .bold()
            .apply_to(format!("{} - Max Actions Before Stall", title))
    );
    println!(
        "{}",
        Style::new()
            .dim()
            .apply_to("Maximum number of actions before worker stalls (0 = no limit)")
    );
    println!("{}", Style::new().dim().apply_to("─".repeat(60)));
    println!(
        "Current value: {}",
        if current_value == 0 {
            "No limit".to_string()
        } else {
            current_value.to_string()
        }
    );

    let input: String = Input::new()
        .with_prompt("Enter max actions (0 for no limit)")
        .default(current_value.to_string())
        .interact_text()?;

    let new_value: usize = input.parse().unwrap_or(0);

    // Update config
    let profile = config.profiles.get_mut(profile_name).unwrap();
    let perms = profile
        .permissions
        .get_or_insert_with(AgentPermissions::default);
    if is_main {
        perms.main_agent.max_actions_before_stall = new_value;
    } else {
        perms.worker.max_actions_before_stall = new_value;
    }

    config.save_default()?;

    if new_value == 0 {
        println!("\n✅ Max actions before stall: No limit");
    } else {
        println!("\n✅ Max actions before stall set to {}", new_value);
    }
    std::thread::sleep(std::time::Duration::from_millis(800));
    Ok(true)
}
