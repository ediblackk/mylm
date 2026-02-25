//! Short-Key JSON Protocol Parser
//!
//! Handles parsing of Short-Key action format from LLM responses.
//! Supports fenced JSON blocks, inline JSON, and various normalization strategies.
//!
//! Y-Switch Design: Parser extracts ALL fields, Planner decides which tracks to take.

use serde::{Deserialize, Serialize};
use crate::agent::types::intents::ToolCall;
use super::{ParsedResponse, ParseError, extract_json_objects, extract_code_blocks};

/// Extracted Short-Key fields - flat extraction, no decisions made here.
/// 
/// This is the Y-switch source: the planner uses these fields to create
/// appropriate intents. All fields are optional - presence determines routing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShortKeyExtracted {
    /// Thought/reasoning ("t" field)
    pub thought: String,
    /// Remember content - save to memory ("r" field)
    pub remember: Option<String>,
    /// Tool call ("a" + "i" fields combined)
    pub tool_call: Option<ToolCall>,
    /// Final answer to user ("f" field)
    pub final_answer: Option<String>,
    /// Confirm flag - request approval before tool execution ("c" field)
    pub confirm: bool,
}

/// Short-Key Action representation.
///
/// Fields:
/// - `t`: Thought/reasoning (optional, default empty)
/// - `a`: Action/tool name to execute (optional)
/// - `i`: Input arguments for the action (optional)
/// - `f`: Final answer/message to user (optional)
/// - `c`: Confirm flag - when true, chat first and wait for user approval before acting
/// - `r`: Remember - save this content to memory (optional, inline fire-and-forget)
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
    /// Remember: save this content to long-term memory (inline, fire-and-forget)
    /// The memory save happens asynchronously - LLM continues immediately
    #[serde(rename = "r")]
    pub remember: Option<String>,
}

/// Parser for Short-Key JSON format
#[derive(Debug, Default)]
pub struct ShortKeyParser;

impl ShortKeyParser {
    /// Create a new ShortKeyParser
    pub fn new() -> Self {
        Self
    }

    /// Parse content and return a ParsedResponse
    ///
    /// Y-Switch Design: Extracts ALL fields from Short-Key JSON.
    /// The PLANNER decides which intents to create (remember, tool, emit, etc.)
    /// This avoids losing data when multiple fields are present (e.g., "r" + "f" + "a").
    pub fn parse_to_response(&self, content: &str) -> Result<ParsedResponse, ParseError> {
        match self.parse(content) {
            Ok(actions) => {
                if actions.is_empty() {
                    return Ok(ParsedResponse::Malformed {
                        error: "No actions found in response".to_string(),
                        raw: content.to_string(),
                    });
                }

                // Get the first action (primary action)
                let first = &actions[0];

                // Extract all fields - NO DECISIONS made here!
                // The planner (traffic controller) will decide what to do.
                let extracted = ShortKeyExtracted {
                    thought: first.thought.clone(),
                    remember: first.remember.clone(),
                    tool_call: first.action.as_ref().map(|tool_name| {
                        ToolCall {
                            name: tool_name.clone(),
                            arguments: first.input.clone().unwrap_or(serde_json::Value::Null),
                            working_dir: None,
                            timeout_secs: None,
                        }
                    }),
                    final_answer: first.final_answer.clone(),
                    confirm: first.confirm,
                };

                // Return flat extraction - planner does the Y-switch routing
                Ok(ParsedResponse::ShortKey(extracted))
            }
            Err(e) => Err(e),
        }
    }

