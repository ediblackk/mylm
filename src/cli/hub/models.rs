//! Model selection - Main LLM and worker model configuration

use anyhow::Result;
use console::Style;
use inquire::{Select as InquireSelect, Text};
use mylm_core::config::Config;

use crate::cli::hub::utils::fetch_models;

/// Select main LLM model - first choose provider, then model
pub async fn handle_select_main_model(config: &mut Config) -> Result<bool> {
    println!("\n🧠 Select Main LLM");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );

    // Step 1: Select Provider
    if config.providers.is_empty() {
        println!("⚠️  No providers configured. Add a provider first.");
        return Ok(false);
    }

    let provider_names: Vec<String> = config.providers.keys().cloned().collect();
    let selected_provider =
        InquireSelect::new("Select provider:", provider_names).prompt()?;

    // Make it the active provider
    config.active_provider = selected_provider.clone();

    // Update legacy endpoint for compatibility
    if let Some(provider_cfg) = config.providers.get(&selected_provider) {
        config.endpoint.provider = provider_cfg.provider_type.clone();
        config.endpoint.base_url = Some(provider_cfg.base_url.clone());
        config.endpoint.api_key = provider_cfg.api_key.clone();
    }

    // Step 2: Select Model from this provider
    println!("\n🔄 Fetching models from {}...", selected_provider);

    let provider_cfg = config.providers.get(&selected_provider).unwrap();
    let models = match fetch_models(
        &provider_cfg.base_url,
        &provider_cfg.api_key.clone().unwrap_or_default(),
    )
    .await
    {
        Ok(m) => m,
        Err(e) => {
            println!("⚠️  Could not fetch models: {}", e);
            println!("   Falling back to manual entry.");
            Vec::new()
        }
    };

    let selected_model = if models.is_empty() {
        Text::new("Model name:")
            .with_initial_value(&config.endpoint.model)
            .prompt()?
    } else {
        if models.len() > 20 {
            println!("   (Type to search through {} models)", models.len());
        }

        let initial = models
            .iter()
            .position(|m| m == &config.endpoint.model)
            .unwrap_or(0);

        InquireSelect::new("Select model:", models)
            .with_starting_cursor(initial)
            .prompt()?
    };

    config.endpoint.model = selected_model.clone();

    // Model Metadata prompts
    println!("\n📊 Model Metadata (Optional, press Enter to skip)");

    let max_ctx = Text::new("Max context tokens:")
        .with_help_message("e.g. 128000")
        .prompt()?;
    if !max_ctx.trim().is_empty() {
        if let Ok(val) = max_ctx.trim().parse::<usize>() {
            config.endpoint.max_context_tokens = Some(val);
        }
    }

    let in_price = Text::new("Input price (per 1M tokens):")
        .with_help_message("e.g. 0.15")
        .prompt()?;
    if !in_price.trim().is_empty() {
        if let Ok(val) = in_price.trim().parse::<f64>() {
            config.endpoint.input_price = Some(val);
        }
    }

    let out_price = Text::new("Output price (per 1M tokens):")
        .with_help_message("e.g. 0.60")
        .prompt()?;
    if !out_price.trim().is_empty() {
        if let Ok(val) = out_price.trim().parse::<f64>() {
            config.endpoint.output_price = Some(val);
        }
    }

    let threshold = Text::new("Condensation threshold:")
        .with_help_message("Tokens to trigger summary (e.g. 100000)")
        .prompt()?;
    if !threshold.trim().is_empty() {
        if let Ok(val) = threshold.trim().parse::<usize>() {
            config.endpoint.condensation_threshold = Some(val);
        }
    }

    config.save_to_default_location()?;

    println!(
        "✅ Main LLM set to: {} @ {}",
        selected_model, selected_provider
    );
    Ok(true)
}

