use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;

/// The output of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum ToolOutput {
    /// The tool completed immediately with a result.
    Immediate(serde_json::Value),
    /// The tool started a background job.
    Background {
        job_id: String,
        description: String,
    },
}

impl ToolOutput {
    pub fn as_string(&self) -> String {
        match self {
            Self::Immediate(v) => {
                if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                }
            }
            Self::Background { job_id, description } => {
                format!("Started background job {}: {}", job_id, description)
            }
        }
    }
}

/// Categorizes tools based on how they should be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// Execute silently/internally (e.g., Memory, WebSearch).
    Internal,
    /// Execute visibly in Terminal (e.g., Shell).
    Terminal,
    /// Web-based tools (search, crawl)
    Web,
}

/// A trait for tools that can be executed by the agent.
///
/// Tools are the primary way the agent interacts with the world.
/// Each tool must implement this trait and be `Send + Sync` to be used in the agentic loop.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The name of the tool (e.g. "execute_command")
    fn name(&self) -> &str;

    /// A brief description of what the tool does
    fn description(&self) -> &str;

    /// A description of how to use the tool, including parameter format
    fn usage(&self) -> &str;

    /// Optional JSON schema for tool parameters
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "args": {
                    "type": "string",
                    "description": self.usage()
                }
            },
            "required": ["args"]
        })
    }

    /// Execute the tool with the provided arguments
    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>>;

    /// The kind of tool (Internal or Terminal)
    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}
