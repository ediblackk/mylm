//! Command safety analysis module

/// Safety level for a command
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum CommandSafety {
    /// Command is safe to execute
    Safe,
    /// Command is potentially dangerous
    Dangerous(String),
}

#[allow(dead_code)]
impl CommandSafety {
    /// Check if the command is dangerous
    pub fn is_dangerous(&self) -> bool {
        matches!(self, CommandSafety::Dangerous(_))
    }

    /// Get the reason for danger
    pub fn reason(&self) -> String {
        match self {
            CommandSafety::Safe => "Safe".to_string(),
            CommandSafety::Dangerous(reason) => reason.clone(),
        }
    }
}

/// Checker for command safety
#[derive(Debug, Default)]
pub struct SafetyChecker;

#[allow(dead_code)]
impl SafetyChecker {
    /// Create a new safety checker
    pub fn new() -> Self {
        SafetyChecker
    }

    /// Assess the safety of a command
    pub fn assess(&self, command_str: &str, command: &str, args: &[String]) -> CommandSafety {
        // List of dangerous commands/patterns
        let dangerous_patterns = [
            ("rm", "-rf"),
            ("rm", "-r"),
            ("mkfs", ""),
            ("dd", "if="),
            ("shred", ""),
            ("chmod", "-R"),
            ("chown", "-R"),
        ];

        for (cmd, pattern) in dangerous_patterns {
            if command == cmd && (pattern.is_empty() || args.iter().any(|arg| arg.contains(pattern))) {
                return CommandSafety::Dangerous(format!(
                    "Command '{}' matches dangerous pattern: {} {}",
                    command_str, cmd, pattern
                ));
            }
        }

        // Check for common destructive patterns in the whole string
        let destructive_patterns = [
            "> /dev/sda",
            ":(){:|:&};:",
            "mv /",
            "rm -rf /",
        ];

        for pattern in destructive_patterns {
            if command_str.contains(pattern) {
                return CommandSafety::Dangerous(format!(
                    "Command contains destructive pattern: {}",
                    pattern
                ));
            }
        }

        CommandSafety::Safe
    }
}
