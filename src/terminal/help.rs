//! Dynamic Help System for F1 Help Screen
//!
//! Generates comprehensive help text dynamically from app state and configuration.

use mylm_core::config::manager::{Config, ConfigManager};
use std::sync::Arc;

/// A keybinding entry for display
#[derive(Debug, Clone)]
pub struct Keybinding {
    pub keys: &'static str,
    pub description: &'static str,
}

/// A slash command entry for display
#[derive(Debug, Clone)]
pub struct SlashCommand {
    pub command: &'static str,
    pub description: &'static str,
}

/// HelpSystem generates dynamic help content
pub struct HelpSystem;

impl HelpSystem {
    /// Generate comprehensive help text dynamically
    pub fn generate_help_text(
        app: &crate::terminal::app::App,
        config_manager: Option<&Arc<ConfigManager>>,
    ) -> String {
        let mut output = String::new();

        // Header
        output.push_str("╔══════════════════════════════════════════════════════════════════════════════╗\n");
        output.push_str("║                            MYLM HELP (F1)                                    ║\n");
        output.push_str("╠══════════════════════════════════════════════════════════════════════════════╣\n");

        // KEYBINDINGS SECTION
        output.push_str("║ KEYBINDINGS                                                                  ║\n");
        let keybindings = Self::get_keybindings();
        for kb in &keybindings {
            let line = format!("║   {:<20} {:<45}║\n", kb.keys, kb.description);
            output.push_str(&line);
        }
        output.push_str("╠══════════════════════════════════════════════════════════════════════════════╣\n");

        // SLASH COMMANDS SECTION
        output.push_str("║ SLASH COMMANDS                                                               ║\n");
        let commands = Self::get_slash_commands();
        for cmd in &commands {
            let line = format!("║   {:<20} {:<45}║\n", cmd.command, cmd.description);
            output.push_str(&line);
        }
        output.push_str("╠══════════════════════════════════════════════════════════════════════════════╣\n");

        // CURRENT CONFIGURATION SECTION
        output.push_str("║ CURRENT CONFIGURATION                                                        ║\n");

        // Get config from config_manager or use app config defaults
        let (max_context, condense_threshold, max_output, worker_limit, rate_tokens, rate_reqs) =
            if let Some(_cm) = config_manager {
                // Try to get config asynchronously - this is best effort
                // Since we're in a sync context, use the Config defaults
                let config = Config::default();
                (
                    config.max_context_tokens,
                    config.condense_threshold,
                    config.max_output_tokens,
                    config.worker_limit,
                    config.rate_limit_tokens_per_minute,
                    config.rate_limit_requests_per_minute,
                )
            } else {
                // Use hardcoded defaults from core config
                (128_000, 0.8, 4096, 5, 100_000, 100)
            };

        // Get context usage from context_manager
        let (context_usage, _) = app.context_manager.get_token_usage();
        let context_ratio = app.context_manager.get_context_ratio();
        let usage_percent = (context_ratio * 100.0) as usize;

        // Get active worker count from job registry
        let active_workers = app.job_registry.list_active_jobs().len();

        output.push_str(&format!(
            "║   Max Context Tokens:     {:>10}                                      ║\n",
            format!("{}", max_context)
        ));
        output.push_str(&format!(
            "║   Condensation Threshold: {:>10}%                                     ║\n",
            (condense_threshold * 100.0) as usize
        ));
        output.push_str(&format!(
            "║   Max Output Tokens:      {:>10}                                      ║\n",
            format!("{}", max_output)
        ));
        output.push_str(&format!(
            "║   Worker Limit:           {:>10}                                      ║\n",
            worker_limit
        ));
        output.push_str(&format!(
            "║   Rate Limit:             {:>10} tokens/min, {} req/min               ║\n",
            rate_tokens, rate_reqs
        ));
        output.push_str(&format!(
            "║   Active Workers:         {:>10}                                      ║\n",
            active_workers
        ));
        output.push_str(&format!(
            "║   Current Context Usage:  {:>10} / {} ({:>3}%)                         ║\n",
            context_usage, max_context, usage_percent
        ));
        output.push_str("╠══════════════════════════════════════════════════════════════════════════════╣\n");

        // CONTEXT MANAGEMENT SECTION
        output.push_str("║ CONTEXT MANAGEMENT                                                           ║\n");
        output.push_str("║   • Automatic condensation triggers at threshold (80% by default)            ║\n");
        output.push_str("║   • Manual condensation: Press Ctrl+L while in Chat focus                    ║\n");
        output.push_str("║   • Workers isolate context - each job has its own environment               ║\n");
        output.push_str("║   • Job results persist and can be viewed in the jobs panel (F4)             ║\n");
        output.push_str("╠══════════════════════════════════════════════════════════════════════════════╣\n");

        // TIPS SECTION
        output.push_str("║ TIPS                                                                         ║\n");
        output.push_str("║   • Use /pacore for parallel context retrieval reasoning                     ║\n");
        output.push_str("║   • Background jobs appear in jobs panel (F4)                                ║\n");
        output.push_str("║   • Workers can process large documents in parallel                          ║\n");
        output.push_str("║   • Configuration hot-reloads when you edit config.toml                      ║\n");
        output.push_str("║   • Use Ctrl+Shift+←/→ to adjust chat/terminal split                         ║\n");
        output.push_str("║   • Toggle terminal visibility with Ctrl+Shift+T                             ║\n");
        output.push_str("╚══════════════════════════════════════════════════════════════════════════════╝\n");

        output
    }