/// Select worker model - can be from different provider than main
pub async fn handle_select_worker_model(config: &mut Config) -> Result<bool> {
    println!("\n⚡ Select Worker Model");
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!("Worker model handles sub-tasks and simpler operations.");
    println!("Can be from same or different provider than main LLM.");
    println!();

    if config.providers.is_empty() {
        println!("⚠️  No providers configured. Add a provider first.");
        return Ok(false);
    }

    // Show current setting
    let resolved = config.resolve_profile();
    let current_worker_model = if resolved.agent.worker_model == resolved.agent.main_model {
        format!("Same as Main ({})", resolved.agent.main_model)
    } else {
        resolved.agent.worker_model.clone()
    };
    println!("📌 Current worker model: {}", 
        Style::new().green().apply_to(&current_worker_model)
    );
    println!();

    // Step 1: Select Provider for worker (with cancel option)
    let provider_names: Vec<String> = config.providers.keys().cloned().collect();
    let selected_provider = match InquireSelect::new("Select provider for worker:", provider_names)
        .prompt() 
    {
        Ok(p) => p,
        Err(inquire::InquireError::OperationCanceled) | Err(inquire::InquireError::OperationInterrupted) => {
            println!("↩️  Cancelled - keeping current setting.");
            return Ok(false);
        }
        Err(e) => return Err(e.into()),
    };

    // Step 2: Select Model from this provider
    let provider_cfg = config.providers.get(&selected_provider).unwrap();

    println!("🔄 Fetching models from {}...", selected_provider);
    let mut models = match fetch_models(
        &provider_cfg.base_url,
        &provider_cfg.api_key.clone().unwrap_or_default(),
    )
    .await
    {
        Ok(m) => m,
        Err(_) => Vec::new(),
    };

    // Add "Same as Main LLM" option at the top
    let same_as_main = format!("🔄 Same as Main ({})", config.endpoint.model);
    models.insert(0, same_as_main.clone());
    
    // Add Cancel option
    let cancel_option = "❌ Cancel (Go Back)".to_string();
    models.push(cancel_option.clone());

    let selected = match InquireSelect::new("Select worker model:", models).prompt() {
        Ok(m) => m,
        Err(inquire::InquireError::OperationCanceled) | Err(inquire::InquireError::OperationInterrupted) => {
            println!("↩️  Cancelled - keeping current setting.");
            return Ok(false);
        }
        Err(e) => return Err(e.into()),
    };

    // Handle cancel selection
    if selected == cancel_option {
        println!("↩️  Cancelled - keeping current setting.");
        return Ok(false);
    }

    let worker_model = if selected == same_as_main {
        None // Use main model
    } else {
        Some(format!("{}/{}", selected_provider, selected))
    };

    // Update profile with worker model
    let profile = config.profiles.entry(config.profile.clone()).or_default();
    let current_agent = profile.agent.clone().unwrap_or_default();

    let mut max_ctx = None;
    let mut in_price = None;
    let mut out_price = None;
    let mut threshold = None;

    // Model Metadata prompts
    println!("\n📊 Worker Model Metadata (Optional, press Enter to skip)");

    let input = Text::new("Max context tokens:")
        .with_help_message("e.g. 128000")
        .prompt()?;
    if !input.trim().is_empty() {
        if let Ok(val) = input.trim().parse::<usize>() {
            max_ctx = Some(val);
        }
    }

    let input = Text::new("Input price (per 1M tokens):")
        .with_help_message("e.g. 0.15")
        .prompt()?;
    if !input.trim().is_empty() {
        if let Ok(val) = input.trim().parse::<f64>() {
            in_price = Some(val);
        }
    }

    let input = Text::new("Output price (per 1M tokens):")
        .with_help_message("e.g. 0.60")
        .prompt()?;
    if !input.trim().is_empty() {
        if let Ok(val) = input.trim().parse::<f64>() {
            out_price = Some(val);
        }
    }

    let input = Text::new("Condensation threshold:")
        .with_help_message("Tokens to trigger summary (e.g. 100000)")
        .prompt()?;
    if !input.trim().is_empty() {
        if let Ok(val) = input.trim().parse::<usize>() {
            threshold = Some(val);
        }
    }

    profile.agent = Some(mylm_core::config::AgentOverride {
        max_iterations: current_agent.max_iterations,
        iteration_rate_limit: current_agent.iteration_rate_limit,
        main_model: current_agent.main_model,
        worker_model: worker_model.clone(),
        max_context_tokens: max_ctx,
        input_price: in_price,
        output_price: out_price,
        condensation_threshold: threshold,
        permissions: None,
        main_rpm: None,
        workers_rpm: None,
        worker_limit: None,
        rate_limit_tier: None,
        max_actions_before_stall: None,
        max_consecutive_messages: None,
        max_recovery_attempts: None,
        max_tool_failures: None,
    });

    config.save_to_default_location()?;

    match worker_model {
        Some(m) => println!("✅ Worker model set to: {}", m),
        None => println!("✅ Worker model set to use Main LLM"),
    }
    Ok(true)
}
