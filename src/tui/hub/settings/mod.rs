//! Settings menus - main settings dashboard and submenus

pub mod application;
pub mod main_llm;
pub mod provider;
pub mod web_search;
pub mod worker_llm;

use anyhow::Result;
use dialoguer::Select;
use mylm_core::config::Config;

use crate::tui::hub::choices::SettingsMenuChoice;
use crate::tui::hub::display::print_config_banner;
use crate::tui::hub::handlers;

/// Run the main settings dashboard
pub async fn run(config: &mut Config) -> Result<()> {
    loop {
        match show_settings_dashboard(config)? {
            SettingsMenuChoice::ManageProviders => {
                provider::run(config).await?;
            }
            SettingsMenuChoice::MainLLMSettings => {
                main_llm::run(config).await?;
            }
            SettingsMenuChoice::WorkerLLMSettings => {
                worker_llm::run(config).await?;
            }
            SettingsMenuChoice::TestMainConnection => {
                handlers::test_profile_connection(config, &config.active_profile.clone())
                    .await?;
            }
            SettingsMenuChoice::TestWorkerConnection => {
                handlers::test_profile_connection(config, "worker").await?;
            }
            SettingsMenuChoice::WebSearchSettings => {
                web_search::run(config).await?;
            }
            SettingsMenuChoice::ApplicationSettings => {
                application::run(config).await?;
            }
            SettingsMenuChoice::Back => break,
        }
    }
    Ok(())
}

fn show_settings_dashboard(config: &Config) -> Result<SettingsMenuChoice> {
    print_config_banner(config);

    let choices = vec![
        SettingsMenuChoice::ManageProviders,
        SettingsMenuChoice::MainLLMSettings,
        SettingsMenuChoice::WorkerLLMSettings,
        SettingsMenuChoice::TestMainConnection,
        SettingsMenuChoice::TestWorkerConnection,
        SettingsMenuChoice::WebSearchSettings,
        SettingsMenuChoice::ApplicationSettings,
        SettingsMenuChoice::Back,
    ];

    let selection = Select::new()
        .with_prompt("Select setting to configure")
        .items(&choices)
        .default(0)
        .interact()?;

    Ok(choices[selection])
}
