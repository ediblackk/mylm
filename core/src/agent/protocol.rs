use serde::{Deserialize, Serialize};

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
