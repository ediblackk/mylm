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

    /// Onboarding completed flag
    #[serde(default)]
    pub onboarding_completed: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tmux_enabled: true,
            shell: default_shell(),
            editor: default_editor(),
            theme: Theme::default(),
            onboarding_completed: false,
        }
    }
}

fn default_shell() -> String {
    if cfg!(target_os = "windows") {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

fn default_editor() -> String {
    if cfg!(target_os = "windows") {
        std::env::var("EDITOR").unwrap_or_else(|_| "notepad".to_string())
    } else {
        std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string())
    }
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

/// Memory settings for context injection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySettings {
    /// Hot memory limit: recent memories to include in prompt
    #[serde(default = "default_memory_context_window")]
    pub context_window: usize,
    /// Semantic search results to include
    #[serde(default = "default_memory_semantic_limit")]
    pub semantic_search_limit: usize,
    /// Tool search results limit
    #[serde(default = "default_memory_tool_limit")]
    pub tool_search_limit: usize,
}

fn default_memory_context_window() -> usize { 5 }
fn default_memory_semantic_limit() -> usize { 10 }
fn default_memory_tool_limit() -> usize { 5 }

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            context_window: default_memory_context_window(),
            semantic_search_limit: default_memory_semantic_limit(),
            tool_search_limit: default_memory_tool_limit(),
        }
    }
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

    /// Memory context injection settings
    #[serde(default)]
    pub memory_settings: MemorySettings,

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
            memory_settings: MemorySettings::default(),
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
