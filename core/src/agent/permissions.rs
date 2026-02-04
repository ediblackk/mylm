use crate::config::v2::types::AgentPermissions;

/// Custom error for permission violations
#[derive(Debug, Clone)]
pub enum PermissionError {
    ToolNotAllowed { tool_name: String },
    CommandForbidden { command: String, pattern: String },
}

impl std::fmt::Display for PermissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PermissionError::ToolNotAllowed { tool_name } => {
                write!(f, "Tool '{}' is not allowed by agent permissions", tool_name)
            }
            PermissionError::CommandForbidden { command, pattern } => {
                write!(f, "Command '{}' matches forbidden pattern '{}'", command, pattern)
            }
        }
    }
}

impl std::error::Error for PermissionError {}

/// Check if a command string matches a glob pattern.
/// Pattern format: '*' matches any sequence of characters, '?' matches single character.
/// Examples: "ls *" matches "ls -la", "echo *" matches "echo hello"
pub fn matches_pattern(command: &str, pattern: &str) -> bool {
    // Simple glob implementation: convert pattern to regex-like matching
    // For now, use basic string operations: '*' becomes ".*" equivalent

    let pattern = pattern.trim();
    let command = command.trim();

    // If pattern is exactly "*", match everything
    if pattern == "*" {
        return true;
    }

    // If pattern contains no wildcards, do exact match
    if !pattern.contains('*') && !pattern.contains('?') {
        return command == pattern;
    }

    // Simple wildcard matching: split by '*' and check each segment
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        // Single segment with no '*' (but may have '?')
        return simple_char_match(command, pattern);
    }

    // Check if pattern starts with *
    let mut pos = 0;
    let mut part_iter = parts.iter().enumerate();

    // First part: if not empty, must match at start
    if let Some((0, first)) = part_iter.next() {
        if !first.is_empty() {
            if !command.starts_with(first) {
                return false;
            }
            pos = first.len();
        }
    }

    // Middle parts: must appear in order
    for (_i, part) in part_iter {
        if part.is_empty() {
            continue; // consecutive * or trailing *
        }
        if let Some(found) = command[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }

    // Last part: if not empty, must match at end
    if let Some((last_idx, last)) = parts.iter().enumerate().last() {
        if last_idx > 0 && !last.is_empty() {
            if !command.ends_with(last) {
                return false;
            }
        }
    }

    true
}

/// Simple character-by-character matching with '?' wildcard
fn simple_char_match(command: &str, pattern: &str) -> bool {
    if command.len() != pattern.len() {
        return false;
    }
    for (c, p) in command.chars().zip(pattern.chars()) {
        if p != '?' && c != p {
            return false;
        }
    }
    true
}

/// Check if a command is allowed to execute based on permissions.
/// Returns Ok(()) if allowed, Err(PermissionError) if forbidden.
/// If auto_approve matches, returns Ok(()) without requiring confirmation.
/// If forbidden matches, returns Err.
/// Otherwise returns Ok(()) but caller should still do normal confirmation.
pub fn check_command_permission(
    command: &str,
    permissions: &AgentPermissions,
) -> Result<(), PermissionError> {
    // Check forbidden commands first (highest priority)
    if let Some(forbidden) = &permissions.forbidden_commands {
        for pattern in forbidden {
            if matches_pattern(command, pattern) {
                return Err(PermissionError::CommandForbidden {
                    command: command.to_string(),
                    pattern: pattern.clone(),
                });
            }
        }
    }

    // If auto_approve matches, it's explicitly allowed (but still must not be forbidden)
    // We just return Ok - caller can use this to skip confirmation
    if let Some(auto_approve) = &permissions.auto_approve_commands {
        for pattern in auto_approve {
            if matches_pattern(command, pattern) {
                return Ok(());
            }
        }
    }

    // If we reach here and auto_approve is Some, command didn't match any auto_approve pattern
    // But it's not forbidden either, so it's allowed with normal confirmation
    // (If auto_approve is None, all commands are allowed with confirmation)
    Ok(())
}

/// Check if a tool name is allowed by permissions.
/// Returns Ok(()) if allowed, Err if not allowed.
pub fn check_tool_permission(
    tool_name: &str,
    permissions: &AgentPermissions,
) -> Result<(), PermissionError> {
    if let Some(allowed_tools) = &permissions.allowed_tools {
        // If allowed_tools list exists, tool must be in it
        if !allowed_tools.iter().any(|allowed| {
            // Use exact match for tool names (case-sensitive for now)
            allowed == tool_name
        }) {
            return Err(PermissionError::ToolNotAllowed {
                tool_name: tool_name.to_string(),
            });
        }
    }
    Ok(())
}
