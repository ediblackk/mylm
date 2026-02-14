//! Global state persistence tool for cross-session data storage.
//!
//! Provides key-value storage for maintaining state between conversations
//! and sessions. Supports get, set, delete, and list operations on JSON values.
//!
//! # Main Types
//! - `StateTool`: Tool implementation for state operations
//! - `StateCommand`: Enum of available state commands

use anyhow::{Context, Result};
use async_trait::async_trait;
use crate::agent_old::tool::{Tool, ToolKind, ToolOutput};
use crate::state::StateStore;
use std::error::Error as StdError;
use std::sync::{Arc, RwLock};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "command", content = "args")]
pub enum StateCommand {
    #[serde(rename = "state_get")]
    Get { key: String },
    #[serde(rename = "state_set")]
    Set { key: String, value: serde_json::Value },
    #[serde(rename = "state_delete")]
    Delete { key: String },
    #[serde(rename = "state_list")]
    List,
}

pub struct StateTool {
    store: Arc<RwLock<StateStore>>,
}

impl StateTool {
    pub fn new(store: Arc<RwLock<StateStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for StateTool {
    fn name(&self) -> &str {
        "global_state"
    }

    fn description(&self) -> &str {
        "Persistence tool for storing and retrieving global state values (JSON) between conversations and sessions."
    }

    fn usage(&self) -> &str {
        r#"JSON object with:
- "command": "state_get" | "state_set" | "state_delete" | "state_list"
- "args": { "key": "string", "value": any } (depending on command)

Examples:
{"command": "state_set", "args": {"key": "user_preferences", "value": {"theme": "dark"}}}
{"command": "state_get", "args": {"key": "user_preferences"}}
{"command": "state_list", "args": {}}"#
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "enum": ["state_get", "state_set", "state_delete", "state_list"]
                },
                "args": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string" },
                        "value": { "type": "object" }
                    }
                }
            },
            "required": ["command"]
        })
    }

    async fn call(&self, args: &str) -> Result<ToolOutput, Box<dyn StdError + Send + Sync>> {
        let cmd: StateCommand = serde_json::from_str(args)
            .context("Failed to parse state command. Ensure it's a valid JSON matching the schema.")
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync + 'static> { e.into() })?;

        match cmd {
            StateCommand::Get { key } => {
                let store = self.store.read().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
                match store.get(&key) {
                    Some(val) => Ok(ToolOutput::Immediate(serde_json::to_value(&val)?)),
                    None => Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                        "Key '{}' not found",
                        key
                    )))),
                }
            }
            StateCommand::Set { key, value } => {
                let mut store = self.store.write().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
                store.set(key.clone(), value)?;
                Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                    "Successfully set key '{}'",
                    key
                ))))
            }
            StateCommand::Delete { key } => {
                let mut store = self.store.write().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
                store.delete(&key)?;
                Ok(ToolOutput::Immediate(serde_json::Value::String(format!(
                    "Successfully deleted key '{}'",
                    key
                ))))
            }
            StateCommand::List => {
                let store = self.store.read().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
                let keys = store.list();
                Ok(ToolOutput::Immediate(serde_json::to_value(keys)?))
            }
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Internal
    }
}
