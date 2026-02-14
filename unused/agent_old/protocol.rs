//! Protocol parsing for agent communication
//! 
//! This module handles parsing of various agent communication formats:
//! - Short-Key JSON Protocol (compact format with single-letter keys)
//! - ReAct format (Thought/Action/Action Input/Final Answer)
//! - Native tool calls (OpenAI-compatible function calling)

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use regex::Regex;

// =============================================================================
// Short-Key Protocol Structures
// =============================================================================

/// The "Short-Key" Request (LLM -> System)
/// Represents a single cognitive step: Thought + Action + Input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    /// Optional unique identifier for tracking parallel calls
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Thought: Internal reasoning (CoT)
    #[serde(rename = "t")]
    pub thought: String,

    /// Action: The tool name to execute
    #[serde(rename = "a")]
    pub action: String,

    /// Input: Arguments for the tool (Strict JSON)
    #[serde(rename = "i")]
    pub input: serde_json::Value,
}

/// The "Short-Key" Response (System -> LLM)
/// Represents the outcome of an action: Result OR Error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// Result: Successful output (String or JSON)
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,

    /// Error: Structured failure info
    #[serde(rename = "e", skip_serializing_if = "Option::is_none")]
    pub error: Option<AgentError>,
}

/// Structured Error Feedback
/// Designed to help the Recovery Worker diagnose issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentError {
    /// Message: Human/LLM readable description
    #[serde(rename = "m")]
    pub message: String,

    /// Code: Error type or category (e.g., "JSON_PARSE", "TIMEOUT")
    #[serde(rename = "c", skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Context: Additional data (e.g., stack trace, file path, malformed snippet)
    #[serde(rename = "x", skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

// =============================================================================
// Short-Key Action Parsing (Legacy Agent)
// =============================================================================

/// Short-Key action structure for legacy Agent
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortKeyAction {
    #[serde(rename = "t")]
    pub thought: String,
    #[serde(rename = "a")]
    pub action: Option<String>,
    #[serde(rename = "i")]
    pub input: Option<serde_json::Value>,
    #[serde(rename = "f")]
    pub final_answer: Option<String>,
}

/// Try to parse a [`ShortKeyAction`] from arbitrary model output.
///
/// This is intentionally defensive because some models/proxies produce:
/// - fenced JSON blocks (```json ... ```)
/// - JSON embedded in surrounding prose
/// - invalid JSON with literal newlines inside string values
pub fn parse_short_key_action_from_content(content: &str) -> Option<ShortKeyAction> {
    // Fast-path: entire content is a JSON object.
    if let Some(sk) = parse_short_key_action_candidate(content.trim()) {
        return Some(sk);
    }

    let mut candidates: Vec<String> = Vec::new();
    candidates.extend(extract_json_code_fence_blocks(content));
    candidates.extend(extract_balanced_json_objects(content));

    // Deduplicate; the model can repeat the same JSON multiple times.
    let mut seen: HashSet<String> = HashSet::new();
    for c in candidates {
        let trimmed = c.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !seen.insert(trimmed.to_string()) {
            continue;
        }
        if let Some(sk) = parse_short_key_action_candidate(trimmed) {
            return Some(sk);
        }
    }

    None
}

fn parse_short_key_action_candidate(candidate: &str) -> Option<ShortKeyAction> {
    match serde_json::from_str::<ShortKeyAction>(candidate) {
        Ok(v) => Some(v),
        Err(_) => {
            // Some models output invalid JSON with literal newlines inside string values.
            // Normalize it into valid JSON and try again.
            let normalized = escape_unescaped_newlines_in_json_strings(candidate);
            serde_json::from_str::<ShortKeyAction>(&normalized).ok()
        }
    }
}

/// Extract ```json ... ``` blocks.
///
/// Important: we terminate only on a *closing fence line* that is exactly ``` (plus whitespace),
/// so occurrences of ``` inside JSON string values (e.g. markdown in `f`) won't truncate.
fn extract_json_code_fence_blocks(content: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let lower = content.to_lowercase();
    let mut search_from = 0usize;

    // Closing fence must be on its own line.
    let end_fence_re = Regex::new(r"(?m)^[ \t]*```[ \t]*$").expect("valid regex");

    while let Some(rel_start) = lower[search_from..].find("```json") {
        let fence_start = search_from + rel_start;
        let after_tag = fence_start + "```json".len();

        // Fenced content begins after the next newline.
        let content_start = match content[after_tag..].find('\n') {
            Some(rel_nl) => after_tag + rel_nl + 1,
            None => break,
        };

        let hay = &content[content_start..];
        if let Some(m) = end_fence_re.find(hay) {
            let end_fence_start = content_start + m.start();
            blocks.push(content[content_start..end_fence_start].to_string());
            search_from = content_start + m.end();
        } else {
            break;
        }
    }

    blocks
}