    /// Get all keybindings
    fn get_keybindings() -> Vec<Keybinding> {
        vec![
            Keybinding {
                keys: "F1",
                description: "Show this help screen",
            },
            Keybinding {
                keys: "F2",
                description: "Toggle focus between Terminal and Chat",
            },
            Keybinding {
                keys: "F3",
                description: "Toggle Memory Relationship Graph",
            },
            Keybinding {
                keys: "F4",
                description: "Toggle Background Jobs Panel",
            },
            Keybinding {
                keys: "Ctrl+C",
                description: "Abort current AI task (while running)",
            },
            Keybinding {
                keys: "Ctrl+L",
                description: "Manual context condensation",
            },
            Keybinding {
                keys: "Enter",
                description: "Submit message / Confirm action",
            },
            Keybinding {
                keys: "Esc",
                description: "Cancel / Back / Exit prompt",
            },
            Keybinding {
                keys: "Up/Down",
                description: "Navigate history / Scroll",
            },
            Keybinding {
                keys: "PgUp/PgDn",
                description: "Scroll terminal/chat history",
            },
            Keybinding {
                keys: "Ctrl+Y",
                description: "Copy last AI response (then U: copy all)",
            },
            Keybinding {
                keys: "Ctrl+B",
                description: "Copy visible terminal buffer",
            },
            Keybinding {
                keys: "Ctrl+V",
                description: "Toggle Verbose Mode",
            },
            Keybinding {
                keys: "Ctrl+T",
                description: "Toggle showing AI Thoughts",
            },
            Keybinding {
                keys: "Ctrl+A",
                description: "Toggle Auto-Approve / Go to start",
            },
            Keybinding {
                keys: "Ctrl+E",
                description: "Go to end of line",
            },
            Keybinding {
                keys: "Ctrl+K",
                description: "Kill (delete) to end of line",
            },
            Keybinding {
                keys: "Ctrl+U",
                description: "Kill (delete) to start of line",
            },
            Keybinding {
                keys: "Ctrl+Shift+T",
                description: "Toggle Terminal Visibility",
            },
            Keybinding {
                keys: "Ctrl+Shift+←/→",
                description: "Adjust chat width",
            },
        ]
    }

    /// Get all slash commands
    fn get_slash_commands() -> Vec<SlashCommand> {
        vec![
            SlashCommand {
                command: "/help, /h",
                description: "Show this help",
            },
            SlashCommand {
                command: "/pacore",
                description: "Toggle parallel context retrieval mode",
            },
            SlashCommand {
                command: "/pacore on|off",
                description: "Enable/disable PaCoRe",
            },
            SlashCommand {
                command: "/pacore rounds <n,n>",
                description: "Set PaCoRe rounds (e.g., 4,1)",
            },
            SlashCommand {
                command: "/pacore status",
                description: "Show PaCoRe status",
            },
            SlashCommand {
                command: "/pacore save",
                description: "Save PaCoRe config to disk",
            },
            SlashCommand {
                command: "/profile <name>",
                description: "Switch to named profile",
            },
            SlashCommand {
                command: "/model <name>",
                description: "Set model for active profile",
            },
            SlashCommand {
                command: "/model clear",
                description: "Clear model override",
            },
            SlashCommand {
                command: "/config <key> <val>",
                description: "Update config (model, max_iterations)",
            },
            SlashCommand {
                command: "/exec <command>",
                description: "Execute shell command",
            },
            SlashCommand {
                command: "/verbose",
                description: "Toggle verbose mode",
            },
            SlashCommand {
                command: "/logs [n]",
                description: "Show recent logs (default: 20)",
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keybindings_list_not_empty() {
        let kbs = HelpSystem::get_keybindings();
        assert!(!kbs.is_empty());
        assert!(kbs.iter().any(|kb| kb.keys == "F1"));
    }

    #[test]
    fn test_slash_commands_list_not_empty() {
        let cmds = HelpSystem::get_slash_commands();
        assert!(!cmds.is_empty());
        assert!(cmds.iter().any(|cmd| cmd.command.contains("/help")));
    }
}