    /// Parse one or more ShortKeyAction from content
    ///
    /// Supports batch parsing - returns Vec for single or multiple actions
    pub fn parse(&self, content: &str) -> Result<Vec<ShortKeyAction>, ParseError> {
        let trimmed = content.trim();

        // 1. Check for fenced JSON blocks first (most explicit)
        let fenced_blocks = extract_code_blocks(content, "json");
        for block in fenced_blocks {
            if let Some(actions) = self.parse_batch_or_single(&block) {
                return Ok(actions);
            }
        }

        // 2. Try parsing the whole trimmed content
        if let Some(actions) = self.parse_batch_or_single(trimmed) {
            return Ok(actions);
        }

        // 3. Extract balanced JSON objects or arrays
        let candidates = extract_json_objects(content);
        for c in candidates {
            if let Some(actions) = self.parse_batch_or_single(&c) {
                return Ok(actions);
            }
        }

        // 4. Try XML-style tool calls (e.g., <function=web_search>)
        if let Some(action) = self.parse_xml_tool_call(content) {
            return Ok(vec![action]);
        }

        // 5. If nothing worked, return a Parse Error
        Err(ParseError::new("Failed to parse Short-Key JSON from model response")
            .with_content(content))
    }

    /// Parse XML-style tool calls like:
    /// <tool_call>
    /// <function=web_search>
    /// <parameter=args>query</parameter>
    /// </function>
    /// </tool_call>
    fn parse_xml_tool_call(&self, content: &str) -> Option<ShortKeyAction> {
        // Look for <function=name> pattern
        let func_start = content.find("<function=")?;
        let func_name_start = func_start + 10; // len("<function=")
        let func_name_end = content[func_name_start..].find(">")?;
        let tool_name = &content[func_name_start..func_name_start + func_name_end];
        
        // Parse ALL parameters, not just the first one
        let mut input_map = serde_json::Map::new();
        let mut search_start = 0;
        
        while let Some(param_start) = content[search_start..].find("<parameter=") {
            let param_start = search_start + param_start;
            let param_name_start = param_start + 11; // len("<parameter=")
            
            if let Some(param_name_end) = content[param_name_start..].find(">") {
                let param_name = &content[param_name_start..param_name_start + param_name_end];
                
                // Extract value between <parameter=name> and </parameter>
                let value_start = param_name_start + param_name_end + 1;
                let value_end_marker = "</parameter>";
                
                if let Some(value_end) = content[value_start..].find(value_end_marker) {
                    let param_value = content[value_start..value_start + value_end].trim();
                    input_map.insert(param_name.to_string(), serde_json::Value::String(param_value.to_string()));
                    search_start = value_start + value_end + value_end_marker.len();
                    continue;
                }
            }
            break;
        }
        
        // Build the input object from all parsed parameters
        let input = if input_map.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::Value::Object(input_map)
        };
        
