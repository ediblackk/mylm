//! Model selection handlers for main and worker LLMs

use anyhow::Result;
use console::Style;
use dialoguer::Input;
use inquire::Select as InquireSelect;
use mylm_core::config::{Config, ProfileConfig};
use reqwest;
use serde_json;

/// Handle select main model
pub async fn handle_select_main_model(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!(
        "\n{}",
        Style::new().bold().apply_to("Select Main Model")
    );
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));

    // Step 1: Select Provider
    if config.providers.is_empty() {
        println!("\n❌ No providers configured. Add a provider first.");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        return Ok(false);
    }

    let mut provider_names: Vec<String> = config.providers.keys().cloned().collect();
    provider_names.sort();

    let ans = InquireSelect::new("Select provider:", provider_names.clone())
        .with_page_size(15)
        .with_help_message("↑↓ to navigate, type to filter, Enter to select")
        .prompt();

    let selected_provider = match ans {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };

    // Get provider config
    let provider_cfg = match config.providers.get(&selected_provider) {
        Some(p) => p.clone(),
        None => return Ok(false),
    };

    // Update active profile to use this provider
    {
        let profile_name = config.active_profile.clone();
        if let Some(profile) = config.profiles.get_mut(&profile_name) {
            profile.provider = selected_provider.clone();
        }
    }

    // Step 2: Fetch models from provider
    println!("\n🔄 Fetching models from {}...", selected_provider);

    let models =
        match fetch_models(&provider_cfg.base_url, &provider_cfg.api_key.clone().unwrap_or_default())
            .await
        {
            Ok(m) => {
                if m.is_empty() {
                    println!("   No models returned, using manual entry.");
                } else {
                    println!("   Found {} models", m.len());
                }
                m
            }
            Err(e) => {
                println!("   ⚠️  Could not fetch models: {}", e);
                println!("   Falling back to manual entry.");
                Vec::new()
            }
        };

    // Step 3: Select or enter model
    let current_model = config.active_profile().model.clone().unwrap_or_default();

    let selected_model = if models.is_empty() {
        // Manual entry
        let model: String = Input::new()
            .with_prompt("Enter model name")
            .default(current_model)
            .interact()?;
        model
    } else {
        // Select from list
        let ans = InquireSelect::new("Select model:", models)
            .with_page_size(15)
            .with_help_message("↑↓ to navigate, type to filter, Enter to select")
            .prompt();

        match ans {
            Ok(s) => s,
            Err(_) => return Ok(false),
        }
    };

    // Save to profile
    let profile_name = config.active_profile.clone();
    if let Some(profile) = config.profiles.get_mut(&profile_name) {
        profile.model = Some(selected_model.clone());
        // Mark as needing re-test since provider/model changed
        config.mark_profile_needs_test(&profile_name);
        config.save_default()?;
        println!(
            "\n✅ Main LLM set to: {} @ {}",
            selected_model, selected_provider
        );
        println!("   ⚠️  Run Test Connection to verify configuration");
    }

    std::thread::sleep(std::time::Duration::from_millis(800));
    Ok(true)
}

