//! Action Stamp System
//!
//! Provides persistent visual indicators in the chat pane for agent actions.
//! These stamps remain visible as a history of what the agent has done,
//! including tool executions, condensations, and successful operations.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Types of action stamps that can be recorded
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionStampType {
    /// Tool was executed successfully
    ToolSuccess,
    /// Tool execution failed
    ToolFailed,
    /// Context was condensed/summarized
    ContextCondensed,
    /// Memory was accessed/recalled
    MemoryRecalled,
    /// File was read
    FileRead,
    /// File was written/modified
    FileWritten,
    /// Command was executed
    CommandExecuted,
    /// Web search was performed
    WebSearch,
    /// Thinking/planning step
    Thinking,
    /// Task completed successfully
    TaskComplete,
}

impl ActionStampType {
    /// Get the emoji icon for this stamp type
    pub fn icon(&self) -> &'static str {
        match self {
            ActionStampType::ToolSuccess => "✓",
            ActionStampType::ToolFailed => "✗",
            ActionStampType::ContextCondensed => "📝",
            ActionStampType::MemoryRecalled => "🧠",
            ActionStampType::FileRead => "📖",
            ActionStampType::FileWritten => "💾",
            ActionStampType::CommandExecuted => "⚡",
            ActionStampType::WebSearch => "🔍",
            ActionStampType::Thinking => "💭",
            ActionStampType::TaskComplete => "✅",
        }
    }

    /// Get the color code for terminal display (ratatui Color name)
    pub fn color_name(&self) -> &'static str {
        match self {
            ActionStampType::ToolSuccess => "green",
            ActionStampType::ToolFailed => "red",
            ActionStampType::ContextCondensed => "yellow",
            ActionStampType::MemoryRecalled => "magenta",
            ActionStampType::FileRead => "cyan",
            ActionStampType::FileWritten => "blue",
            ActionStampType::CommandExecuted => "yellow",
            ActionStampType::WebSearch => "cyan",
            ActionStampType::Thinking => "dark_gray",
            ActionStampType::TaskComplete => "green",
        }
    }
}

/// A persistent action stamp recording an agent action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionStamp {
    /// Type of action
    pub stamp_type: ActionStampType,
    /// Brief title/summary (e.g., tool name, filename)
    pub title: String,
    /// Optional detailed description
    pub detail: Option<String>,
    /// When the action occurred
    pub timestamp: DateTime<Utc>,
    /// Token usage for this action (if applicable)
    pub token_usage: Option<(usize, usize)>, // (input, output)
    /// Cost for this action (if applicable)
    pub cost: Option<f64>,
}

impl ActionStamp {
    /// Create a new action stamp
    pub fn new(stamp_type: ActionStampType, title: impl Into<String>) -> Self {
        Self {
            stamp_type,
            title: title.into(),
            detail: None,
            timestamp: Utc::now(),
            token_usage: None,
            cost: None,
        }
    }

    /// Add detail description
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Add token usage information
    pub fn with_usage(mut self, input: usize, output: usize) -> Self {
        self.token_usage = Some((input, output));
        self
    }

    /// Add cost information
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost = Some(cost);
        self
    }

    /// Format for display in the UI
    pub fn format_for_display(&self) -> String {
        let icon = self.stamp_type.icon();
        let mut result = format!("[{} {}]", icon, self.title);
        
        if let Some(ref detail) = self.detail {
            if !detail.is_empty() {
                result.push_str(&format!(" {}", detail));
            }
        }
        
        if let Some((input, output)) = self.token_usage {
            result.push_str(&format!(" ({}→{} tokens)", input, output));
        }
        
        if let Some(cost) = self.cost {
            if cost > 0.0 {
                result.push_str(&format!(" ${:.4}", cost));
            }
        }
        
        result
    }

    /// Format as a compact single-line stamp
    pub fn format_compact(&self) -> String {
        let icon = self.stamp_type.icon();
        format!("[{} {}]", icon, self.title)
    }
}

/// Registry for tracking action stamps in a conversation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionStampRegistry {
    stamps: Vec<ActionStamp>,
    max_stamps: usize,
}

impl ActionStampRegistry {
    /// Create a new registry with a maximum number of stamps to keep
    pub fn new(max_stamps: usize) -> Self {
        Self {
            stamps: Vec::new(),
            max_stamps,
        }
    }

    /// Add a new stamp to the registry
    pub fn add(&mut self, stamp: ActionStamp) {
        self.stamps.push(stamp);
        
        // Trim old stamps if exceeding max
        if self.stamps.len() > self.max_stamps {
            let excess = self.stamps.len() - self.max_stamps;
            self.stamps.drain(0..excess);
        }
    }

    /// Get all stamps
    pub fn all(&self) -> &[ActionStamp] {
        &self.stamps
    }

    /// Get recent stamps (last N)
    pub fn recent(&self, count: usize) -> &[ActionStamp] {
        let start = self.stamps.len().saturating_sub(count);
        &self.stamps[start..]
    }

    /// Clear all stamps
    pub fn clear(&mut self) {
        self.stamps.clear();
    }

