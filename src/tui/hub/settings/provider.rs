//! Provider settings menu loop

use anyhow::Result;

use crate::tui::hub::choices::ProviderMenuChoice;
use crate::tui::hub::handlers;
use mylm_core::config::Config;

/// Provider menu loop
pub async fn run(config: &mut Config) -> Result<()> {
    loop {
        match show_provider_menu()? {
            ProviderMenuChoice::AddProvider => {
                handlers::handle_add_provider(config).await?;
            }
            ProviderMenuChoice::EditProvider => {
                handlers::handle_edit_provider(config).await?;
            }
            ProviderMenuChoice::RemoveProvider => {
                handlers::handle_remove_provider(config)?;
            }
            ProviderMenuChoice::Back => break,
        }
    }
    Ok(())
}

/// Show provider menu
fn show_provider_menu() -> Result<ProviderMenuChoice> {
    use dialoguer::Select;

    let choices = vec![
        ProviderMenuChoice::AddProvider,
        ProviderMenuChoice::EditProvider,
        ProviderMenuChoice::RemoveProvider,
        ProviderMenuChoice::Back,
    ];

    let selection = Select::new()
        .with_prompt("Provider Management")
        .items(&choices)
        .default(0)
        .interact()?;

    Ok(choices[selection])
}
