//! Help view rendering (F1) and HelpSystem

use crate::tui::app::state::AppStateContainer as App;
use mylm_core::config::manager::ConfigManager;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::sync::Arc;

/// A keybinding entry for display
#[derive(Debug, Clone)]
pub struct Keybinding {
    pub keys: &'static str,
    pub description: &'static str,
}

/// HelpSystem generates dynamic help content
pub struct HelpSystem;

impl HelpSystem {
    /// Generate comprehensive help text dynamically
    pub fn generate_help_text(
        _app: &App,
        _config_manager: Option<&Arc<ConfigManager>>,
    ) -> String {
        let mut output = String::new();

        // Header
        output.push_str("╔══════════════════════════════════════════════════════════════════════════════╗\n");
        output.push_str("║                            Info                                              ║\n");
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
        output.push_str("╚══════════════════════════════════════════════════════════════════════════════╝\n");

        output
    }

    /// Get all keybindings
    fn get_keybindings() -> Vec<Keybinding> {
        vec![
            Keybinding {
                keys: "F1",
                description: "Toggle Help",
            },
            Keybinding {
                keys: "F2",
                description: "Toggle Focus (Chat/Terminal)",
            },
            Keybinding {
                keys: "F3",
                description: "Toggle Memory View",
            },
            Keybinding {
                keys: "F4",
                description: "Toggle Jobs Panel",
            },
            Keybinding {
                keys: "Ctrl+Shift+←/→",
                description: "Adjust chat/terminal split (20%-100%)",
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
}

pub fn render_help_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let help_text = HelpSystem::generate_help_text(app, None);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" [ myLM Help (F1 to close, ↑/↓ to scroll) ] ")
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.help_scroll as u16, 0));

    frame.render_widget(paragraph, area);
}

/// Render help panel as a centered modal popup
/// Reserved for future help system UI (currently unused)
#[allow(dead_code)]
pub fn render_help_panel(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let help_text = HelpSystem::generate_help_text(app, None);

    // Create a centered popup (80% width, 80% height)
    let popup_area = super::centered_rect(80, 80, area);

    // Clear the background
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" [ myLM Help (Press any key to close) ] ")
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, popup_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keybindings_list_not_empty() {
        let kbs = HelpSystem::get_keybindings();
        assert!(!kbs.is_empty());
    }
}
