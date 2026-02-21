//! Commonboard Tool - Inter-agent coordination via Commonbox
//!
//! Provides coordination primitives for workers:
//! - **claim**: Claim a file/resource before working on it
//! - **progress**: Report progress updates
//! - **complete**: Mark task as complete with summary
//! - **query**: Check coordination board status

use crate::agent::runtime::core::{Capability, ToolCapability, RuntimeContext, ToolError};
use crate::agent::types::intents::ToolCall;
use crate::agent::types::events::ToolResult;
use crate::agent::commonbox::Commonbox;
use crate::agent::identity::AgentId;
use std::sync::Arc;

/// Commonboard tool for inter-agent coordination
#[derive(Clone)]
pub struct CommonboardTool {
    commonbox: Arc<Commonbox>,
}

impl CommonboardTool {
    /// Create a new commonboard tool
    pub fn new(commonbox: Arc<Commonbox>) -> Self {
        Self { commonbox }
    }
}

impl Capability for CommonboardTool {
    fn name(&self) -> &'static str {
        "commonboard"
    }
}

#[async_trait::async_trait]
impl ToolCapability for CommonboardTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        let action = call.arguments.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("query");

        // Get agent_id from context or use anonymous
        let agent_id = call.arguments.get("agent_id")
            .and_then(|v| v.as_str())
            .map(|s| AgentId::worker(s.to_string()))
            .unwrap_or_else(|| AgentId::worker("unknown".to_string()));

        match action {
            "claim" => {
                let resource = call.arguments.get("resource")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Missing 'resource' for claim action"))?;

                let agent_short = agent_id.short_name();
                match self.commonbox.claim_resource(agent_id, resource).await {
                    Ok(()) => Ok(ToolResult::Success {
                        output: format!("Claimed resource: {}", resource),
                        structured: Some(serde_json::json!({
                            "status": "claimed",
                            "resource": resource,
                            "by": agent_short,
                        })),
                    }),
                    Err(e) => Ok(ToolResult::Success {
                        output: format!("Failed to claim: {}", e),
                        structured: Some(serde_json::json!({
                            "status": "failed",
                            "error": e.to_string(),
                        })),
                    }),
                }
            }

            "progress" => {
                let message = call.arguments.get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Missing 'message' for progress action"))?;

                self.commonbox.report_progress(agent_id.clone(), message).await;

                Ok(ToolResult::Success {
                    output: format!("Progress reported: {}", message),
                    structured: Some(serde_json::json!({
                        "status": "reported",
                        "message": message,
                    })),
                })
            }

            "complete" => {
                let summary = call.arguments.get("summary")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Missing 'summary' for complete action"))?;

                self.commonbox.mark_complete(agent_id.clone(), summary).await;

                Ok(ToolResult::Success {
                    output: format!("Task completed: {}", summary),
                    structured: Some(serde_json::json!({
                        "status": "completed",
                        "summary": summary,
                    })),
                })
            }

            "query" | "list" => {
                let snapshot = self.commonbox.get_coordination_snapshot().await;
                let entries = self.commonbox.list_coordination().await;

                Ok(ToolResult::Success {
                    output: snapshot.clone(),
                    structured: Some(serde_json::json!({
                        "status": "ok",
                        "snapshot": snapshot,
                        "count": entries.len(),
                    })),
                })
            }

            "check" => {
                let resource = call.arguments.get("resource")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Missing 'resource' for check action"))?;

                let claimed_by = self.commonbox.is_resource_claimed(resource).await;

                Ok(ToolResult::Success {
                    output: if let Some(ref id) = claimed_by {
                        format!("Resource '{}' is claimed by {}", resource, id.short_name())
                    } else {
                        format!("Resource '{}' is available", resource)
                    },
                    structured: Some(serde_json::json!({
                        "resource": resource,
                        "claimed": claimed_by.is_some(),
                        "claimed_by": claimed_by.map(|id| id.short_name()),
                    })),
                })
            }

            _ => Err(ToolError::new(format!("Unknown action: {}", action))),
        }
    }
}
