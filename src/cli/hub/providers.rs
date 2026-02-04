//! Provider management - Add, edit, remove LLM providers

use anyhow::Result;
use console::Style;
use dialoguer::Password;
use inquire::{Select as InquireSelect, Text};
use mylm_core::config::{Config, Provider};

use crate::cli::hub::menus::ProviderMenuChoice;
use crate::cli::hub::utils::fetch_models;

/// Provider preset information
pub struct ProviderPreset {
    pub name: &'static str,
    pub display_name: &'static str,
    pub base_url: &'static str,
    pub provider_type: Provider,
    pub api_key_required: bool,
}

/// Get all available provider presets
pub fn get_provider_presets() -> Vec<ProviderPreset> {
    vec![
        ProviderPreset {
            name: "openai",
            display_name: "🟢 OpenAI",
            base_url: "https://api.openai.com/v1",
            provider_type: Provider::Openai,
            api_key_required: true,
        },
        ProviderPreset {
            name: "anthropic",
            display_name: "🟡 Anthropic",
            base_url: "https://api.anthropic.com/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "openrouter",
            display_name: "🔵 OpenRouter",
            base_url: "https://openrouter.ai/api/v1",
            provider_type: Provider::Openrouter,
            api_key_required: true,
        },
        ProviderPreset {
            name: "google",
            display_name: "🔴 Google Gemini",
            base_url: "https://generativelanguage.googleapis.com/v1beta",
            provider_type: Provider::Google,
            api_key_required: true,
        },
        ProviderPreset {
            name: "deepseek",
            display_name: "🟣 DeepSeek",
            base_url: "https://api.deepseek.com/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "mistral",
            display_name: "🟠 Mistral AI",
            base_url: "https://api.mistral.ai/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "cohere",
            display_name: "⚫ Cohere",
            base_url: "https://api.cohere.com/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "ai21",
            display_name: "⚪ AI21 Labs",
            base_url: "https://api.ai21.com/studio/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "groq",
            display_name: "🟤 Groq",
            base_url: "https://api.groq.com/openai/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "perplexity",
            display_name: "🔷 Perplexity",
            base_url: "https://api.perplexity.ai",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "together",
            display_name: "🔶 Together AI",
            base_url: "https://api.together.xyz/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "fireworks",
            display_name: "🎆 Fireworks AI",
            base_url: "https://api.fireworks.ai/inference/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "replicate",
            display_name: "🔄 Replicate",
            base_url: "https://api.replicate.com/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "moonshot",
            display_name: "🌙 Moonshot AI (Kimi)",
            base_url: "https://api.moonshot.cn/v1",
            provider_type: Provider::Kimi,
            api_key_required: true,
        },
        ProviderPreset {
            name: "zai",
            display_name: "🇨🇭 Z AI",
            base_url: "https://api.z.ai/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "minimax",
            display_name: "📊 MiniMax",
            base_url: "https://api.minimax.chat/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "cerebras",
            display_name: "🧠 Cerebras",
            base_url: "https://api.cerebras.ai/v1",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
        ProviderPreset {
            name: "ollama",
            display_name: "🏠 Ollama",
            base_url: "http://localhost:11434/v1",
            provider_type: Provider::Ollama,
            api_key_required: false,
        },
        ProviderPreset {
            name: "lmstudio",
            display_name: "💻 LM Studio",
            base_url: "http://localhost:1234/v1",
            provider_type: Provider::Custom,
            api_key_required: false,
        },
        ProviderPreset {
            name: "custom",
            display_name: "⚙️  Custom / Other",
            base_url: "",
            provider_type: Provider::Custom,
            api_key_required: true,
        },
    ]
}

/// Show provider management menu
pub fn show_provider_menu() -> Result<ProviderMenuChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let options = vec![
        ProviderMenuChoice::AddProvider,
        ProviderMenuChoice::EditProvider,
        ProviderMenuChoice::RemoveProvider,
        ProviderMenuChoice::Back,
    ];

    let ans: Result<ProviderMenuChoice, _> =
        InquireSelect::new("Manage Providers", options).prompt();

    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(ProviderMenuChoice::Back),
    }
}

