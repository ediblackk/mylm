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
    pub fn build_terminal_pack(&self, terminal_content: &str) -> Option<ContextPack> {
        let (max_chars, label) = match self.profile {
            ContextProfile::Minimal => return None,
            ContextProfile::Balanced => (3000, "Terminal Snapshot (Recent)"),
            ContextProfile::Verbose => (12000, "Terminal Snapshot (Extended)"),
        };

        let truncated = self.truncate_lines(terminal_content, max_chars);
        if truncated.trim().is_empty() {
            return None;
        }

        Some(ContextPack::new(label, truncated))
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