/// Extract top-level `{ ... }` candidates by scanning with brace balancing.
///
/// Respects JSON strings and escapes, so braces inside strings don't affect balancing.
fn extract_balanced_json_objects(content: &str) -> Vec<String> {
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

/// Convert invalid JSON containing literal newlines inside string values into valid JSON.
///
/// Only escapes `\n`/\r` when inside a JSON string literal.
fn escape_unescaped_newlines_in_json_strings(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escape = false;

    for ch in input.chars() {
        if in_string {
            if escape {
                out.push(ch);
                escape = false;
                continue;
            }
            match ch {
                '\\' => {
                    out.push(ch);
                    escape = true;
                }
                '"' => {
                    out.push(ch);
                    in_string = false;
                }
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                _ => out.push(ch),
            }
        } else {
            out.push(ch);
            if ch == '"' {
                in_string = true;
            }
        }
    }

    out
}

// =============================================================================
// ReAct Protocol Parsing
// =============================================================================

/// Result of parsing ReAct format
#[derive(Debug, Clone)]
pub struct ReActAction {
    pub tool_name: String,
    pub args: String,
    pub thought: Option<String>,
    pub has_final_answer: bool,
}

/// Parse ReAct format from content
/// 
/// ReAct format looks like:
/// ```text
/// Thought: <reasoning>
/// Action: <tool_name>
/// Action Input: <arguments>
/// ```
/// 
/// Or with Final Answer:
/// ```text
/// Thought: <reasoning>
/// Final Answer: <response>
/// ```
pub fn parse_react_action(content: &str) -> Option<ReActAction> {
    // Improved ReAct parsing (handles multi-line Action Input)
    let action_re = Regex::new(r"(?m)^Action:\s*(.*)").ok()?;
    // Fix: Use non-greedy match and stop at next potential block
    let action_input_re = Regex::new(r"(?ms)^Action Input:\s*(.*?)(?:\nThought:|\nObservation:|\nFinal Answer:|\z)").ok()?;
    let final_answer_re = Regex::new(r"(?mi)Final Answer:").ok()?;

    let action_match = action_re.captures(content);
    let action_input_match = action_input_re.captures(content);

    if action_match.is_none() && action_input_match.is_none() {
        return None;
    }

    let tool_name = action_match.as_ref()
        .map(|c| c[1].trim().to_string());
    
    let args = action_input_match.as_ref().map(|caps| {
        let mut val = caps[1].trim().to_string();
        if let Some(pos) = val.find("Observation:") {
            val.truncate(pos);
        }
        val.trim().to_string()
    });

    let thought = content.find("Action:")
        .map(|pos| content[..pos].trim().to_string())
        .filter(|t| !t.is_empty());

    let has_final_answer = final_answer_re.is_match(content);

    Some(ReActAction {
        tool_name: tool_name?,
        args: args.unwrap_or_default(),
        thought,
        has_final_answer,
    })
}

/// Check if content contains malformed ReAct indicators
pub fn detect_malformed_react(content: &str) -> Option<String> {
    let has_action = content.contains("Action:");
    let has_action_input = content.contains("Action Input:");
    
    if has_action && !has_action_input {
        Some("Missing 'Action Input:' tag.".to_string())
    } else if !has_action && has_action_input {
        Some("Missing 'Action:' tag.".to_string())
    } else {
        None
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Truncate a string for logging purposes (to prevent huge log entries)
pub fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}... [truncated {} chars]", &s[..max_len], s.len() - max_len)
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_short_key_action() {
        let content = r#"{"t": "I need to read a file", "a": "read_file", "i": {"path": "/tmp/test"}}"#;
        let result = parse_short_key_action_from_content(content);
        assert!(result.is_some());
        let action = result.unwrap();
        assert_eq!(action.thought, "I need to read a file");
        assert_eq!(action.action, Some("read_file".to_string()));
    }

    #[test]
    fn test_parse_short_key_with_final_answer() {
        let content = r#"{"t": "Task complete", "f": "The answer is 42"}"#;
        let result = parse_short_key_action_from_content(content);
        assert!(result.is_some());
        let action = result.unwrap();
        assert_eq!(action.thought, "Task complete");
        assert_eq!(action.final_answer, Some("The answer is 42".to_string()));
        assert!(action.action.is_none());
    }

    #[test]
    fn test_extract_json_code_fence() {
        let content = r#"Some text
```json
{"t": "test", "a": "tool"}
```
More text"#;
        let blocks = extract_json_code_fence_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("\"t\": \"test\""));
    }

    #[test]
    fn test_parse_react_action() {
        let content = "Thought: I need to read a file\nAction: read_file\nAction Input: {\"path\": \"/tmp/test\"}";
        let result = parse_react_action(content);
        assert!(result.is_some());
        let action = result.unwrap();
        assert_eq!(action.tool_name, "read_file");
        assert!(action.args.contains("/tmp/test"));
    }

    #[test]
    fn test_truncate_for_log() {
        let s = "a".repeat(100);
        let truncated = truncate_for_log(&s, 50);
        assert!(truncated.starts_with("a"));
        assert!(truncated.contains("truncated"));
        assert!(truncated.contains("50"));
    }
}
