//! Web search settings menu

use anyhow::Result;
use console::Style;
use dialoguer::{Input, Password, Select};
use mylm_core::config::{Config, SearchProvider};

/// Web search settings loop
pub async fn run(config: &mut Config) -> Result<()> {
    // Sync features.web_search with profile.web_search.enabled on entry
    let profile_enabled = config.active_profile().web_search.enabled;
    if config.features.web_search != profile_enabled {
        log::debug!(
            "[CONFIG] Syncing web_search: features={} profile={}",
            config.features.web_search,
            profile_enabled
        );
        config.features.web_search = profile_enabled;
    }

    handle_web_search_settings(config).await
}

async fn handle_web_search_settings(config: &mut Config) -> Result<()> {
    loop {
        print!("\x1B[2J\x1B[1;1H");
        println!("\n{}", Style::new().bold().apply_to("Web Search Settings"));
        println!("{}", Style::new().dim().apply_to("─".repeat(40)));

        // Get current web search config from active profile
        let web_search = &config.active_profile().web_search;

        println!(
            "Status: {}",
            if web_search.enabled {
                "✅ Enabled"
            } else {
                "❌ Disabled"
            }
        );
        println!("Provider: {:?}\n", web_search.provider);

        let has_extra_params = web_search
            .extra_params
            .as_ref()
            .map(|p| !p.is_empty())
            .unwrap_or(false);

        let choices = vec![
            "Toggle Web Search",
            "Select Provider",
            "Set API Key",
            if has_extra_params {
                "⚙️  Configure Extra Parameters"
            } else {
                "Configure Extra Parameters"
            },
            "Back",
        ];

        let selection = Select::new()
            .with_prompt("Select option")
            .items(&choices)
            .default(0)
            .interact()?;

        match selection {
            0 => {
                // Toggle web search - sync both config.features and profile.web_search
                let new_status = {
                    let profile = config.active_profile_mut();
                    profile.web_search.enabled = !profile.web_search.enabled;
                    profile.web_search.enabled
                };
                // Sync with global features flag
                config.features.web_search = new_status;
                config.save_default()?;

                log::info!(
                    "[CONFIG] Web search {} for profile '{}'",
                    if new_status { "enabled" } else { "disabled" },
                    config.active_profile
                );

                println!(
                    "\n✅ Web search {}",
                    if new_status { "enabled" } else { "disabled" }
                );
                println!("   Profile: {}", config.active_profile);
                println!(
                    "   Provider: {:?}",
                    config.active_profile().web_search.provider
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            }
            1 => {
                // Select provider
                let providers = vec![
                    ("DuckDuckGo (Free, no API key)", SearchProvider::DuckDuckGo),
                    ("SerpApi (Google/Bing results)", SearchProvider::Serpapi),
                    ("Brave Search", SearchProvider::Brave),
                    ("OpenAI", SearchProvider::Openai),
                    ("Exa (Neural Search)", SearchProvider::Exa),
                    ("Google Custom Search", SearchProvider::Google),
                    ("Tavily (AI-native)", SearchProvider::Tavily),
                    ("Kimi (Moonshot AI)", SearchProvider::Kimi),
                    ("Custom", SearchProvider::Custom),
                ];

                let provider_names: Vec<&str> =
                    providers.iter().map(|(name, _)| *name).collect();

                let provider_selection = Select::new()
                    .with_prompt("Select search provider")
                    .items(&provider_names)
                    .default(0)
                    .interact()?;

                let selected_provider = providers[provider_selection].1.clone();
                {
                    let profile = config.active_profile_mut();
                    profile.web_search.provider = selected_provider;
                }
                config.save_default()?;

                println!(
                    "\n✅ Provider set to: {}",
                    provider_names[provider_selection]
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
            }
            2 => {
                // Set API key
                let api_key = Password::new()
                    .with_prompt("Enter API key (or leave empty to use env var)")
                    .allow_empty_password(true)
                    .interact()?;

                let provider = config.active_profile().web_search.provider.clone();

                {
                    let profile = config.active_profile_mut();
                    if api_key.is_empty() {
                        profile.web_search.api_key = None;
                        config.save_default()?;
                        log::info!(
                            "[CONFIG] Web search API key cleared for provider {:?}",
                            provider
                        );
                        println!("\n✅ API key cleared (will use environment variable)");
                        tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                    } else {
                        profile.web_search.api_key = Some(api_key.clone());
                        config.save_default()?;
                        log::info!(
                            "[CONFIG] Web search API key set for provider {:?}",
                            provider
                        );
                        println!("\n✅ API key set");

                        // Test the API key if provider requires it
                        if provider != SearchProvider::DuckDuckGo {
                            println!("   Testing API key...");
                            match test_web_search_api_key(&provider, &api_key).await {
                                Ok(()) => {
                                    log::info!(
                                        "[CONFIG] Web search API key test passed for {:?}",
                                        provider
                                    );
                                    println!("   ✅ API key is valid!");
                                }
                                Err(e) => {
                                    log::warn!(
                                        "[CONFIG] Web search API key test failed for {:?}: {}",
                                        provider,
                                        e
                                    );
                                    println!("   ⚠️  API key test failed: {}", e);
                                    println!("   The key was saved, but may not work correctly.");
                                }
                            }
                        } else {
                            println!("   ℹ️  DuckDuckGo doesn't require an API key");
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                    }
                }
            }
            3 => {
                // Configure extra parameters
                handle_web_search_extra_params(config).await?;
            }
            _ => return Ok(()),
        }
    }
}

async fn handle_web_search_extra_params(config: &mut Config) -> Result<()> {
    use std::collections::HashMap;

    let provider = config.active_profile().web_search.provider.clone();

    // Define which parameters are supported by each provider
    let supported_params: HashMap<&str, Vec<SearchProvider>> = [
        ("type", vec![SearchProvider::Exa]),
        ("numResults", vec![SearchProvider::Exa, SearchProvider::Serpapi]),
        ("category", vec![SearchProvider::Exa]),
        ("maxAgeHours", vec![SearchProvider::Exa]),
        ("includeDomains", vec![SearchProvider::Exa]),
        ("excludeDomains", vec![SearchProvider::Exa]),
        ("contents.text", vec![SearchProvider::Exa]),
        (
            "contents.highlights.maxCharacters",
            vec![SearchProvider::Exa],
        ),
    ]
    .into_iter()
    .collect();

    let is_supported = |param: &str| -> bool {
        supported_params
            .get(param)
            .map(|providers| providers.contains(&provider))
            .unwrap_or(false)
    };

    loop {
        print!("\x1B[2J\x1B[1;1H");
        println!(
            "\n{}",
            Style::new().bold().apply_to("Web Search Extra Parameters")
        );
        println!("{}", Style::new().dim().apply_to("─".repeat(50)));
        println!("Provider: {:?}", provider);
        println!(
            "{}",
            Style::new()
                .dim()
                .apply_to("(Parameters marked with ✗ are not supported by this provider)")
        );
        println!();

        // Get current extra params
        let extra_params = config
            .active_profile()
            .web_search
            .extra_params
            .clone()
            .unwrap_or_default();

        // Build menu items showing current values
        let mut menu_items = vec![];
        let mut param_keys = vec![];

        macro_rules! add_param {
            ($key:expr, $label:expr, $default:expr) => {
                let supported = is_supported($key);
                let current = extra_params
                    .get($key)
                    .cloned()
                    .unwrap_or_else(|| $default.to_string());
                let status = if supported { "✓" } else { "✗" };
                menu_items.push(format!(
                    "{} {}: {} (current: {})",
                    status,
                    $label,
                    if supported { "" } else { "[not supported]" },
                    current
                ));
                param_keys.push($key);
            };
        }

        add_param!("type", "Search Type (auto/instant/deep)", "auto");
        add_param!("numResults", "Number of Results", "5");
        add_param!("category", "Category Filter", "none");
        add_param!("maxAgeHours", "Max Age Hours (-1=cache, 0=live)", "default");
        add_param!("includeDomains", "Include Domains (comma-separated)", "");
        add_param!("excludeDomains", "Exclude Domains (comma-separated)", "");
        add_param!("contents.text", "Use Full Text (true/false)", "false");
        add_param!(
            "contents.highlights.maxCharacters",
            "Highlight Max Characters",
            "2000"
        );

        menu_items.push("Clear All Parameters".to_string());
        menu_items.push("Back".to_string());

        let selection = Select::new()
            .with_prompt("Select parameter to configure")
            .items(&menu_items)
            .default(0)
            .interact()?;

        if selection == menu_items.len() - 1 {
            // Back
            return Ok(());
        }

        if selection == menu_items.len() - 2 {
            // Clear all parameters
            let profile = config.active_profile_mut();
            profile.web_search.extra_params = None;
            config.save_default()?;
            println!("\n✅ All extra parameters cleared");
            tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
            continue;
        }

        let param_key = param_keys[selection];

        if !is_supported(param_key) {
            println!(
                "\n⚠️  This parameter is not supported by {:?}",
                provider
            );
            println!(
                "   Supported providers: {:?}",
                supported_params.get(param_key).unwrap_or(&vec![])
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
            continue;
        }

        // Get current value
        let current_value = extra_params.get(param_key).cloned().unwrap_or_default();

        // Prompt for new value
        let new_value: String = Input::new()
            .with_prompt(format!(
                "Enter value for '{}' (leave empty to remove)",
                param_key
            ))
            .allow_empty(true)
            .default(current_value)
            .interact()?;

        // Update the config
        let profile = config.active_profile_mut();
        let params = profile
            .web_search
            .extra_params
            .get_or_insert_with(HashMap::new);

        if new_value.is_empty() {
            params.remove(param_key);
        } else {
            params.insert(param_key.to_string(), new_value);
        }

        config.save_default()?;
        println!("\n✅ Parameter '{}' updated", param_key);
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}

async fn test_web_search_api_key(provider: &SearchProvider, api_key: &str) -> Result<()> {
    use mylm_core::agent::runtime::capability::ToolCapability;
    use mylm_core::agent::runtime::context::RuntimeContext;
    use mylm_core::agent::runtime::tools::web_search::{
        SearchProvider as WsProvider, WebSearchConfig, WebSearchTool,
    };
    use mylm_core::agent::types::intents::ToolCall;

    log::debug!("[CONFIG] Testing web search API key for provider {:?}", provider);

    let config = WebSearchConfig {
        enabled: true,
        api_key: Some(api_key.to_string()),
        provider: match provider {
            SearchProvider::DuckDuckGo => WsProvider::DuckDuckGo,
            SearchProvider::Serpapi => WsProvider::SerpApi,
            SearchProvider::Brave => WsProvider::Brave,
            SearchProvider::Openai => WsProvider::OpenAi,
            SearchProvider::Exa => WsProvider::Exa,
            SearchProvider::Google => WsProvider::Google,
            SearchProvider::Tavily => WsProvider::Tavily,
            SearchProvider::Kimi => WsProvider::DuckDuckGo, // Fallback
            SearchProvider::Custom => WsProvider::Custom,
        },
    };

    let tool = WebSearchTool::with_config(config);
    let call = ToolCall {
        name: "web_search".to_string(),
        arguments: serde_json::json!("test query"),
        working_dir: None,
        timeout_secs: Some(10),
    };

    // Create a dummy runtime context
    let ctx = RuntimeContext::default();

    match tool.execute(&ctx, call).await {
        Ok(result) => match result {
            mylm_core::agent::types::events::ToolResult::Success { .. } => Ok(()),
            mylm_core::agent::types::events::ToolResult::Error { message, .. } => {
                Err(anyhow::anyhow!("{}", message))
            }
            mylm_core::agent::types::events::ToolResult::Cancelled => {
                Err(anyhow::anyhow!("Request was cancelled"))
            }
        },
        Err(e) => Err(anyhow::anyhow!("Request failed: {}", e)),
    }
}
