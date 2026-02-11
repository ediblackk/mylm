//! Shared utilities for the Hub UI

use anyhow::Result;
use console::Style;
use mylm_core::config::Config;
use serde_json::Value;

/// Check if tmux is installed and available in PATH
pub fn is_tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok()
}

/// Fetch models from the API
pub async fn fetch_models(base_url: &str, api_key: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::new();

    let url = if base_url.ends_with('/') {
        format!("{}models", base_url)
    } else {
        format!("{}/models", base_url)
    };

    let mut request = client.get(&url);

    if !api_key.is_empty() && api_key != "none" {
        request = request.header("Authorization", format!("Bearer {}", api_key));
    }

    request = request.header("User-Agent", "mylm-cli/0.1.0");

    let response = request.send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("API request failed: {}", response.status()));
    }

    let body: Value = response.json().await?;

    let mut models = Vec::new();
    if let Some(data) = body.get("data").and_then(|v| v.as_array()) {
        for model in data {
            if let Some(id) = model.get("id").and_then(|v| v.as_str()) {
                models.push(id.to_string());
            }
        }
    }

    if models.is_empty() {
        if let Some(data) = body.get("models").and_then(|v| v.as_array()) {
            for model in data {
                if let Some(id) = model.get("id").and_then(|v| v.as_str()) {
                    models.push(id.to_string());
                }
            }
        }
    }

    models.sort();
    Ok(models)
}

/// Display compact configuration summary
pub fn display_config_summary(config: &Config) {
    let effective = config.resolve_profile();
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let dim = Style::new().dim();

    // Single line status bar
    let ws_status = if config.features.web_search.enabled {
        "🌐"
    } else {
        "·"
    };
    let tmux_status = if config.tmux_autostart { "🔄" } else { "·" };
    let pacore_status = if config.features.pacore.enabled {
        format!("⚡{}", config.features.pacore.rounds)
    } else {
        "·".to_string()
    };
    let rate_display = if effective.agent.iteration_rate_limit > 0 {
        format!("⏱️{}", effective.agent.iteration_rate_limit)
    } else {
        "·".to_string()
    };

    println!(
        "  {} {} {} {} {} {} {} {} {} {} {} {}",
        dim.apply_to("Iter:"),
        yellow.apply_to(format!("{}", effective.agent.max_iterations)),
        dim.apply_to("│"),
        rate_display,
        dim.apply_to("│ Web:"),
        ws_status,
        dim.apply_to("│ Tmux:"),
        tmux_status,
        dim.apply_to("│ PaCoRe:"),
        pacore_status,
        dim.apply_to("│ Key:"),
        if effective.api_key.is_some() {
            green.apply_to("✓")
        } else {
            Style::new().red().apply_to("✗")
        }
    );
    println!();
}

/// Print compact mylm dashboard header
pub async fn print_banner(_config: &Config) {
    let green = Style::new().green().bold();
    let cyan = Style::new().cyan();
    let dim = Style::new().dim();

    // Compact dashboard line - no extra newlines
    println!(
        "{} {}  {}  {}  {}",
        green.apply_to("mylm"),
        cyan.apply_to(format!("v{}", env!("CARGO_PKG_VERSION"))),
        dim.apply_to(format!("build {}", env!("BUILD_NUMBER"))),
        dim.apply_to(format!("{}", &env!("GIT_HASH")[..8.min(env!("GIT_HASH").len())])),
        dim.apply_to("Terminal AI")
    );
}
