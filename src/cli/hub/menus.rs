//! Menu enums and their Display implementations for the Hub UI

/// Main hub choice enum
#[derive(Debug, PartialEq)]
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

impl std::fmt::Display for HubChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HubChoice::PopTerminal => {
                if mylm_core::context::terminal::TerminalContext::is_inside_tmux() {
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

/// Settings dashboard main menu choices
#[derive(Debug, PartialEq)]
pub enum SettingsMenuChoice {
    ManageProviders, // Add/Edit/Remove providers
    SelectMainModel, // Choose provider + model
    SelectWorkerModel, // Choose provider + model for worker
    WebSearchSettings, // Web search provider config
    AgentSettings, // Max iterations, tmux, etc
    Back,
}

impl std::fmt::Display for SettingsMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsMenuChoice::ManageProviders => write!(f, "🔌 [1] Manage Providers"),
            SettingsMenuChoice::SelectMainModel => write!(f, "🧠 [2] Select Main LLM"),
            SettingsMenuChoice::SelectWorkerModel => write!(f, "⚡ [3] Select Worker Model"),
            SettingsMenuChoice::WebSearchSettings => write!(f, "🌐 [4] Web Search"),
            SettingsMenuChoice::AgentSettings => write!(f, "⚙️  [5] Agent Settings"),
            SettingsMenuChoice::Back => write!(f, "⬅️  [6] Back"),
        }
    }
}

/// Provider management submenu
#[derive(Debug, PartialEq)]
pub enum ProviderMenuChoice {
    AddProvider,
    EditProvider,
    RemoveProvider,
    Back,
}

impl std::fmt::Display for ProviderMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderMenuChoice::AddProvider => write!(f, "➕ Add Provider"),
            ProviderMenuChoice::EditProvider => write!(f, "✏️  Edit Provider"),
            ProviderMenuChoice::RemoveProvider => write!(f, "🗑️  Remove Provider"),
            ProviderMenuChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Web search settings submenu
#[derive(Debug, PartialEq)]
pub enum WebSearchMenuChoice {
    ToggleEnabled,
    SetProvider,
    SetApiKey,
    Back,
}

impl std::fmt::Display for WebSearchMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebSearchMenuChoice::ToggleEnabled => write!(f, "✅ Toggle Enabled"),
            WebSearchMenuChoice::SetProvider => write!(f, "🧭 Set Provider"),
            WebSearchMenuChoice::SetApiKey => write!(f, "🔑 Set API Key"),
            WebSearchMenuChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Agent settings submenu
#[derive(Debug, PartialEq)]
pub enum AgentSettingsChoice {
    IterationsSettings,
    RateLimitSettings,
    ToggleTmuxAutostart,
    PaCoReSettings,
    PermissionsSettings,
    Back,
}

impl std::fmt::Display for AgentSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentSettingsChoice::IterationsSettings => write!(f, "🔁 Iterations Settings"),
            AgentSettingsChoice::RateLimitSettings => write!(f, "⏱️  Rate Limit Settings (LLM)"),
            AgentSettingsChoice::ToggleTmuxAutostart => write!(f, "🔄 Toggle Tmux Autostart"),
            AgentSettingsChoice::PaCoReSettings => write!(f, "⚡ PaCoRe Settings"),
            AgentSettingsChoice::PermissionsSettings => write!(f, "🔒 Permissions"),
            AgentSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Permissions settings submenu
#[derive(Debug, PartialEq)]
pub enum PermissionsMenuChoice {
    SetAllowedTools,
    SetAutoApproveCommands,
    SetForbiddenCommands,
    Back,
}

impl std::fmt::Display for PermissionsMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PermissionsMenuChoice::SetAllowedTools => write!(f, "🔧 Allowed Tools"),
            PermissionsMenuChoice::SetAutoApproveCommands => write!(f, "✅ Auto-Approve Commands"),
            PermissionsMenuChoice::SetForbiddenCommands => write!(f, "🚫 Forbidden Commands"),
            PermissionsMenuChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Iterations settings submenu
#[derive(Debug, PartialEq)]
pub enum IterationsSettingsChoice {
    SetMaxIterations,
    SetRateLimit,
    Back,
}

impl std::fmt::Display for IterationsSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IterationsSettingsChoice::SetMaxIterations => write!(f, "🔢 Set Max Iterations"),
            IterationsSettingsChoice::SetRateLimit => write!(f, "⏱️  Set Iteration Delay (ms)"),
            IterationsSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// LLM Rate Limit settings submenu
#[derive(Debug, PartialEq)]
pub enum RateLimitSettingsChoice {
    SetMainRpm,
    SetWorkersRpm,
    Back,
}

impl std::fmt::Display for RateLimitSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RateLimitSettingsChoice::SetMainRpm => write!(f, "🤖 Set Main Agent Rate Limit (RPM)"),
            RateLimitSettingsChoice::SetWorkersRpm => write!(f, "👷 Set Workers Rate Limit (RPM)"),
            RateLimitSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// PaCoRe settings submenu
#[derive(Debug, PartialEq)]
pub enum PaCoReSettingsChoice {
    TogglePaCoRe,
    SetPaCoReRounds,
    Back,
}

impl std::fmt::Display for PaCoReSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaCoReSettingsChoice::TogglePaCoRe => write!(f, "⚡ Toggle PaCoRe"),
            PaCoReSettingsChoice::SetPaCoReRounds => write!(f, "📊 Set PaCoRe Rounds"),
            PaCoReSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}
