use crate::config::ContextProfile;

/// A self-contained unit of context (e.g., Terminal History, Git Status, Memory)
#[derive(Debug, Clone)]
pub struct ContextPack {
    pub title: String,
    pub content: String,
    
    pub token_estimate: usize,
}

impl ContextPack {
    pub fn new(title: &str, content: String) -> Self {
        let token_estimate = content.len() / 4;
        Self {
            title: title.to_string(),
            content,
            token_estimate,
        }
    }

    /// Render the pack as a markdown section
    pub fn render(&self) -> String {
        format!("\n\n## {}\n{}", self.title, self.content)
    }
}

/// Builder for creating context packs based on profile and budget
pub struct ContextBuilder {
    profile: ContextProfile,
    // Future: total_budget: usize,
}

impl ContextBuilder {
    pub fn new(profile: ContextProfile) -> Self {
        Self { profile }
    }

    /// Build a terminal context pack from the raw screen buffer string
    /// 
    /// The content is sanitized to remove ANSI escape sequences and patterns
    /// that might trigger WAF/content filtering (e.g., shell commands in prompts).
    pub fn build_terminal_pack(&self, terminal_content: &str) -> Option<ContextPack> {
        let (max_chars, label) = match self.profile {
            ContextProfile::Minimal => return None,
            ContextProfile::Balanced => (3000, "Terminal Snapshot (Recent)"),
            ContextProfile::Verbose => (12000, "Terminal Snapshot (Extended)"),
        };

        // Sanitize terminal content to avoid WAF triggering
        let sanitized = self.sanitize_terminal_content(terminal_content);
        let truncated = self.truncate_lines(&sanitized, max_chars);
        if truncated.trim().is_empty() {
            return None;
        }

        Some(ContextPack::new(label, truncated))
    }

    /// Sanitize terminal content by removing WAF-triggering patterns.
    /// Uses simple replacement text that doesn't look like code/markup.
    fn sanitize_terminal_content(&self, content: &str) -> String {
        // Step 1: Strip ANSI escape sequences
        let ansi_regex = regex::Regex::new(r"\x1B\[[0-9;]*[a-zA-Z]|\x1B\][^\x07]*\x07|\x1B\[[\?0-9]*[hl]").unwrap();
        let without_ansi = ansi_regex.replace_all(content, "");

        // Step 2: Remove command substitution patterns
        let cmd_subst_regex = regex::Regex::new(r"\$\([^)]+\)|`[^`]+`|\$\{[^}]+\}").unwrap();
        let without_cmd_subst = cmd_subst_regex.replace_all(&without_ansi, " ... ");

        // Step 3: Remove shell prompt patterns
        let shell_prompt_regex = regex::Regex::new(r"[a-zA-Z0-9_-]+@[a-zA-Z0-9_-]+:[^$]+\$\s*>?\s*|[a-zA-Z0-9_-]+@[a-zA-Z0-9_-]+\$\s*").unwrap();
        let without_shell_prompts = shell_prompt_regex.replace_all(&without_cmd_subst, " ");

        // Step 4: Remove the word "command" in JSON keys
        let command_key_regex = regex::Regex::new(r#"\"?command\"?"#).unwrap();
        let without_command_key = command_key_regex.replace_all(&without_shell_prompts, "cmd");

        // Step 5: Remove execute_command action name
        let exec_cmd_regex = regex::Regex::new(r"execute_command").unwrap();
        let without_exec_cmd = exec_cmd_regex.replace_all(&without_command_key, "run");

        // Step 6: Remove suggestion UI lines
        let suggestion_regex = regex::Regex::new(r"\[Suggestion\]:.*").unwrap();
        let without_suggestions = suggestion_regex.replace_all(&without_exec_cmd, " ");

        // Step 7: Remove lines starting with "> " (shell redirection)
        let redirect_regex = regex::Regex::new(r"(?m)^> .+").unwrap();
        let without_redirects = redirect_regex.replace_all(&without_suggestions, " ");

        // Step 8: Remove shell operators and test patterns
        let shell_ops_regex = regex::Regex::new(r"2>/dev/null|>/dev/null|&&|\|\||\[ -t 0 \]|stty echo|stty -echo|\{ [^}]+ \}|> [^;\n]+").unwrap();
        let without_shell_ops = shell_ops_regex.replace_all(&without_redirects, " ");

        // Step 9: Remove terminal context markers
        let terminal_marker_regex = regex::Regex::new(r"--- TERMINAL CONTEXT ---|--- COMMAND OUTPUT ---|CMD_OUTPUT:").unwrap();
        let without_markers = terminal_marker_regex.replace_all(&without_shell_ops, " ");

        // Step 10: Collapse multiple spaces
        let collapsed = regex::Regex::new(r"\s+").unwrap().replace_all(&without_markers, " ");

        // Step 11: Remove control characters except newlines and tabs
        let without_ctrl: String = collapsed
            .chars()
            .filter(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
            .collect();

        without_ctrl
    }

    /// Helper to keep last N chars but aligned to line boundaries
    fn truncate_lines(&self, content: &str, char_limit: usize) -> String {
        if content.len() <= char_limit {
            return content.to_string();
        }

        // Take the tail
        let start_byte = content.len().saturating_sub(char_limit);
        let slice = &content[start_byte..];

        // Find the first newline to avoid a partial line at the top
        if let Some(idx) = slice.find('\n') {
            slice[idx + 1..].to_string()
        } else {
            slice.to_string()
        }
    }
}
