use anyhow::Result;
use async_trait::async_trait;
use crate::agent::tool::{Tool, ToolOutput, ToolKind};
use serde::{Deserialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::RwLock;

#[derive(Clone)]
pub struct ScratchpadTool {
    scratchpad: Arc<RwLock<String>>,
}

impl ScratchpadTool {
    pub fn new(scratchpad: Arc<RwLock<String>>) -> Self {
        Self { scratchpad }
    }
}

#[derive(Debug, Deserialize)]
struct ScratchpadArgs {
    text: Option<String>,
    #[serde(default = "default_action")]
    action: String,
}

fn default_action() -> String {
    "overwrite".to_string()
}

#[async_trait]
impl Tool for ScratchpadTool {
    fn name(&self) -> &str {
        "scratchpad"
    }

    fn description(&self) -> &str {
        "Manage a persistent scratchpad for storing short-term memory, plans, and todos."
    }

    fn usage(&self) -> &str {
        r#"
        {
            "text": "content to store",
            "action": "overwrite" | "append" | "clear"
        }
        "#
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Content to store in the scratchpad"
                },
                "action": {
                    "type": "string",
                    "enum": ["overwrite", "append", "clear"],
                    "description": "Action to perform on the scratchpad (default: overwrite)"
                }
            },
            "required": ["action"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn std::error::Error + Send + Sync>> {
        let args: ScratchpadArgs = serde_json::from_str(args)?;
        let mut scratchpad = self.scratchpad.write().map_err(|_| anyhow::anyhow!("Scratchpad lock poisoned"))?;

        match args.action.as_str() {
            "overwrite" => {
                if let Some(text) = args.text {
                    *scratchpad = text;
                    Ok(ToolOutput::Immediate(serde_json::Value::String("Scratchpad updated (overwritten).".to_string())))
                } else {
                    Ok(ToolOutput::Immediate(serde_json::Value::String("Error: 'text' is required for overwrite action.".to_string())))
                }
            },
            "append" => {
                if let Some(text) = args.text {
                    if !scratchpad.is_empty() {
                        scratchpad.push('\n');
                    }
                    scratchpad.push_str(&text);
                    Ok(ToolOutput::Immediate(serde_json::Value::String("Scratchpad updated (appended).".to_string())))
                } else {
                    Ok(ToolOutput::Immediate(serde_json::Value::String("Error: 'text' is required for append action.".to_string())))
                }
            },
            "clear" => {
                scratchpad.clear();
                Ok(ToolOutput::Immediate(serde_json::Value::String("Scratchpad cleared.".to_string())))
            },
            _ => {
                Ok(ToolOutput::Immediate(serde_json::Value::String(format!("Unknown action: {}", args.action))))
            }
        }
    }
    
    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}
