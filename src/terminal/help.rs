//! Dynamic Help System for F1 Help Screen
//!
//! Generates comprehensive help text dynamically from app state and configuration.

use mylm_core::config::manager::ConfigManager;
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
        _app: &crate::terminal::app::App,
        _config_manager: Option<&Arc<ConfigManager>>,
    ) -> String {
        let mut output = String::new();

        // Header
        output.push_str("╔══════════════════════════════════════════════════════════════════════════════╗\n");
        output.push_str("║                            MYLM HELP                                         ║\n");
        output.push_str("╠══════════════════════════════════════════════════════════════════════════════╣\n");

        // KEYBINDINGS SECTION
        output.push_str("║ KEYBINDINGS                                                                  ║\n");
        let keybindings = Self::get_keybindings();
        for kb in &keybindings {
            let line = format!("║   {:<20} {:<45}║\n", kb.keys, kb.description);
            output.push_str(&line);
        }
        output.push_str("╠══════════════════════════════════════════════════════════════════════════════╣\n");

        // MOUSE SECTION
        output.push_str("║ MOUSE                                                                        ║\n");
        output.push_str("║   Shift+Click          Bypass mylm mouse capture (use terminal's native      ║\n");
        output.push_str("║                        selection & right-click menu)                         ║\n");
        output.push_str("║   Drag                 Select text in terminal or chat pane                  ║\n");
        output.push_str("║   Right/Middle-click   Paste clipboard (or copy if clicking on selection)    ║\n");
        output.push_str("║   Scroll wheel         Scroll focused pane                                   ║\n");
        output.push_str("╠══════════════════════════════════════════════════════════════════════════════╣\n");

        // SLASH COMMANDS SECTION
        output.push_str("║ SLASH COMMANDS (type in Chat input)                                          ║\n");
        let commands = Self::get_slash_commands();
        for cmd in &commands {
            let line = format!("║   {:<20} {:<45}║\n", cmd.command, cmd.description);
            output.push_str(&line);
        }
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
        output.push_str("║   • Hold Shift when clicking to use your terminal's native mouse features    ║\n");
        output.push_str("║   • Use /pacore for parallel context retrieval reasoning                     ║\n");
        output.push_str("║   • Background jobs appear in jobs panel (F4)                                ║\n");
        output.push_str("║   • Workers can process large documents in parallel                          ║\n");
        output.push_str("║   • Configuration hot-reloads when you edit config.toml                      ║\n");
        output.push_str("║   • Use Ctrl+Shift+←/→ to adjust chat/terminal split (20%-100%)              ║\n");
        output.push_str("╚══════════════════════════════════════════════════════════════════════════════╝\n");

        output
    }

    /// Get all keybindings
    fn get_keybindings() -> Vec<Keybinding> {
        vec![
            Keybinding {
                keys: "F1",
                description: "Toggle this help screen",
            },
            Keybinding {
                keys: "F2",
                description: "Cycle focus (Terminal → Chat → Jobs)",
            },
            Keybinding {
                keys: "F3",
                description: "Toggle memory graph view",
            },
            Keybinding {
                keys: "F4",
                description: "Toggle jobs panel",
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
    }

    #[test]
    fn test_slash_commands_list_not_empty() {
        let cmds = HelpSystem::get_slash_commands();
        assert!(!cmds.is_empty());
        assert!(cmds.iter().any(|cmd| cmd.command.contains("/help")));
    }
}
