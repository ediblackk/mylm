//! Application Configuration
//!
//! UI settings, feature toggles, and application preferences.

use serde::{Deserialize, Serialize};

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Enable tmux integration
    #[serde(default = "default_true")]
    pub tmux_enabled: bool,

    /// Default terminal shell
    #[serde(default = "default_shell")]
    pub shell: String,

    /// Editor command
    #[serde(default = "default_editor")]
    pub editor: String,

    /// Theme
    #[serde(default)]
    pub theme: Theme,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tmux_enabled: true,
            shell: default_shell(),
            editor: default_editor(),
            theme: Theme::default(),
        }
    }
}

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

fn default_editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string())
}

fn default_true() -> bool {
    true
}

/// UI Theme
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Default,
    Dark,
    Light,
}

/// Feature toggles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureConfig {
    /// Enable web search
    #[serde(default)]
    pub web_search: bool,

    /// Enable memory/vector store
    #[serde(default = "default_true")]
    pub memory: bool,

    /// Enable worker delegation
    #[serde(default = "default_true")]
    pub workers: bool,

    /// Enable telemetry
    #[serde(default)]
    pub telemetry: bool,

    /// Auto-approve safe commands
    #[serde(default)]
    pub auto_approve_safe: bool,

    /// PaCoRe (Proactive Context Recall) settings
    #[serde(default)]
    pub pacore: PaCoReConfig,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            web_search: false,
            memory: true,
            workers: true,
            telemetry: false,
            auto_approve_safe: false,
            pacore: PaCoReConfig::default(),
        }
    }
}

/// PaCoRe configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaCoReConfig {
    /// Enable PaCoRe
    #[serde(default)]
    pub enabled: bool,

    /// Number of proactive rounds
    #[serde(default = "default_pacore_rounds")]
    pub rounds: usize,
}

impl Default for PaCoReConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rounds: default_pacore_rounds(),
        }
    }
}

fn default_pacore_rounds() -> usize {
    3
}
