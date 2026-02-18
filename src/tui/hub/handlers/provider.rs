//! Provider management handlers

use anyhow::Result;
use console::Style;
use dialoguer::{Confirm, Input, Password};
use inquire::Select as InquireSelect;
use mylm_core::config::{Config, ProfileConfig, ProviderConfig, ProviderType};

/// Handle add provider
pub async fn handle_add_provider(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!(
        "\n{}",
        Style::new().bold().apply_to("Add LLM Provider")
    );
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));
    println!(
        "{}\n",
        Style::new()
            .italic()
            .dim()
            .apply_to("Tip: Type to filter the list, ↑/↓ to navigate, Enter to select")
    );

    // Provider presets
    let presets = vec![
        // === GENERIC / LOCAL OPTIONS FIRST ===
        ("OpenAI Compatible (Custom)", "", ProviderType::Custom, 0),
        ("Ollama (Local Models)", "http://localhost:11434/v1", ProviderType::Ollama, 0),
        ("LM Studio (Local)", "http://localhost:1234/v1", ProviderType::Custom, 0),
        // === CLOUD PROVIDERS BY AI SCORE ===
        // Tier 1: Best AI Score (48-53) - Elite Providers
        (
            "Anthropic (Claude)",
            "https://api.anthropic.com/v1",
            ProviderType::Custom,
            53,
        ),
        ("OpenAI", "https://api.openai.com/v1", ProviderType::OpenAi, 51),
        (
            "Google Gemini",
            "https://generativelanguage.googleapis.com/v1beta",
            ProviderType::Google,
            50,
        ),
        // Tier 2: High AI Score (40-47) - Excellent Providers
        (
            "Moonshot (Kimi)",
            "https://api.moonshot.cn/v1",
            ProviderType::Kimi,
            47,
        ),
        (
            "DeepSeek",
            "https://api.deepseek.com/v1",
            ProviderType::Custom,
            42,
        ),
        (
            "MiniMax",
            "https://api.minimax.chat/v1",
            ProviderType::Custom,
            42,
        ),
        ("xAI (Grok)", "https://api.x.ai/v1", ProviderType::Custom, 41),
        ("AWS Bedrock", "", ProviderType::Custom, 36),
        // Tier 3: Good AI Score (25-35) - Solid Providers
        (
            "Alibaba Qwen",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            ProviderType::Custom,
            32,
        ),
        ("Mistral", "https://api.mistral.ai/v1", ProviderType::Custom, 23),
        // Routers / Aggregators
        (
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            ProviderType::OpenRouter,
            0,
        ),
        (
            "Together AI",
            "https://api.together.xyz/v1",
            ProviderType::Custom,
            0,
        ),
        (
            "Fireworks",
            "https://api.fireworks.ai/inference/v1",
            ProviderType::Custom,
            0,
        ),
        ("Groq", "https://api.groq.com/openai/v1", ProviderType::OpenAi, 0),
        // Other Notable Providers
        ("Cerebras", "https://api.cerebras.ai/v1", ProviderType::Custom, 0),
        ("SambaNova", "https://api.sambanova.ai/v1", ProviderType::Custom, 0),
        ("Azure OpenAI", "", ProviderType::Custom, 0),
        ("GCP Vertex AI", "", ProviderType::Custom, 0),
        // Chinese Providers
        (
            "Doubao",
            "https://ark.cn-beijing.volces.com/api/v3",
            ProviderType::Custom,
            0,
        ),
        // Other Routers
        ("Vercel AI Gateway", "", ProviderType::Custom, 0),
        ("LiteLLM", "", ProviderType::Custom, 0),
        (
            "Hugging Face",
            "https://api-inference.huggingface.co",
            ProviderType::Custom,
            0,
        ),
        ("Unbound", "https://api.unbound.com/v1", ProviderType::Custom, 0),
        ("Requesty", "https://router.requesty.ai/v1", ProviderType::Custom, 0),
        (
            "DeepInfra",
            "https://api.deepinfra.com/v1/openai",
            ProviderType::OpenAi,
            0,
        ),
        ("Baseten", "https://app.baseten.co/v1", ProviderType::Custom, 0),
        (
            "Featherless",
            "https://api.featherless.ai/v1",
            ProviderType::Custom,
            0,
        ),
    ];

    let preset_names: Vec<&str> = presets.iter().map(|(name, _, _, _)| *name).collect();

    // Use inquire's Select which has built-in filtering
    let ans = InquireSelect::new(
        "Search or select provider (type to filter, ESC to cancel):",
        preset_names,
    )
    .with_page_size(15)
    .with_help_message("↑↓ to navigate, type to filter, Enter to select")
    .prompt();

    // Handle cancellation
    let selection = match ans {
        Ok(s) => {
            // Find the index of the selected item
            match presets.iter().position(|(name, _, _, _)| *name == s) {
                Some(idx) => idx,
                None => return Ok(false), // Not found (shouldn't happen)
            }
        }
        Err(_) => return Ok(false), // User cancelled with ESC
    };

    let (preset_name, preset_url, provider_type, _) = &presets[selection];

    // Get provider name
    let name: String = Input::new()
        .with_prompt("Provider name (for reference)")
        .default(preset_name.to_string())
        .interact()?;

    // Get base URL
    let base_url: String = if !preset_url.is_empty() {
        Input::new()
            .with_prompt("Base URL")
            .default(preset_url.to_string())
            .interact()?
    } else {
        Input::new().with_prompt("Base URL").interact()?
    };

    // Get API key (optional for local providers)
    let is_local = preset_name.contains("Local")
        || preset_name.contains("Ollama")
        || preset_name.contains("LM Studio");
    let prompt_text = if is_local {
        "API key (optional for local providers)"
    } else {
        "API key"
    };

    let api_key: String = Password::new()
        .with_prompt(prompt_text)
        .allow_empty_password(true)
        .interact()?;

    let api_key = if api_key.is_empty() { None } else { Some(api_key) };

    // Create provider config (use placeholder model - user selects later)
    let provider_config = ProviderConfig {
        provider_type: provider_type.clone(),
        base_url,
        api_key,
        default_model: "default".to_string(),
        models: vec![],
        timeout_secs: 120,
    };

    // Add to config
    config.providers.insert(name.clone(), provider_config);

    // If this is the first provider, set it as active
    if config.providers.len() == 1 {
        config.active_profile = "default".to_string();
        let profile = ProfileConfig {
            provider: name.clone(),
            model: None, // User will select model later
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
        config.profiles.insert("default".to_string(), profile);
    }

    // Save config
    config.save_default()?;

    println!("\n✅ Provider '{}' added successfully!", name);
    println!(
        "   Base URL: {}",
        config.providers.get(&name).unwrap().base_url
    );
    println!("   You can select a model from this provider in 'Main LLM Settings'");

    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    Ok(true)
}

