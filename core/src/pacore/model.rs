use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: Option<bool>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            name: None,
            tool_calls: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Choice {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Message>,
    pub finish_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
