//! Web search settings configuration

use anyhow::Result;
use dialoguer::Password;
use inquire::Select as InquireSelect;
use mylm_core::config::Config;

use crate::cli::hub::menus::WebSearchMenuChoice;

/// Show web search settings menu
pub fn show_web_search_menu(config: &Config) -> Result<WebSearchMenuChoice> {
    // Clear screen for clean display
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout())?;

    let enabled = if config.features.web_search.enabled {
        "On"
    } else {
        "Off"
    };
    let provider = match config.features.web_search.provider {
        mylm_core::config::SearchProvider::Kimi => "Kimi",
        mylm_core::config::SearchProvider::Serpapi => "SerpApi",
        mylm_core::config::SearchProvider::Brave => "Brave",
    };
    let key_status =
        if config.features.web_search.api_key.as_ref().is_none_or(|k| k.is_empty()) {
            "Not set"
        } else {
            "Set"
        };

    println!("\n🌐 Web Search Settings");
    println!("  Enabled:   {}", enabled);
    println!("  Provider:  {}", provider);
    println!("  API Key:   {}", key_status);
    println!();

    let options = vec![
        WebSearchMenuChoice::ToggleEnabled,
        WebSearchMenuChoice::SetProvider,
        WebSearchMenuChoice::SetApiKey,
        WebSearchMenuChoice::Back,
    ];

    let ans: Result<WebSearchMenuChoice, _> = InquireSelect::new("Web Search", options).prompt();
    match ans {
        Ok(choice) => Ok(choice),
        Err(_) => Ok(WebSearchMenuChoice::Back),
    }
}

/// Handle web search settings
pub async fn handle_web_search_settings(config: &mut Config) -> Result<bool> {
    loop {
        let action = show_web_search_menu(config)?;

        match action {
            WebSearchMenuChoice::ToggleEnabled => {
                config.features.web_search.enabled = !config.features.web_search.enabled;
                config.save_to_default_location()?;
                println!(
                    "✅ Web search {}",
                    if config.features.web_search.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
            }
            WebSearchMenuChoice::SetProvider => {
                let providers = vec![
                    "Kimi (Moonshot AI)",
                    "SerpAPI (Google/Bing)",
                    "Brave Search",
                ];
                let choice =
                    InquireSelect::new("Select web search provider:", providers).prompt()?;

                config.features.web_search.provider = match choice {
                    "Kimi (Moonshot AI)" => mylm_core::config::SearchProvider::Kimi,
                    "Brave Search" => mylm_core::config::SearchProvider::Brave,
                    _ => mylm_core::config::SearchProvider::Serpapi,
                };
                config.features.web_search.enabled = true;
                config.save_to_default_location()?;
                println!("✅ Web search provider updated!");
            }
            WebSearchMenuChoice::SetApiKey => {
                let key = Password::new()
                    .with_prompt("Web Search API Key")
                    .allow_empty_password(true)
                    .interact()?;
                if !key.trim().is_empty() {
                    config.features.web_search.api_key = Some(key);
                    config.save_to_default_location()?;
                    println!("✅ API Key saved!");
                }
            }
            WebSearchMenuChoice::Back => break,
        }
    }
    Ok(true)
}