/// Handle edit provider
pub async fn handle_edit_provider(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Edit Provider"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));

    if config.providers.is_empty() {
        println!("\n❌ No providers configured.");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        return Ok(false);
    }

    let mut provider_names: Vec<String> = config.providers.keys().cloned().collect();
    provider_names.sort();
    provider_names.push("⬅️  Back".to_string());

    let ans = InquireSelect::new("Select provider to edit:", provider_names.clone())
        .with_page_size(15)
        .with_help_message("↑↓ to navigate, type to filter, Enter to select")
        .prompt();

    // Handle cancellation or back
    let selection = match ans {
        Ok(s) => {
            if s == "⬅️  Back" {
                return Ok(false);
            }
            match provider_names.iter().position(|name| *name == s) {
                Some(idx) => idx,
                None => return Ok(false),
            }
        }
        Err(_) => return Ok(false),
    };

    let name = provider_names[selection].clone();
    let provider = config.providers.get(&name).cloned();

    if let Some(mut provider) = provider {
        // Edit base URL
        provider.base_url = Input::new()
            .with_prompt("Base URL")
            .default(provider.base_url)
            .interact()?;

        // Edit API key
        let new_key: String = Password::new()
            .with_prompt("API key (leave empty to keep current)")
            .allow_empty_password(true)
            .interact()?;
        if !new_key.is_empty() {
            provider.api_key = Some(new_key);
        }

        // Edit default model
        provider.default_model = Input::new()
            .with_prompt("Default model")
            .default(provider.default_model)
            .interact()?;

        // Update provider
        config.providers.insert(name.clone(), provider);
        config.save_default()?;

        println!("\n✅ Provider '{}' updated!", name);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    Ok(true)
}

/// Handle remove provider
pub fn handle_remove_provider(config: &mut Config) -> Result<bool> {
    print!("\x1B[2J\x1B[1;1H");
    println!("\n{}", Style::new().bold().apply_to("Remove Provider"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));

    if config.providers.is_empty() {
        println!("\n❌ No providers configured.");
        return Ok(false);
    }

    let mut provider_names: Vec<String> = config.providers.keys().cloned().collect();
    provider_names.sort();
    provider_names.push("⬅️  Back".to_string());

    let ans = InquireSelect::new("Select provider to remove:", provider_names.clone())
        .with_page_size(15)
        .with_help_message("↑↓ to navigate, type to filter, Enter to select")
        .prompt();

    // Handle cancellation or back
    let selection = match ans {
        Ok(s) => {
            if s == "⬅️  Back" {
                return Ok(false);
            }
            match provider_names.iter().position(|name| *name == s) {
                Some(idx) => idx,
                None => return Ok(false),
            }
        }
        Err(_) => return Ok(false),
    };

    let name = provider_names[selection].clone();

    if Confirm::new()
        .with_prompt(format!("Are you sure you want to remove '{}'?", name))
        .default(false)
        .interact()?
    {
        config.providers.remove(&name);
        config.save_default()?;
        println!("\n✅ Provider '{}' removed!", name);
    } else {
        println!("\nCancelled.");
    }

    Ok(true)
}
