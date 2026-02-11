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
    PromptSettings, // Prompt customization
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
            SettingsMenuChoice::PromptSettings => write!(f, "📝 [6] Prompt Settings"),
            SettingsMenuChoice::Back => write!(f, "⬅️  [7] Back"),
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
    WorkerResilienceSettings,
    ToggleTmuxAutostart,
    ToggleAgentVersion,
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
            AgentSettingsChoice::ToggleAgentVersion => write!(f, "🤖 Toggle Agent Version (V1/V2)"),
            AgentSettingsChoice::PaCoReSettings => write!(f, "⚡ PaCoRe Settings"),
            AgentSettingsChoice::PermissionsSettings => write!(f, "🔒 Permissions"),
            AgentSettingsChoice::WorkerResilienceSettings => write!(f, "🛡️  Worker Resilience Settings"),
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
    ConfigureWorkerShell,
    Back,
}

impl std::fmt::Display for PermissionsMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PermissionsMenuChoice::SetAllowedTools => write!(f, "🔧 Allowed Tools"),
            PermissionsMenuChoice::SetAutoApproveCommands => write!(f, "✅ Auto-Approve Commands"),
            PermissionsMenuChoice::SetForbiddenCommands => write!(f, "🚫 Forbidden Commands"),
            PermissionsMenuChoice::ConfigureWorkerShell => write!(f, "👷 Worker Shell Permissions"),
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
    SetRateLimitTier,
    SetWorkerLimit,
    SetMainRpm,
    SetWorkersRpm,
    Back,
}

impl std::fmt::Display for RateLimitSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RateLimitSettingsChoice::SetRateLimitTier => write!(f, "⚡ Set Rate Limit Tier (Provider)"),
            RateLimitSettingsChoice::SetWorkerLimit => write!(f, "👷 Set Max Workers"),
            RateLimitSettingsChoice::SetMainRpm => write!(f, "🤖 Set Main Agent Rate Limit (RPM)"),
            RateLimitSettingsChoice::SetWorkersRpm => write!(f, "⚙️  Set Workers Rate Limit (RPM)"),
            RateLimitSettingsChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}

/// Worker Resilience settings submenu
#[derive(Debug, PartialEq)]
pub enum WorkerResilienceSettingsChoice {
    SetMaxToolFailures,
    Back,
}

impl std::fmt::Display for WorkerResilienceSettingsChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerResilienceSettingsChoice::SetMaxToolFailures => write!(f, "🔧 Set Max Tool Failures"),
            WorkerResilienceSettingsChoice::Back => write!(f, "⬅️  Back"),
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

/// Worker Shell settings submenu
#[derive(Debug, PartialEq)]
pub enum WorkerShellMenuChoice {
    SetAllowedPatterns,
    SetRestrictedPatterns,
    SetForbiddenPatterns,
    SetEscalationMode,
    ResetToDefaults,
    Back,
}

impl std::fmt::Display for WorkerShellMenuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerShellMenuChoice::SetAllowedPatterns => write!(f, "🔧 Set Allowed Patterns"),
            WorkerShellMenuChoice::SetRestrictedPatterns => write!(f, "⚠️  Set Restricted Patterns"),
            WorkerShellMenuChoice::SetForbiddenPatterns => write!(f, "🚫 Set Forbidden Patterns"),
            WorkerShellMenuChoice::SetEscalationMode => write!(f, "⚙️  Set Escalation Mode"),
            WorkerShellMenuChoice::ResetToDefaults => write!(f, "🔄 Reset to Defaults"),
            WorkerShellMenuChoice::Back => write!(f, "⬅️  Back"),
        }
    }
}