        Some(ShortKeyAction {
            thought: format!("XML tool call: {}", tool_name),
            action: Some(tool_name.to_string()),
            input: Some(input),
            final_answer: None,
            confirm: false,
            remember: None,
        })
    }

    /// Parse as batch (array) or single object
    fn parse_batch_or_single(&self, candidate: &str) -> Option<Vec<ShortKeyAction>> {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Try as array
        if let Ok(batch) = serde_json::from_str::<Vec<ShortKeyAction>>(trimmed) {
            return Some(batch);
        }

        // Try as single object
        if let Some(single) = self.parse_single(trimmed) {
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

    /// Parse a single ShortKeyAction
    fn parse_single(&self, candidate: &str) -> Option<ShortKeyAction> {
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

    /// Extract streaming content from partial JSON
    /// 
    /// This is designed for real-time streaming where JSON may be incomplete.
    /// It extracts partial "t" (thought) and "f" (final) values as they arrive.
    /// 
    /// Returns (thought_so_far, final_so_far, is_complete)
    /// 
    /// # Example
    /// ```
    /// let parser = ShortKeyParser::new();
    /// let (t, f, done) = parser.extract_streaming_content(r#"{"t": "Hel"#);
    /// assert_eq!(t, "Hel");
    /// assert_eq!(f, "");
    /// assert!(!done);
    /// ```
    pub fn extract_streaming_content(&self, partial: &str) -> (String, String, bool) {
        let mut thought = String::new();
        let mut final_answer = String::new();
        
        // Try to find "t" field value
        if let Some(t_start) = find_field_start(partial, "t") {
            thought = extract_partial_string_value(&partial[t_start..]);
        }
        
        // Try to find "f" field value
        if let Some(f_start) = find_field_start(partial, "f") {
            final_answer = extract_partial_string_value(&partial[f_start..]);
        }
        
        // Check if JSON appears complete (has closing brace)
        let is_complete = partial.trim().ends_with('}') && 
                         partial.chars().filter(|&c| c == '{').count() == 
                         partial.chars().filter(|&c| c == '}').count();
        
        (thought, final_answer, is_complete)
    }
}

/// Find the start of a field's string value in partial JSON
fn find_field_start(json: &str, field: &str) -> Option<usize> {
    // Look for pattern: "t": " or "t":
    let pattern = format!(r#""{}": ""#, field);
    if let Some(pos) = json.find(&pattern) {
        return Some(pos + pattern.len() - 1); // Point to opening quote
    }
    
    // Try without space: "t":"
    let pattern2 = format!(r#""{}":""#, field);
    if let Some(pos) = json.find(&pattern2) {
        return Some(pos + pattern2.len() - 1);
    }
    
    None
}

/// Extract a partial string value from JSON (handles escaped quotes)
fn extract_partial_string_value(input: &str) -> String {
    let mut result = String::new();
    let mut escaped = false;
    let mut in_string = false;
    
    for ch in input.chars() {
        if escaped {
            match ch {
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                _ => result.push(ch),
            }
            escaped = false;
            continue;
        }
        
        match ch {
            '\\' => {
                escaped = true;
            }
            '"' => {
                if in_string {
                    // End of string - stop here
                    return result;
                } else {
                    in_string = true;
                }
            }
            _ if in_string => result.push(ch),
            _ => {} // Outside string, ignore
        }
    }
    
    // Return accumulated content (incomplete string)
    result
}

impl super::ResponseParser for ShortKeyParser {
    fn parse(&self, content: &str) -> Result<ParsedResponse, ParseError> {
        self.parse_to_response(content)
    }
}

/// Parse Short-Key JSON action from LLM response (convenience function)
///
/// Handles:
/// - Fenced JSON blocks (```json ... ```)
/// - Inline JSON objects
/// - Normalized JSON with unescaped newlines
pub fn parse_short_key_action(content: &str) -> Option<ShortKeyAction> {
    let parser = ShortKeyParser::new();
    parser.parse_single(content.trim())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_short_key_action() {
        let content = r#"{"t": "Test thought", "a": "shell", "i": {"command": "ls"}}"#;
        let action = parse_short_key_action(content).unwrap();
        
        assert_eq!(action.thought, "Test thought");
        assert_eq!(action.action, Some("shell".to_string()));
        assert!(action.input.is_some());
        assert_eq!(action.final_answer, None);
        assert!(!action.confirm);
    }

    #[test]
    fn test_parse_final_answer() {
        let content = r#"{"t": "Thinking...", "f": "Hello user!"}"#;
        let action = parse_short_key_action(content).unwrap();
        
        assert_eq!(action.final_answer, Some("Hello user!".to_string()));
        assert_eq!(action.action, None);
    }

    #[test]
    fn test_parse_with_confirm() {
        let content = r#"{"t": "Should I?", "c": true, "a": "shell", "i": {"command": "rm"}}"#;
        let action = parse_short_key_action(content).unwrap();
        
        assert!(action.confirm);
        assert_eq!(action.action, Some("shell".to_string()));
    }

    #[test]
    fn test_parse_batch() {
        let parser = ShortKeyParser::new();
        let content = r#"[{"a": "tool1"}, {"a": "tool2"}]"#;
        let actions = parser.parse(content).unwrap();
        
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].action, Some("tool1".to_string()));
        assert_eq!(actions[1].action, Some("tool2".to_string()));
    }

    #[test]
    fn test_parse_to_response_final_answer() {
        let parser = ShortKeyParser::new();
        let content = r#"{"f": "Hello!"}"#;
        let response = parser.parse_to_response(content).unwrap();
        
        match response {
            ParsedResponse::ShortKey(extracted) => {
                assert_eq!(extracted.final_answer, Some("Hello!".to_string()));
                assert!(extracted.remember.is_none());
                assert!(extracted.tool_call.is_none());
            }
            _ => panic!("Expected ShortKey, got {:?}", response),
        }
    }

    #[test]
    fn test_parse_to_response_tool_call() {
        let parser = ShortKeyParser::new();
        let content = r#"{"t": "List files", "a": "shell", "i": {"command": "ls"}}"#;
        let response = parser.parse_to_response(content).unwrap();
        
        match response {
            ParsedResponse::ShortKey(extracted) => {
                assert!(extracted.tool_call.is_some());
                assert_eq!(extracted.tool_call.unwrap().name, "shell");
                assert_eq!(extracted.thought, "List files");
            }
            _ => panic!("Expected ShortKey, got {:?}", response),
        }
    }

    #[test]
    fn test_parse_to_response_confirm() {
        let parser = ShortKeyParser::new();
        let content = r#"{"t": "Delete file?", "c": true, "a": "shell", "i": {"command": "rm file"}}"#;
        let response = parser.parse_to_response(content).unwrap();
        
        match response {
            ParsedResponse::ShortKey(extracted) => {
                assert_eq!(extracted.thought, "Delete file?");
                assert!(extracted.confirm);
                assert!(extracted.tool_call.is_some());
                assert_eq!(extracted.tool_call.unwrap().name, "shell");
            }
            _ => panic!("Expected ShortKey, got {:?}", response),
        }
    }

    #[test]
    fn test_escape_unescaped_newlines() {
        let input = r#"{"t": "Line 1
Line 2", "f": "answer"}"#;
        let normalized = escape_unescaped_newlines_in_json_strings(input);
        
        assert!(normalized.contains("Line 1\\nLine 2"));
        // Should now be valid JSON
        let _: serde_json::Value = serde_json::from_str(&normalized).unwrap();
    }

    #[test]
    fn test_parse_from_fenced_block() {
        let parser = ShortKeyParser::new();
        let content = r#"Some text
```json
{"f": "Hello from fence"}
```
More text"#;
        
        let response = parser.parse_to_response(content).unwrap();
        match response {
            ParsedResponse::ShortKey(extracted) => assert_eq!(extracted.final_answer, Some("Hello from fence".to_string())),
            _ => panic!("Expected ShortKey from fenced block"),
        }
    }

    #[test]
    fn test_parse_invalid_json() {
        let parser = ShortKeyParser::new();
        let content = "not json at all";
        let result = parser.parse(content);
        
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_streaming_content() {
        let parser = ShortKeyParser::new();
        
        // Test partial thought
        let (t, f, done) = parser.extract_streaming_content(r#"{"t": "Hel"#);
        assert_eq!(t, "Hel");
        assert_eq!(f, "");
        assert!(!done);
        
        // Test partial thought and final
        let (t, f, done) = parser.extract_streaming_content(r#"{"t": "Hello", "f": "Wor"#);
        assert_eq!(t, "Hello");
        assert_eq!(f, "Wor");
        assert!(!done);
        
        // Test complete JSON
        let (t, f, done) = parser.extract_streaming_content(r#"{"t": "Thought", "f": "Answer"}"#);
        assert_eq!(t, "Thought");
        assert_eq!(f, "Answer");
        assert!(done);
        
        // Test only final answer
        let (t, f, done) = parser.extract_streaming_content(r#"{"f": "Just answer"}"#);
        assert_eq!(t, "");
        assert_eq!(f, "Just answer");
        assert!(done);
        
        // Test with escaped quotes
        let (t, _f, _done) = parser.extract_streaming_content(r#"{"t": "Say \"hello\"", "f": ""}"#);
        assert_eq!(t, r#"Say "hello""#);
    }

    #[test]
    fn test_parse_remember_field() {
        let parser = ShortKeyParser::new();
        // Y-SWITCH: Parser extracts ALL fields, planner decides what to do
        let content = r#"{"t": "User likes Python", "r": "User prefers Python", "f": "I'll use Python."}"#;
        let response = parser.parse_to_response(content).unwrap();
        
        match response {
            ParsedResponse::ShortKey(extracted) => {
                // Both "r" and "f" are extracted - no data loss!
                assert_eq!(extracted.remember, Some("User prefers Python".to_string()));
                assert_eq!(extracted.final_answer, Some("I'll use Python.".to_string()));
                assert_eq!(extracted.thought, "User likes Python");
            }
            _ => panic!("Expected ShortKey, got {:?}", response),
        }
    }
    
    #[test]
    fn test_parse_remember_only() {
        let parser = ShortKeyParser::new();
        // When only remember is present (no final_answer or action)
        let content = r#"{"t": "User likes Python", "r": "User prefers Python"}"#;
        let response = parser.parse_to_response(content).unwrap();
        
        match response {
            ParsedResponse::ShortKey(extracted) => {
                assert_eq!(extracted.remember, Some("User prefers Python".to_string()));
                assert!(extracted.final_answer.is_none());
                assert!(extracted.tool_call.is_none());
            }
            _ => panic!("Expected ShortKey, got {:?}", response),
        }
    }

    // Note: Recall (rr) has been removed from the protocol
    // Memory is now injected proactively before LLM calls
    // Only Remember (r) remains as fire-and-forget

    #[test]
    fn test_parse_remember_and_call() {
        let parser = ShortKeyParser::new();
        let content = r#"{"t": "Save pref and run", "r": "User likes dark mode", "a": "shell", "i": {"command": "ls"}}"#;
        let response = parser.parse_to_response(content).unwrap();
        
        match response {
            ParsedResponse::ShortKey(extracted) => {
                // Y-SWITCH: All fields extracted - planner creates multiple intents!
                assert_eq!(extracted.remember, Some("User likes dark mode".to_string()));
                assert!(extracted.tool_call.is_some());
                assert_eq!(extracted.tool_call.unwrap().name, "shell");
            }
            _ => panic!("Expected ShortKey, got {:?}", response),
        }
    }
    
    #[test]
    fn test_parse_y_switch_all_fields() {
        let parser = ShortKeyParser::new();
        // Test with ALL fields present - Y-switch extracts everything
        let content = r#"{"t": "Full response", "r": "Important fact", "a": "write_file", "i": {"path": "/tmp/test", "content": "hello"}, "f": "Done!", "c": true}"#;
        let response = parser.parse_to_response(content).unwrap();
        
        match response {
            ParsedResponse::ShortKey(extracted) => {
                assert_eq!(extracted.thought, "Full response");
                assert_eq!(extracted.remember, Some("Important fact".to_string()));
                assert!(extracted.tool_call.is_some());
                assert_eq!(extracted.tool_call.unwrap().name, "write_file");
                assert_eq!(extracted.final_answer, Some("Done!".to_string()));
                assert!(extracted.confirm);
            }
            _ => panic!("Expected ShortKey, got {:?}", response),
        }
    }

    #[test]
    fn test_parse_short_key_action_with_memory() {
        let content = r#"{"t": "Learning", "r": "Important fact", "a": "shell", "i": {"command": "ls"}}"#;
        let action = parse_short_key_action(content).unwrap();
        
        assert_eq!(action.thought, "Learning");
        assert_eq!(action.remember, Some("Important fact".to_string()));
        assert_eq!(action.action, Some("shell".to_string()));
    }
}
