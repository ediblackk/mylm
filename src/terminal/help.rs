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

        // NOTES SECTION
        output.push_str("║ NOTES                                                                        ║\n");
        output.push_str("║   • Hold Shift when clicking to use your terminal's native mouse features    ║\n");
        output.push_str("║   • Background jobs appear in jobs panel (F4)                                ║\n");
        output.push_str("║   • Configuration hot-reloads when you edit config.toml                      ║\n");
        output.push_str("╚══════════════════════════════════════════════════════════════════════════════╝\n");

        output
    }

    /// Get all keybindings
    fn get_keybindings() -> Vec<Keybinding> {
        vec![
            Keybinding {
                keys: "Ctrl+Shift+←/→",
                description: "Adjust chat/terminal split (20%-100%)",
            },
            Keybinding {
                keys: "F1",
                description: "Toggle this help screen",
            },
            Keybinding {
                keys: "F2",
                description: "Cycle focus (Terminal → Chat → Jobs)",
            },
            Keybinding {
                keys: "F4",
                description: "Toggle jobs panel",
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
                keys: "Ctrl+C",
                description: "Abort current AI task (while running)",
            },
            Keybinding {
                keys: "Ctrl+A",
                description: "Toggle Auto-Approve",
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
                keys: "Up/Down",
                description: "Navigate history / Scroll",
            },
            Keybinding {
                keys: "PgUp/PgDn",
                description: "Scroll terminal/chat history",
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
                command: "/profile <name>",
                description: "Switch to named profile",
            },
            SlashCommand {
                command: "/model <name>",
                description: "Set model for active profile",
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
                command: "/jobs",
                description: "List active jobs",
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
