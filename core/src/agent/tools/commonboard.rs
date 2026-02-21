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
use crate::agent::runtime::orchestrator::commonbox::Commonbox;
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

    /// Get tool description for LLM prompt
    pub fn description() -> &'static str {
        r#"# commonboard - Inter-agent Coordination

Use this tool to coordinate with other agents working in parallel.

## Actions

### claim
Claim exclusive access to a file or resource.
**YOU MUST CLAIM before modifying any file that might be shared.**

Usage: {"action": "claim", "resource": "src/main.rs"}
Response: {"status": "claimed"} or {"status": "failed", "error": "Resource already claimed"}

### release
Release a claim when done. **Always release after completing work.**

Usage: {"action": "release", "resource": "src/main.rs"}
Response: {"status": "released"} or {"status": "failed", "error": "Not claimed by you"}

### check
Check if a resource is claimed without claiming it.

Usage: {"action": "check", "resource": "src/main.rs"}
Response: {"claimed": true, "claimed_by": "WORKER-1"} or {"claimed": false}

### list_claims
View all active claims.

Usage: {"action": "list_claims"}
Response: {"claims": [{"resource": "src/main.rs", "claimed_by": "WORKER-1"}]}

### progress
Report progress to other agents.

Usage: {"action": "progress", "message": "Refactoring module X, 50% done"}

### complete
Mark task complete with summary.

Usage: {"action": "complete", "summary": "Refactored module X, all tests pass"}

## Coordination Protocol

1. **Before modifying**: `claim` the file
2. **During work**: Report `progress` if long-running
3. **After done**: `release` the file
4. **On completion**: Mark `complete`

**WARNING**: Modifying files without claiming may cause conflicts!"#
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

            "release" => {
                let resource = call.arguments.get("resource")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::new("Missing 'resource' for release action"))?;

                let agent_short = agent_id.short_name();
                match self.commonbox.release_resource(&agent_id, resource).await {
                    Ok(()) => Ok(ToolResult::Success {
                        output: format!("Released resource: {}", resource),
                        structured: Some(serde_json::json!({
                            "status": "released",
                            "resource": resource,
                            "by": agent_short,
                        })),
                    }),
                    Err(e) => Ok(ToolResult::Success {
                        output: format!("Failed to release: {}", e),
                        structured: Some(serde_json::json!({
                            "status": "failed",
                            "error": e.to_string(),
                        })),
                    }),
                }
            }

            "list_claims" => {
                let claims = self.commonbox.list_claims().await;
                let claims_list: Vec<_> = claims.iter().map(|(res, id)| {
                    serde_json::json!({
                        "resource": res,
                        "claimed_by": id.short_name(),
                    })
                }).collect();

                let output = if claims.is_empty() {
                    "No active resource claims.".to_string()
                } else {
                    claims.iter()
                        .map(|(res, id)| format!("{}: {}", res, id.short_name()))
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                Ok(ToolResult::Success {
                    output,
                    structured: Some(serde_json::json!({
                        "status": "ok",
                        "claims": claims_list,
                        "count": claims.len(),
                    })),
                })
            }

            _ => Err(ToolError::new(format!("Unknown action: {}", action))),
        }
    }
}
