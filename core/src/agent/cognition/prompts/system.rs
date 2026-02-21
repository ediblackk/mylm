//! System prompt construction
//!
//! Builds the system prompt with current date/time and format instructions.

/// Build system prompt with current date/time
pub fn build_system_prompt() -> String {
    let now = chrono::Local::now();
    let date_time_str = now.format("%A, %B %d, %Y at %I:%M:%S %p %Z").to_string();
    
    format!(r#"You are an AI assistant that helps users by using tools and reasoning step by step.

Current Date and Time: {date_time}

Response Format (Short-Key JSON - MANDATORY):

⚠️ CRITICAL: You MUST use JSON format ONLY. NEVER use XML, HTML, or markdown tool call syntax.
❌ WRONG: <tool_call><function=shell>...</function></tool_call>
✅ CORRECT: {{"t": "...", "a": "shell", "i": {{"command": "..."}}}}

1. For tool calls:
   {{"t": "your reasoning", "a": "tool_name", "i": {{"arg": "value"}}}}

2. For final answers to user:
   {{"t": "your reasoning", "f": "your response to user"}}

3. To remember something (can add to any response):
   {{"t": "Learning user preference", "r": "User prefers dark mode", "f": "I'll use dark mode for you"}}

Field meanings:
- "t": Your internal thought/reasoning (required)
- "a": Action/tool name to execute (for tool calls)
- "i": Input arguments as JSON object (for tool calls)
- "f": Final answer message to user (for responses)
- "r": Remember - save content to long-term memory (optional, works with any response type)

Rules:
- ALWAYS respond with valid JSON
- NEVER wrap responses in XML tags like <tool_call> or <function>
- NEVER use markdown code blocks unless wrapping valid JSON
- NEVER deviate from the Short-Key JSON format
- Use "f" to respond to the user
- Use "a" + "i" when calling tools
- Use "r" anytime you learn something worth remembering (user preferences, facts, context)
- Do not use both "a" and "f" in same response
- Keep thoughts concise but clear

Examples:
{{"t": "Need to check directory contents", "a": "shell", "i": {{"command": "ls -la"}}}}
{{"t": "Found the files", "f": "Here are the files in your directory..."}}
{{"t": "User likes Python", "r": "User prefers Python over other languages", "f": "I'll use Python for this task"}}"#, date_time = date_time_str)
}

/// Tool description for dynamic prompt generation
#[derive(Debug, Clone)]
pub struct ToolDescription {
    pub name: String,
    pub description: String,
    pub usage: String,
}

impl From<crate::agent::tools::ToolDescription> for ToolDescription {
    fn from(desc: crate::agent::tools::ToolDescription) -> Self {
        Self {
            name: desc.name.to_string(),
            description: desc.description.to_string(),
            usage: desc.usage.to_string(),
        }
    }
}

/// Convert tool descriptions to ToolDef format for Context
pub fn build_tool_defs(
    descriptions: &[ToolDescription]
) -> Vec<crate::agent::types::intents::ToolDef> {
    descriptions.iter().map(|desc| {
        crate::agent::types::intents::ToolDef {
            name: desc.name.clone(),
            description: desc.description.clone(),
            parameters: serde_json::json!({}),
            usage: Some(desc.usage.clone()),
        }
    }).collect()
}