    /// Get count of stamps
    pub fn len(&self) -> usize {
        self.stamps.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.stamps.is_empty()
    }

    /// Get stamps by type
    pub fn by_type(&self, stamp_type: ActionStampType) -> Vec<&ActionStamp> {
        self.stamps
            .iter()
            .filter(|s| s.stamp_type == stamp_type)
            .collect()
    }

    /// Get total cost of all stamps
    pub fn total_cost(&self) -> f64 {
        self.stamps.iter().filter_map(|s| s.cost).sum()
    }

    /// Get total token usage across all stamps
    pub fn total_tokens(&self) -> (usize, usize) {
        self.stamps
            .iter()
            .filter_map(|s| s.token_usage)
            .fold((0, 0), |(acc_in, acc_out), (in_tok, out_tok)| {
                (acc_in + in_tok, acc_out + out_tok)
            })
    }
}

/// Helper functions for creating common stamps
pub mod stamps {
    use super::*;

    /// Create a tool success stamp
    pub fn tool_success(tool_name: &str, detail: Option<&str>) -> ActionStamp {
        let mut stamp = ActionStamp::new(ActionStampType::ToolSuccess, tool_name.to_string());
        if let Some(d) = detail {
            stamp.detail = Some(d.to_string());
        }
        stamp
    }

    /// Create a tool failure stamp
    pub fn tool_failed(tool_name: &str, error: &str) -> ActionStamp {
        ActionStamp::new(ActionStampType::ToolFailed, tool_name.to_string())
            .with_detail(format!("Error: {}", error))
    }

    /// Create a context condensation stamp
    pub fn context_condensed(before_tokens: usize, after_tokens: usize) -> ActionStamp {
        let saved = before_tokens.saturating_sub(after_tokens);
        ActionStamp::new(ActionStampType::ContextCondensed, "Context Condensed".to_string())
            .with_detail(format!("{}→{} tokens (saved {})", before_tokens, after_tokens, saved))
    }

    /// Create a memory recall stamp
    pub fn memory_recalled(count: usize) -> ActionStamp {
        ActionStamp::new(ActionStampType::MemoryRecalled, format!("Recalled {} memories", count))
    }

    /// Create a file read stamp
    pub fn file_read(path: &str) -> ActionStamp {
        ActionStamp::new(ActionStampType::FileRead, format!("Read: {}", path))
    }

    /// Create a file written stamp
    pub fn file_written(path: &str) -> ActionStamp {
        ActionStamp::new(ActionStampType::FileWritten, format!("Wrote: {}", path))
    }

    /// Create a command executed stamp
    pub fn command_executed(cmd: &str) -> ActionStamp {
        let truncated = if cmd.len() > 40 {
            format!("{}...", &cmd[..37])
        } else {
            cmd.to_string()
        };
        ActionStamp::new(ActionStampType::CommandExecuted, truncated)
    }

    /// Create a web search stamp
    pub fn web_search(query: &str, results: usize) -> ActionStamp {
        ActionStamp::new(ActionStampType::WebSearch, format!("Search: {} results", results))
            .with_detail(format!("Query: {}", query))
    }

    /// Create a thinking stamp
    pub fn thinking(thought: &str) -> ActionStamp {
        let truncated = if thought.len() > 50 {
            format!("{}...", &thought[..47])
        } else {
            thought.to_string()
        };
        ActionStamp::new(ActionStampType::Thinking, truncated)
    }

    /// Create a task complete stamp
    pub fn task_complete(summary: &str) -> ActionStamp {
        ActionStamp::new(ActionStampType::TaskComplete, "Task Complete".to_string())
            .with_detail(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_stamp_creation() {
        let stamp = ActionStamp::new(ActionStampType::ToolSuccess, "read_file")
            .with_detail("Read config.toml")
            .with_usage(100, 50)
            .with_cost(0.0015);

        assert_eq!(stamp.stamp_type, ActionStampType::ToolSuccess);
        assert_eq!(stamp.title, "read_file");
        assert!(stamp.detail.is_some());
        assert_eq!(stamp.token_usage, Some((100, 50)));
        assert_eq!(stamp.cost, Some(0.0015));
    }

    #[test]
    fn test_action_stamp_format() {
        let stamp = ActionStamp::new(ActionStampType::FileWritten, "config.toml");
        let formatted = stamp.format_compact();
        assert!(formatted.contains("💾"));
        assert!(formatted.contains("config.toml"));
    }

    #[test]
    fn test_registry() {
        let mut registry = ActionStampRegistry::new(10);
        
        registry.add(stamps::tool_success("read_file", Some("test.txt")));
        registry.add(stamps::file_written("output.txt"));
        
        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());
        
        let file_stamps = registry.by_type(ActionStampType::FileWritten);
        assert_eq!(file_stamps.len(), 1);
    }

    #[test]
    fn test_registry_max_limit() {
        let mut registry = ActionStampRegistry::new(3);
        
        for i in 0..5 {
            registry.add(ActionStamp::new(ActionStampType::Thinking, format!("thought {}", i)));
        }
        
        assert_eq!(registry.len(), 3);
    }
}