/// Handle select worker model
pub async fn handle_select_worker_model(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!(
        "\n{}",
        Style::new().bold().apply_to("Select Worker Model")
    );
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    println!(
        "{}",
        Style::new()
            .italic()
            .dim()
            .apply_to("Worker model can be from a different provider than main model\n")
    );

    // Step 1: Select Provider
    if config.providers.is_empty() {
        println!("\n❌ No providers configured. Add a provider first.");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        return Ok(false);
    }

    let mut provider_names: Vec<String> = config.providers.keys().cloned().collect();
    provider_names.sort();

    let ans = InquireSelect::new("Select provider for worker:", provider_names.clone())
        .with_page_size(15)
        .with_help_message("↑↓ to navigate, type to filter, Enter to select")
        .prompt();

    let selected_provider = match ans {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };

    // Get provider config
    let provider_cfg = match config.providers.get(&selected_provider) {
        Some(p) => p.clone(),
        None => return Ok(false),
    };

    // Step 2: Fetch models from provider
    println!("\n🔄 Fetching models from {}...", selected_provider);

    let models =
        match fetch_models(&provider_cfg.base_url, &provider_cfg.api_key.clone().unwrap_or_default())
            .await
        {
            Ok(m) => {
                if m.is_empty() {
                    println!("   No models returned, using manual entry.");
                } else {
                    println!("   Found {} models", m.len());
                }
                m
            }
            Err(e) => {
                println!("   ⚠️  Could not fetch models: {}", e);
                println!("   Falling back to manual entry.");
                Vec::new()
            }
        };

    // Step 3: Select or enter model
    let selected_model = if models.is_empty() {
        // Manual entry
        let model: String = Input::new()
            .with_prompt("Enter worker model name")
            .interact()?;
        model
    } else {
        // Select from list
        let ans = InquireSelect::new("Select worker model:", models)
            .with_page_size(15)
            .with_help_message("↑↓ to navigate, type to filter, Enter to select")
            .prompt();

        match ans {
            Ok(s) => s,
            Err(_) => return Ok(false),
        }
    };

    // For now, store worker model in a separate field or profile
    // We'll create a "worker" profile if it doesn't exist
    if !config.profiles.contains_key("worker") {
        let worker_profile = ProfileConfig {
            provider: selected_provider.clone(),
            model: Some(selected_model.clone()),
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
        config.profiles.insert("worker".to_string(), worker_profile);
    } else {
        if let Some(profile) = config.profiles.get_mut("worker") {
            profile.provider = selected_provider.clone();
            profile.model = Some(selected_model.clone());
        }
    }

    // Mark worker profile as needing re-test
    config.mark_profile_needs_test("worker");
    config.save_default()?;
    println!(
        "\n✅ Worker LLM set to: {} @ {}",
        selected_model, selected_provider
    );
    println!("   ⚠️  Run Test Connection to verify configuration");

    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    Ok(true)
}

/// Test connection for a profile (main or worker)
pub async fn test_profile_connection(config: &mut Config, profile_name: &str) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");

    let profile_label = if profile_name == "default" {
        "Main"
    } else {
        profile_name
    };
    println!(
        "\n{}",
        Style::new()
            .bold()
            .apply_to(format!("Test Connection - {} LLM", profile_label))
    );
    println!("{}", Style::new().dim().apply_to("─".repeat(50)));

    // Get profile
    let profile = match config.profiles.get(profile_name) {
        Some(p) => p.clone(),
        None => {
            println!("\n❌ Profile '{}' not found", profile_name);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            return Ok(false);
        }
    };

    // Get provider config
    let provider_cfg = match config.providers.get(&profile.provider) {
        Some(p) => p.clone(),
        None => {
            println!("\n❌ Provider '{}' not found", profile.provider);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            return Ok(false);
        }
    };

    println!("\n  Provider: {}", profile.provider);
    println!(
        "  Model: {}",
        profile.model.as_deref().unwrap_or("Not set")
    );
    println!("  Base URL: {}", provider_cfg.base_url);

    // Test 1: Check if API key is present (if needed)
    println!("\n🔄 Testing connection...");

    let api_key = provider_cfg.api_key.clone().unwrap_or_default();
    if api_key.is_empty()
        && !provider_cfg.base_url.contains("localhost")
        && !provider_cfg.base_url.contains("127.0.0.1")
    {
        println!("   ⚠️  No API key configured (may fail for cloud providers)");
    }

    // Test 2: Try to fetch models
    match fetch_models(&provider_cfg.base_url, &api_key).await {
        Ok(models) => {
            println!("   ✅ API endpoint reachable");
            println!("   ✅ Found {} models", models.len());

            // Test 3: Check if selected model exists in list
            if let Some(ref selected_model) = profile.model {
                if models.contains(selected_model) {
                    println!("   ✅ Selected model '{}' found", selected_model);
                } else if !models.is_empty() {
                    println!(
                        "   ⚠️  Selected model '{}' not in available models",
                        selected_model
                    );
                    println!(
                        "      Available: {:?}",
                        &models[..models.len().min(5)]
                    );
                }
            } else {
                println!("   ⚠️  No model selected");
            }

            // Mark as tested
            config.mark_profile_tested(profile_name);
            config.save_default()?;

            println!("\n✅ {} LLM configuration verified!", profile_label);
        }
        Err(e) => {
            let error_msg = format!("{}", e);
            println!("   ❌ Connection failed: {}", e);
            println!("\n⚠️  Check your:");
            println!("   - Base URL (should end with /v1 for OpenAI-compatible)");
            println!("   - API key");
            println!("   - Network connection");

            // Mark as tested with error
            config.mark_profile_test_failed(profile_name, error_msg);
            config.save_default()?;
        }
    }

    println!("\nPress Enter to continue...");
    let _ = std::io::stdin().read_line(&mut String::new());
    Ok(true)
}

/// Fetch models from the API
async fn fetch_models(base_url: &str, api_key: &str) -> Result<Vec<String>> {
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

    let body: serde_json::Value = response.json().await?;

    let mut models = Vec::new();
    if let Some(data) = body.get("data").and_then(|v| v.as_array()) {
        for model in data {
            if let Some(id) = model.get("id").and_then(|v| v.as_str()) {
                models.push(id.to_string());
            }
        }
    }

    // Fallback: try "models" key (some providers use this)
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
