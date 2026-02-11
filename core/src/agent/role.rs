//! Agent Role System - Simplified for MVP
//!
//! Provides specialized agent configurations for different tasks.
//! Roles control tool access, models, and system prompt behavior.

use crate::config::v2::types::AgentPermissions;

/// Defines the persona/capabilities of an agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentRole {
    /// Default behavior - all tools, full access
    #[default]
    Default,
    /// Read-only mode - cannot write files or execute commands
    ReadOnly,
    /// Code-focused - optimized for development tasks
    Code,
    /// Explorer - fast search, no modifications
    Explorer,
}

impl AgentRole {
    /// Get the display name for this role
    pub fn name(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::ReadOnly => "readonly",
            Self::Code => "code",
            Self::Explorer => "explorer",
        }
    }

    /// Get a description of this role
    pub fn description(&self) -> &'static str {
        match self {
            Self::Default => "Full access to all tools and capabilities",
            Self::ReadOnly => "Read-only mode. Cannot write files or execute commands.",
            Self::Code => "Optimized for coding tasks with safe defaults",
            Self::Explorer => "Fast exploration mode. Read-only, optimized for search.",
        }
    }

    /// Get the system prompt suffix for this role
    pub fn system_prompt_suffix(&self) -> &'static str {
        match self {
            Self::Default => "",
            Self::ReadOnly => {
                "\n\nIMPORTANT: You are in READ-ONLY mode. You CANNOT:\n\
                 - Write or modify any files\n\
                 - Execute any shell commands\n\
                 - Make any changes to the system\n\
                 You can only read files and provide information."
            }
            Self::Code => {
                "\n\nYou are a coding assistant. Focus on:\n\
                 - Code quality and best practices\n\
                 - Security considerations\n\
                 - Clear explanations of changes\n\
                 - Testing recommendations"
            }
            Self::Explorer => {
                "\n\nYou are in EXPLORATION mode. Your goals:\n\
                 - Quickly understand codebases\n\
                 - Find relevant information\n\
                 - Answer questions efficiently\n\
                 - You cannot modify anything - focus on analysis."
            }
        }
    }

    /// Get permissions for this role
    pub fn permissions(&self) -> AgentPermissions {
        match self {
            Self::Default => AgentPermissions::default(),
            Self::ReadOnly => AgentPermissions {
                allowed_tools: Some(vec![
                    "read_file".to_string(),
                    "search".to_string(),
                    "git_status".to_string(),
                    "git_log".to_string(),
                    "git_diff".to_string(),
                ]),
                auto_approve_commands: Some(vec![]),
                forbidden_commands: Some(vec!["*".to_string()]), // All commands forbidden
                worker_shell: None,
            },
            Self::Code => AgentPermissions {
                allowed_tools: None, // All tools allowed
                auto_approve_commands: Some(vec![
                    "cargo check".to_string(),
                    "cargo build".to_string(),
                    "cargo test".to_string(),
                ]),
                forbidden_commands: Some(vec![
                    "rm -rf /".to_string(),
                    "rm -rf ~".to_string(),
                    "> /etc".to_string(),
                ]),
                worker_shell: None,
            },
            Self::Explorer => AgentPermissions {
                allowed_tools: Some(vec![
                    "read_file".to_string(),
                    "search".to_string(),
                    "git_status".to_string(),
                    "git_log".to_string(),
                ]),
                auto_approve_commands: Some(vec![]),
                forbidden_commands: Some(vec!["*".to_string()]),
                worker_shell: None,
            },
        }
    }

    /// Parse a role from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "default" | "" => Some(Self::Default),
            "readonly" | "read-only" | "ro" => Some(Self::ReadOnly),
            "code" | "coding" | "dev" => Some(Self::Code),
            "explorer" | "explore" | "search" => Some(Self::Explorer),
            _ => None,
        }
    }

    /// List all available roles
    pub fn all_roles() -> &'static [AgentRole] {
        &[Self::Default, Self::ReadOnly, Self::Code, Self::Explorer]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_from_str() {
        assert_eq!(AgentRole::from_str("default"), Some(AgentRole::Default));
        assert_eq!(AgentRole::from_str("readonly"), Some(AgentRole::ReadOnly));
        assert_eq!(AgentRole::from_str("code"), Some(AgentRole::Code));
        assert_eq!(AgentRole::from_str("explorer"), Some(AgentRole::Explorer));
        assert_eq!(AgentRole::from_str("unknown"), None);
    }

    #[test]
    fn test_readonly_permissions() {
        let role = AgentRole::ReadOnly;
        let perms = role.permissions();
        assert!(perms.forbidden_commands.as_ref().unwrap().contains(&"*".to_string()));
    }
}
