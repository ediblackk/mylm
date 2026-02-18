//! Main LLM settings menu loop

use anyhow::Result;

use crate::tui::hub::choices::{
    AgenticSettingsChoice, ContextSettingsChoice, MainLLMSettingsChoice, PaCoReSubSettingsChoice,
};
use crate::tui::hub::handlers;
use dialoguer::Select;
use mylm_core::config::Config;

/// Main LLM settings loop
pub async fn run(config: &mut Config) -> Result<()> {
    loop {
        match show_main_llm_settings_menu()? {
            MainLLMSettingsChoice::SelectModel => {
                handlers::handle_select_main_model(config).await?;
            }
            MainLLMSettingsChoice::ContextSettings => {
                run_context_settings(config, true).await?;
            }
            MainLLMSettingsChoice::AgenticSettings => {
                run_agentic_settings(config, true).await?;
            }
            MainLLMSettingsChoice::Back => break,
        }
    }
    Ok(())
}

fn show_main_llm_settings_menu() -> Result<MainLLMSettingsChoice> {
    use console::Style;

    print!("\x1B[2J\x1B[1;1H");

    println!("\n{}", Style::new().bold().apply_to("Main LLM Settings"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));

    let choices = vec![
        MainLLMSettingsChoice::SelectModel,
        MainLLMSettingsChoice::ContextSettings,
        MainLLMSettingsChoice::AgenticSettings,
        MainLLMSettingsChoice::Back,
    ];

    let selection = Select::new()
        .with_prompt("Configure Main LLM")
        .items(&choices)
        .default(0)
        .interact()?;

    Ok(choices[selection])
}

async fn run_context_settings(config: &mut Config, is_main: bool) -> Result<()> {
    loop {
        match show_context_settings_menu(is_main)? {
            ContextSettingsChoice::SetMaxTokens => {
                handlers::set_max_tokens(config, is_main)?;
            }
            ContextSettingsChoice::SetCondenseThreshold => {
                handlers::set_condense_threshold(config, is_main)?;
            }
            ContextSettingsChoice::SetInputPrice => {
                handlers::set_input_price(config, is_main)?;
            }
            ContextSettingsChoice::SetOutputPrice => {
                handlers::set_output_price(config, is_main)?;
            }
            ContextSettingsChoice::SetRateLimit => {
                handlers::set_rate_limit_rpm(config, is_main)?;
            }
            ContextSettingsChoice::Back => break,
        }
    }
    Ok(())
}

fn show_context_settings_menu(is_main: bool) -> Result<ContextSettingsChoice> {
    use console::Style;

    print!("\x1B[2J\x1B[1;1H");

    let title = if is_main {
        "Main LLM - Context Settings"
    } else {
        "Worker LLM - Context Settings"
    };
    println!("\n{}", Style::new().bold().apply_to(title));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));

    let choices = vec![
        ContextSettingsChoice::SetMaxTokens,
        ContextSettingsChoice::SetCondenseThreshold,
        ContextSettingsChoice::SetInputPrice,
        ContextSettingsChoice::SetOutputPrice,
        ContextSettingsChoice::SetRateLimit,
        ContextSettingsChoice::Back,
    ];

    let selection = Select::new()
        .with_prompt("Select option")
        .items(&choices)
        .default(0)
        .interact()?;

    Ok(choices[selection])
}

async fn run_agentic_settings(config: &mut Config, is_main: bool) -> Result<()> {
    loop {
        match show_agentic_settings_menu(is_main)? {
            AgenticSettingsChoice::SetAllowedCommands => {
                handlers::set_allowed_commands(config, is_main)?;
            }
            AgenticSettingsChoice::SetRestrictedCommands => {
                handlers::set_restricted_commands(config, is_main)?;
            }
            AgenticSettingsChoice::SetShellApprovedPatterns => {
                handlers::set_shell_approved_patterns(config, is_main)?;
            }
            AgenticSettingsChoice::SetShellForbiddenPatterns => {
                handlers::set_shell_forbidden_patterns(config, is_main)?;
            }
            AgenticSettingsChoice::SetMaxActionsBeforeStall => {
                handlers::set_max_actions_before_stall(config, is_main)?;
            }
            AgenticSettingsChoice::PaCoReSettings => {
                run_pacore_settings(config, is_main).await?;
            }
            AgenticSettingsChoice::Back => break,
        }
    }
    Ok(())
}

fn show_agentic_settings_menu(is_main: bool) -> Result<AgenticSettingsChoice> {
    use console::Style;

    print!("\x1B[2J\x1B[1;1H");

    let title = if is_main {
        "Main LLM - Agentic Settings"
    } else {
        "Worker LLM - Agentic Settings"
    };
    println!("\n{}", Style::new().bold().apply_to(title));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));

    let choices = vec![
        AgenticSettingsChoice::SetAllowedCommands,
        AgenticSettingsChoice::SetRestrictedCommands,
        AgenticSettingsChoice::SetShellApprovedPatterns,
        AgenticSettingsChoice::SetShellForbiddenPatterns,
        AgenticSettingsChoice::SetMaxActionsBeforeStall,
        AgenticSettingsChoice::PaCoReSettings,
        AgenticSettingsChoice::Back,
    ];

    let selection = Select::new()
        .with_prompt("Select option")
        .items(&choices)
        .default(0)
        .interact()?;

    Ok(choices[selection])
}

async fn run_pacore_settings(config: &mut Config, is_main: bool) -> Result<()> {
    loop {
        match show_pacore_sub_settings_menu()? {
            PaCoReSubSettingsChoice::ToggleEnabled => {
                handlers::toggle_pacore_enabled(config, is_main)?;
            }
            PaCoReSubSettingsChoice::SetRounds => {
                handlers::set_pacore_rounds(config, is_main)?;
            }
            PaCoReSubSettingsChoice::Back => break,
        }
    }
    Ok(())
}

fn show_pacore_sub_settings_menu() -> Result<PaCoReSubSettingsChoice> {
    use console::Style;

    print!("\x1B[2J\x1B[1;1H");

    println!("\n{}", Style::new().bold().apply_to("PaCoRe Settings"));
    println!("{}", Style::new().dim().apply_to("─".repeat(40)));

    let choices = vec![
        PaCoReSubSettingsChoice::ToggleEnabled,
        PaCoReSubSettingsChoice::SetRounds,
        PaCoReSubSettingsChoice::Back,
    ];

    let selection = Select::new()
        .with_prompt("Configure PaCoRe")
        .items(&choices)
        .default(0)
        .interact()?;

    Ok(choices[selection])
}
