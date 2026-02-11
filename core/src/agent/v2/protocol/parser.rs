//! Short-Key JSON Protocol Parser
//! 
//! Handles parsing of Short-Key action format from LLM responses.
//! Supports fenced JSON blocks, inline JSON, and various normalization strategies.

use regex::Regex;
use serde::{Deserialize, Serialize};
use crate::agent::protocol::AgentError;

/// Short-Key Action representation.
///
/// Fields:
/// - `t`: Thought/reasoning (optional, default empty)
/// - `a`: Action/tool name to execute (optional)
/// - `i`: Input arguments for the action (optional)
/// - `f`: Final answer/message to user (optional)
/// - `c`: Confirm flag - when true, chat first and wait for user approval before acting
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortKeyAction {
    #[serde(rename = "t", default)]
    pub thought: String,
    #[serde(rename = "a")]
    pub action: Option<String>,
    #[serde(rename = "i")]
    pub input: Option<serde_json::Value>,
    #[serde(rename = "f")]
    pub final_answer: Option<String>,
    /// Confirm flag: when true, present the thought/action to user first,
    /// store the action as pending, and wait for approval before executing.
    #[serde(rename = "c", default)]
    pub confirm: bool,
}

/// Try to parse one or more [`ShortKeyAction`] from arbitrary model output.
///
/// This is intentionally defensive because some models/proxies produce:
/// - fenced JSON blocks (```json ... ```)
/// - JSON embedded in surrounding prose
/// - invalid JSON with literal newlines inside string values
pub fn parse_short_key_actions_from_content(content: &str) -> Result<Vec<ShortKeyAction>, AgentError> {
    let trimmed = content.trim();

    // 1. Check for fenced JSON blocks first (most explicit)
    let fenced_blocks = extract_json_code_fence_blocks(content);
    for block in fenced_blocks {
        if let Some(actions) = parse_batch_or_single(&block) {
            return Ok(actions);
        }
    }

    // 2. Try parsing the whole trimmed content
    if let Some(actions) = parse_batch_or_single(trimmed) {
        return Ok(actions);
    }

    // 3. Extract balanced JSON objects or arrays
    let candidates = extract_balanced_json_structures(content);
    for c in candidates {
        if let Some(actions) = parse_batch_or_single(&c) {
            return Ok(actions);
        }
    }

    // 4. If nothing worked, return a Parse Error
    Err(AgentError {
        message: "Failed to parse Short-Key JSON from model response.".to_string(),
        code: Some("PARSE_ERROR".to_string()),
        context: Some(serde_json::json!({ "raw_content": content })),
    })
}

fn parse_batch_or_single(candidate: &str) -> Option<Vec<ShortKeyAction>> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try as array
    if let Ok(batch) = serde_json::from_str::<Vec<ShortKeyAction>>(trimmed) {
        return Some(batch);
    }

    // Try as single object
    if let Some(single) = parse_short_key_action_candidate(trimmed) {
        return Some(vec![single]);
    }

    // Try normalization for both
    let normalized = escape_unescaped_newlines_in_json_strings(trimmed);
    if let Ok(batch) = serde_json::from_str::<Vec<ShortKeyAction>>(&normalized) {
        return Some(batch);
    }
    if let Ok(single) = serde_json::from_str::<ShortKeyAction>(&normalized) {
        return Some(vec![single]);
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

/// Extract top-level `{ ... }` or `[ ... ]` candidates by scanning with brace/bracket balancing.
///
/// Respects JSON strings and escapes, so braces inside strings don't affect balancing.
fn extract_balanced_json_structures(content: &str) -> Vec<String> {
    let mut out = Vec::new();

    let mut in_string = false;
    let mut escape = false;
    let mut brace_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
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
                if brace_depth == 0 && bracket_depth == 0 {
                    start = Some(i);
                }
                brace_depth += 1;
            }
            '}' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                    if brace_depth == 0 && bracket_depth == 0 {
                        if let Some(s) = start.take() {
                            out.push(content[s..=i].to_string());
                        }
                    }
                }
            }
            '[' => {
                if brace_depth == 0 && bracket_depth == 0 {
                    start = Some(i);
                }
                bracket_depth += 1;
            }
            ']' => {
                if bracket_depth > 0 {
                    bracket_depth -= 1;
                    if brace_depth == 0 && bracket_depth == 0 {
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
/// Only escapes `\n`/`\r` when inside a JSON string literal.
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

#[cfg(test)]
mod tests {
    use super::ShortKeyAction;

    #[test]
    fn short_key_action_parses_without_thought() {
        let v: ShortKeyAction = serde_json::from_str(r#"{"f":"test"}"#)
            .expect("ShortKeyAction should parse without 't'");
        assert_eq!(v.thought, "");
        assert_eq!(v.final_answer.as_deref(), Some("test"));
    }

    #[test]
    fn parse_single_action() {
        let content = r#"{"t": "I need to list files.", "a": "execute_command", "i": "ls"}"#;
        let actions = super::parse_short_key_actions_from_content(content).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].thought, "I need to list files.");
        assert_eq!(actions[0].action, Some("execute_command".to_string()));
    }

    #[test]
    fn parse_batch_actions() {
        let content = r#"[{"t": "Check config", "a": "read_file", "i": "config.json"}, {"t": "Check logs", "a": "read_file", "i": "logs.txt"}]"#;
        let actions = super::parse_short_key_actions_from_content(content).unwrap();
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn parse_fenced_json() {
        let content = r#"Some text before
```json
{"t": "Test", "f": "Final answer"}
```
Some text after"#;
        let actions = super::parse_short_key_actions_from_content(content).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].final_answer, Some("Final answer".to_string()));
    }

    #[test]
    fn parse_with_newlines_in_strings() {
        let content = "{\"t\": \"Line 1\nLine 2\", \"f\": \"answer\"}";
        let actions = super::parse_short_key_actions_from_content(content).unwrap();
        assert_eq!(actions[0].thought, "Line 1\nLine 2");
    }
}
