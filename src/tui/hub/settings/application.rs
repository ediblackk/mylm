//! Application settings menu

use anyhow::Result;
use dialoguer::Input;

use crate::tui::hub::choices::ApplicationSettingsChoice;
use mylm_core::config::Config;

/// Application settings loop
pub async fn run(config: &mut Config) -> Result<()> {
    loop {
        match show_application_settings_menu()? {
            ApplicationSettingsChoice::ToggleTmuxAutostart => {
                config.app.tmux_enabled = !config.app.tmux_enabled;
                config.save_default()?;
                println!(
                    "\n✅ Tmux autostart {}",
                    if config.app.tmux_enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
            }
            ApplicationSettingsChoice::SetPreferredAlias => {
                let alias: String = Input::new()
                    .with_prompt("Enter preferred alias")
                    .default(config.active_profile.clone())
                    .interact()?;
                // Store alias in config (would need custom field)
                println!("\n[STUB] Set preferred alias to: {}\n", alias);
            }
            ApplicationSettingsChoice::Back => break,
        }
    }
    Ok(())
}

/// Show application settings menu
fn show_application_settings_menu() -> Result<ApplicationSettingsChoice> {
    use console::Style;
    use dialoguer::Select;

    print!("\x1B[2J\x1B[1;1H");

    println!("\n{}", Style::new().bold().apply_to("Application Settings"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));

    let choices = vec![
        ApplicationSettingsChoice::ToggleTmuxAutostart,
        ApplicationSettingsChoice::SetPreferredAlias,
        ApplicationSettingsChoice::Back,
    ];

    let selection = Select::new()
        .with_prompt("Select option")
        .items(&choices)
        .default(0)
        .interact()?;

    Ok(choices[selection])
}
