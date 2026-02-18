//! Response parsers for LLM output
//!
//! This module provides parsers for various LLM response formats.
//! All parsers are pure functions - no async, no IO.

mod short_key;

pub use short_key::{ShortKeyParser, parse_short_key_action, ShortKeyAction};

use crate::agent::types::intents::ToolCall;

/// Result of parsing an LLM response
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedResponse {
    /// One or more tool calls to execute
    ToolCalls(Vec<ToolCall>),
    /// Final answer to present to user
    FinalAnswer(String),
    /// Request for confirmation before acting (ReAct style)
    ConfirmRequest { 
        /// The agent's thought/reasoning
        thought: String, 
        /// The tool to execute if approved
        tool: ToolCall 
    },
    /// Remember content to memory (inline fire-and-forget save)
    /// The memory save happens asynchronously - no waiting
    Remember { 
        /// Content to save
        content: String,
        /// Optional associated action to perform (happens concurrently)
        next_action: Option<ToolCall>,
    },
    /// Combined action: remember + tool call (fire-and-forget)
    RememberAndCall {
        /// Content to save
        content: String,
        /// Tool call to execute
        tool: ToolCall,
    },
    /// Malformed response that couldn't be parsed
    Malformed { 
        /// Error message describing what went wrong
        error: String, 
        /// Raw content for debugging
        raw: String 
    },
}

/// Trait for response parsers
/// 
/// Implementors must be pure functions - no side effects, no async, no IO.
pub trait ResponseParser {
    /// Parse LLM response content
    ///
    /// # Arguments
    /// * `content` - Raw LLM response text
    ///
    /// # Returns
    /// ParsedResponse enum containing the interpreted action
    fn parse(&self, content: &str) -> Result<ParsedResponse, ParseError>;
}

/// Error during parsing
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub raw_content: Option<String>,
}

impl ParseError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            raw_content: None,
        }
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.raw_content = Some(content.into());
        self
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ParseError: {}", self.message)
    }
}

impl std::error::Error for ParseError {}

/// Extract JSON objects from text using brace balancing
///
/// Handles nested braces and escaped quotes within strings.
pub fn extract_json_objects(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_string = false;
    let mut escape = false;
    let mut depth: i32 = 0;
    let mut start: Option<usize> = None;
    
    for (i, ch) in content.char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        
        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start.take() {
                            out.push(content[s..=i].to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    
    out
}

/// Extract fenced code blocks (```json ... ```)
pub fn extract_code_blocks(content: &str, language: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let fence_pattern = format!("```{}\n", language);
    let lower = content.to_lowercase();
    
    let mut search_from = 0usize;
    
    while let Some(rel_start) = lower[search_from..].find(&fence_pattern) {
        let fence_start = search_from + rel_start;
        let content_start = fence_start + fence_pattern.len();
        
        // Find closing fence
        if let Some(rel_end) = lower[content_start..].find("```") {
            let content_end = content_start + rel_end;
            blocks.push(content[content_start..content_end].trim().to_string());
            search_from = content_end + 3;
        } else {
            break;
        }
    }
    
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_objects() {
        let content = r#"Some text {"key": "value"} more text {"num": 42}"#;
        let objects = extract_json_objects(content);
        
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0], r#"{"key": "value"}"#);
        assert_eq!(objects[1], r#"{"num": 42}"#);
    }

    #[test]
    fn test_extract_json_objects_nested() {
        let content = r#"{"outer": {"inner": "value"}}"#;
        let objects = extract_json_objects(content);
        
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0], r#"{"outer": {"inner": "value"}}"#);
    }

    #[test]
    fn test_extract_code_blocks() {
        let content = r#"Text before
```json
{"key": "value"}
```
Text after"#;
        
        let blocks = extract_code_blocks(content, "json");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], r#"{"key": "value"}"#);
    }

    #[test]
    fn test_parse_error_display() {
        let err = ParseError::new("Invalid JSON");
        assert_eq!(err.to_string(), "ParseError: Invalid JSON");
    }
}
