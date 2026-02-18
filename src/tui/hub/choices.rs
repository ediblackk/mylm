//! Hub menu choices - all menu enums in one place

use std::fmt::{self, Display};

/// Main hub menu choices
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HubChoice {
    PopTerminal,
    PopTerminalMissing,
    ResumeSession,
    StartTui,
    StartIncognito,
    QuickQuery,
    ManageSessions,
    BackgroundJobs,
    Configuration,
    Exit,
}

impl Display for HubChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HubChoice::PopTerminal => {
                if super::is_tmux_available() {
                    write!(f, "🚀 Pop Terminal (tmux)")
                } else {
                    write!(f, "🚀 Pop Terminal (no tmux)")
                }
            }
            HubChoice::PopTerminalMissing => write!(f, "🚀 Pop Terminal (install tmux)"),
            HubChoice::ResumeSession => write!(f, "🔄 Resume Session"),
            HubChoice::StartTui => write!(f, "✨ TUI Session"),
            HubChoice::StartIncognito => write!(f, "🕵️  Incognito"),
            HubChoice::QuickQuery => write!(f, "⚡ Quick Query"),
            HubChoice::Configuration => write!(f, "⚙️  Config"),
            HubChoice::ManageSessions => write!(f, "📂 Sessions"),
            HubChoice::BackgroundJobs => write!(f, "🕒 Jobs"),
            HubChoice::Exit => write!(f, "❌ Exit"),
        }
    }
}

/// Settings dashboard menu choices
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsMenuChoice {
    ManageProviders,
    MainLLMSettings,
    WorkerLLMSettings,
    TestMainConnection,
    TestWorkerConnection,
    WebSearchSettings,
    ApplicationSettings,
    Back,
}

impl Display for SettingsMenuChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsMenuChoice::ManageProviders => write!(f, "🔌 [1] Manage Providers"),
            SettingsMenuChoice::MainLLMSettings => write!(f, "🧠 [2] Main LLM Settings"),
            SettingsMenuChoice::WorkerLLMSettings => write!(f, "⚡ [3] Worker LLM Settings"),
            SettingsMenuChoice::TestMainConnection => write!(f, "🧪 [4] Test Main Connection"),
            SettingsMenuChoice::TestWorkerConnection => write!(f, "🧪 [5] Test Worker Connection"),
            SettingsMenuChoice::WebSearchSettings => write!(f, "🌐 [6] Web Search"),
            SettingsMenuChoice::ApplicationSettings => write!(f, "🔧 [7] Application Settings"),
            SettingsMenuChoice::Back => write!(f, "⬅️  [8] Back"),
        }
    }
}

/// Main LLM settings menu choices
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MainLLMSettingsChoice {
    SelectModel,
    ContextSettings,
    AgenticSettings,
    Back,
}

impl Display for MainLLMSettingsChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MainLLMSettingsChoice::SelectModel => write!(f, "🎯 Select Model"),
            MainLLMSettingsChoice::ContextSettings => write!(f, "📊 Context Settings"),
            MainLLMSettingsChoice::AgenticSettings => write!(f, "🤖 Agentic Settings"),
            MainLLMSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Worker LLM settings menu choices
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorkerLLMSettingsChoice {
    SelectModel,
    ContextSettings,
    AgenticSettings,
    Back,
}

impl Display for WorkerLLMSettingsChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkerLLMSettingsChoice::SelectModel => write!(f, "🎯 Select Model"),
            WorkerLLMSettingsChoice::ContextSettings => write!(f, "📊 Context Settings"),
            WorkerLLMSettingsChoice::AgenticSettings => write!(f, "🤖 Agentic Settings"),
            WorkerLLMSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Application settings submenu choices
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApplicationSettingsChoice {
    ToggleTmuxAutostart,
    SetPreferredAlias,
    Back,
}

impl Display for ApplicationSettingsChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApplicationSettingsChoice::ToggleTmuxAutostart => {
                write!(f, "🔄 Toggle Tmux Autostart")
            }
            ApplicationSettingsChoice::SetPreferredAlias => write!(f, "🏷️  Set Preferred Alias"),
            ApplicationSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Context settings submenu choices
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContextSettingsChoice {
    SetMaxTokens,
    SetCondenseThreshold,
    SetInputPrice,
    SetOutputPrice,
    SetRateLimit,
    Back,
}

impl Display for ContextSettingsChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextSettingsChoice::SetMaxTokens => write!(f, "🔢 Max Context Tokens"),
            ContextSettingsChoice::SetCondenseThreshold => write!(f, "📉 Condense Threshold"),
            ContextSettingsChoice::SetInputPrice => write!(f, "💰 Input Price (per 1M)"),
            ContextSettingsChoice::SetOutputPrice => write!(f, "💰 Output Price (per 1M)"),
            ContextSettingsChoice::SetRateLimit => write!(f, "⏱️  Rate Limit (RPM)"),
            ContextSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Agentic settings submenu choices
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgenticSettingsChoice {
    SetAllowedCommands,
    SetRestrictedCommands,
    SetShellApprovedPatterns,
    SetShellForbiddenPatterns,
    SetMaxActionsBeforeStall,
    PaCoReSettings,
    Back,
}

impl Display for AgenticSettingsChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgenticSettingsChoice::SetAllowedCommands => write!(f, "✅ Always Allowed Tools"),
            AgenticSettingsChoice::SetRestrictedCommands => {
                write!(f, "🚫 Always Restricted Tools")
            }
            AgenticSettingsChoice::SetShellApprovedPatterns => {
                write!(f, "🔧 Shell: Approved Patterns")
            }
            AgenticSettingsChoice::SetShellForbiddenPatterns => {
                write!(f, "🔧 Shell: Forbidden Patterns")
            }
            AgenticSettingsChoice::SetMaxActionsBeforeStall => {
                write!(f, "🔢 Max Actions Before Stall")
            }
            AgenticSettingsChoice::PaCoReSettings => write!(f, "⚡ PaCoRe Settings"),
            AgenticSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// PaCoRe settings submenu choices (inside Agentic)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaCoReSubSettingsChoice {
    ToggleEnabled,
    SetRounds,
    Back,
}

impl Display for PaCoReSubSettingsChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaCoReSubSettingsChoice::ToggleEnabled => write!(f, "✅ Toggle PaCoRe"),
            PaCoReSubSettingsChoice::SetRounds => {
                write!(f, "🔢 Set Rounds (default: 4,1)")
            }
            PaCoReSubSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Provider management submenu choices
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderMenuChoice {
    AddProvider,
    EditProvider,
    RemoveProvider,
    Back,
}

impl Display for ProviderMenuChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderMenuChoice::AddProvider => write!(f, "➕ Add Provider"),
            ProviderMenuChoice::EditProvider => write!(f, "✏️  Edit Provider"),
            ProviderMenuChoice::RemoveProvider => write!(f, "🗑️  Remove Provider"),
            ProviderMenuChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}
