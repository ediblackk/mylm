//! Display functions for hub - banners, menu rendering

use console::Style;
use mylm_core::config::Config;

/// Check if tmux is available
pub fn is_tmux_available() -> bool {
    which::which("tmux").is_ok()
}

/// Check if session file exists
pub fn session_exists() -> bool {
    dirs::data_dir()
        .map(|d| d.join("mylm").join("sessions").join("latest.json"))
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Print hub banner
pub fn print_hub_banner() {
    // Clear screen to prevent leftover content from previous menus
    print!("\x1B[2J\x1B[1;1H");

    let blue = Style::new().blue().bold();
    let dim = Style::new().dim();
    let cyan = Style::new().cyan();

    // Build info from build.rs
    let build_number = env!("BUILD_NUMBER");
    let git_hash = env!("GIT_HASH");

    println!();
    println!(
        "  {} {}  {} {}",
        blue.apply_to("◉ mylm"),
        dim.apply_to("v3"),
        cyan.apply_to(format!("(build {})", build_number)),
        dim.apply_to(format!("[{}]", git_hash))
    );
    println!("  {}", dim.apply_to("Terminal AI Assistant"));
    println!();
}

/// Print config banner
pub fn print_config_banner(config: &Config) {
    print!("\x1B[2J\x1B[1;1H");

    let blue = Style::new().blue().bold();
    let dim = Style::new().dim();
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let red = Style::new().red();

    // Helper to format test status with optional error
    let format_test_status = |config: &Config, profile_name: &str| -> Vec<String> {
        let mut lines = Vec::new();
        if let Some((tested, has_error, error_msg)) = config.get_profile_test_status(profile_name) {
            if tested && !has_error {
                lines.push(format!("     Status: {}", green.apply_to("✅ Working")));
            } else if tested && has_error {
                lines.push(format!("     Status: {}", red.apply_to("❌ Error")));
                if let Some(msg) = error_msg {
                    // Truncate long error messages
                    let display_msg = if msg.len() > 50 {
                        format!("{}...", &msg[..50])
                    } else {
                        msg.to_string()
                    };
                    lines.push(format!("     Error: {}", red.apply_to(display_msg)));
                }
            } else {
                lines.push(format!("     Status: {}", yellow.apply_to("⚠️ Untested")));
            }
        } else {
            lines.push(format!("     Status: {}", red.apply_to("❌ Not Configured")));
        }
        lines
    };

    println!();
    println!(
        "  {} {}",
        blue.apply_to("⚙️  Configuration"),
        dim.apply_to("─".repeat(50))
    );

    // === MAIN LLM ===
    let main_profile = config.active_profile();
    let main_provider = &main_profile.provider;
    let main_model = main_profile
        .model
        .clone()
        .unwrap_or_else(|| "Not set".to_string());

    println!();
    println!(
        "  {} {}",
        yellow.apply_to("🧠 Main LLM"),
        dim.apply_to("─".repeat(40))
    );
    println!("     Provider: {}", green.apply_to(main_provider));
    println!("     Model: {}", green.apply_to(&main_model));
    for line in format_test_status(config, &config.active_profile) {
        println!("{}", line);
    }
    println!(
        "     Context: {} tokens",
        green.apply_to(main_profile.context_window)
    );
    if main_profile.condense_threshold.unwrap_or(0) > 0 {
        println!(
            "     Condense: {} tokens",
            green.apply_to(main_profile.condense_threshold.unwrap())
        );
    }
    if main_profile.rate_limit_rpm > 0 {
        println!(
            "     Rate Limit: {} RPM",
            green.apply_to(main_profile.rate_limit_rpm)
        );
    }
    if let Some(price) = main_profile.input_price {
        if price > 0.0 {
            println!(
                "     Cost: ${}/1M in, ${}/1M out",
                green.apply_to(price),
                green.apply_to(main_profile.output_price.unwrap_or(0.0))
            );
        }
    }

    // === WORKER LLM ===
    if let Some(worker) = config.profiles.get("worker") {
        println!();
        println!(
            "  {} {}",
            yellow.apply_to("⚡ Worker LLM"),
            dim.apply_to("─".repeat(40))
        );
        println!("     Provider: {}", green.apply_to(&worker.provider));
        println!(
            "     Model: {}",
            green.apply_to(worker.model.clone().unwrap_or_else(|| "Not set".to_string()))
        );
        for line in format_test_status(config, "worker") {
            println!("{}", line);
        }
        println!(
            "     Context: {} tokens",
            green.apply_to(worker.context_window)
        );
        if worker.condense_threshold.unwrap_or(0) > 0 {
            println!(
                "     Condense: {} tokens",
                green.apply_to(worker.condense_threshold.unwrap())
            );
        }
        if worker.rate_limit_rpm > 0 {
            println!(
                "     Rate Limit: {} RPM",
                green.apply_to(worker.rate_limit_rpm)
            );
        }
    }

    // === WEB SEARCH ===
    println!();
    println!(
        "  {} {}",
        yellow.apply_to("🌐 Web Search"),
        dim.apply_to("─".repeat(40))
    );
    let web_search = if config.features.web_search {
        green.apply_to("Enabled").to_string()
    } else {
        dim.apply_to("Disabled").to_string()
    };
    println!("     Status: {}", web_search);

    println!();
}