/// Handle adding a new provider
pub async fn handle_add_provider(config: &mut Config) -> Result<bool> {
    println!("\n🔌 Add New Provider");
    println!("{}", Style::new().blue().bold().apply_to("-".repeat(50)));

    // Get provider presets
    let presets = get_provider_presets();
    let preset_names: Vec<&str> = presets.iter().map(|p| p.display_name).collect();

    // Let user select from presets
    let selected_preset = InquireSelect::new("Select provider:", preset_names).prompt()?;

    // Find the preset
    let preset = presets.iter().find(|p| p.display_name == selected_preset).unwrap();

    // Check if already exists
    if config.providers.contains_key(preset.name) {
        println!(
            "⚠️  Provider '{}' already exists. Use Edit to modify it.",
            preset.name
        );
        return Ok(false);
    }

    // Get base URL (use preset default or allow custom)
    let base_url = if preset.name == "custom" {
        Text::new("Base URL:").prompt()?
    } else {
        let url = Text::new("Base URL:")
            .with_initial_value(preset.base_url)
            .prompt()?;
        url
    };

    // Get API key
    let api_key_prompt = if preset.api_key_required {
        "API Key:"
    } else {
        "API Key (optional for local):"
    };

    let api_key = Password::new()
        .with_prompt(api_key_prompt)
        .allow_empty_password(!preset.api_key_required)
        .interact()?;

    // Create provider config
    let provider_config = mylm_core::config::v2::ProviderConfig {
        provider_type: preset.provider_type.clone(),
        base_url,
        api_key: if api_key.is_empty() {
            None
        } else {
            Some(api_key)
        },
        timeout_secs: 30,
    };

    config.providers.insert(preset.name.to_string(), provider_config);

    // If this is the first provider, make it active
    if config.providers.len() == 1 {
        config.active_provider = preset.name.to_string();
    }

    println!("✅ Provider '{}' added successfully!", preset.name);
    config.save_to_default_location()?;
    Ok(true)
}

/// Handle editing a provider
pub async fn handle_edit_provider(config: &mut Config) -> Result<bool> {
    if config.providers.is_empty() {
        println!("⚠️  No providers configured. Add one first.");
        return Ok(false);
    }

    // Select provider to edit
    let provider_names: Vec<String> = config.providers.keys().cloned().collect();
    let name = InquireSelect::new("Select provider to edit:", provider_names).prompt()?;

    let provider_config = config.providers.get(&name).cloned().unwrap();

    println!("\n✏️  Editing Provider: {}", name);
    println!(
        "{}",
        Style::new().blue().bold().apply_to("-".repeat(50))
    );
    println!(
        "Current: {} @ {}",
        format!("{:?}", provider_config.provider_type),
        provider_config.base_url
    );
    println!();

    let options = vec!["Change Base URL", "Change API Key", "Test Connection", "Back"];

    loop {
        let choice = InquireSelect::new("What to edit:", options.clone()).prompt()?;

        match choice {
            "Change Base URL" => {
                let new_url = Text::new("Base URL:")
                    .with_initial_value(&provider_config.base_url)
                    .prompt()?;
                if let Some(cfg) = config.providers.get_mut(&name) {
                    cfg.base_url = new_url;
                }
                config.save_to_default_location()?;
                println!("✅ Base URL updated!");
            }
            "Change API Key" => {
                let new_key = Password::new()
                    .with_prompt("New API Key (empty to remove)")
                    .allow_empty_password(true)
                    .interact()?;
                if let Some(cfg) = config.providers.get_mut(&name) {
                    cfg.api_key = if new_key.is_empty() {
                        None
                    } else {
                        Some(new_key)
                    };
                }
                config.save_to_default_location()?;
                println!("✅ API Key updated!");
            }
            "Test Connection" => {
                let cfg = config.providers.get(&name).unwrap();
                println!("🔄 Testing connection to {}...", cfg.base_url);
                match fetch_models(&cfg.base_url, &cfg.api_key.clone().unwrap_or_default()).await {
                    Ok(models) => println!("✅ Success! Found {} models.", models.len()),
                    Err(e) => println!("❌ Failed: {}", e),
                }
            }
            _ => break,
        }
    }

    Ok(true)
}

/// Handle removing a provider
pub fn handle_remove_provider(config: &mut Config) -> Result<bool> {
    if config.providers.is_empty() {
        println!("⚠️  No providers to remove.");
        return Ok(false);
    }

    let provider_names: Vec<String> = config.providers.keys().cloned().collect();
    let name = InquireSelect::new("Select provider to remove:", provider_names).prompt()?;

    // Don't allow removing the active provider
    if name == config.active_provider {
        println!("⚠️  Cannot remove the active provider. Switch to another provider first.");
        return Ok(false);
    }

    config.providers.remove(&name);
    println!("✅ Provider '{}' removed.", name);
    config.save_to_default_location()?;
    Ok(true)
}
