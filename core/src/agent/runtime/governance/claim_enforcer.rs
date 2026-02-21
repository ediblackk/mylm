//! Claim Enforcement - Resource claim verification
//!
//! Enforces that agents must claim resources before modifying them.
//! Integrated into the runtime tool execution pipeline.

use crate::agent::identity::AgentId;
use crate::agent::runtime::orchestrator::commonbox::Commonbox;
use crate::agent::types::intents::ToolCall;
use std::sync::Arc;

/// Result of claim enforcement check
#[derive(Debug, Clone)]
pub enum ClaimEnforcement {
    /// Tool call is allowed
    Allow,
    /// Tool call is denied - resource claimed by another agent
    Deny { resource: String, claimed_by: String },
    /// Tool call requires claim first
    RequiresClaim { resource: String },
}

/// Enforces resource claim requirements
pub struct ClaimEnforcer {
    commonbox: Arc<Commonbox>,
}

impl ClaimEnforcer {
    /// Create new claim enforcer
    pub fn new(commonbox: Arc<Commonbox>) -> Self {
        Self { commonbox }
    }

    /// Check if a tool call requires a claim
    pub async fn check_tool_call(
        &self,
        agent_id: &AgentId,
        call: &ToolCall,
    ) -> ClaimEnforcement {
        // Extract resource paths from tool arguments
        let resources = self.extract_resources(call);

        for resource in resources {
            if let Some(claimer) = self.commonbox.is_resource_claimed(&resource).await {
                if &claimer != agent_id {
                    // Claimed by someone else
                    return ClaimEnforcement::Deny {
                        resource,
                        claimed_by: claimer.short_name(),
                    };
                }
                // Claimed by this agent - OK
            } else {
                // Not claimed - check if this tool type requires claims
                if self.requires_claim(call) {
                    return ClaimEnforcement::RequiresClaim { resource };
                }
            }
        }

        ClaimEnforcement::Allow
    }

    /// Extract file/resource paths from tool call
    fn extract_resources(&self, call: &ToolCall) -> Vec<String> {
        let mut resources = Vec::new();

        match call.name.as_str() {
            "write_file" | "read_file" | "append_file" | "edit_file" => {
                if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                    resources.push(path.to_string());
                }
            }
            "shell" | "bash" => {
                // Extract file paths from shell command (simplified)
                if let Some(cmd) = call.arguments.get("command").and_then(|v| v.as_str()) {
                    resources.extend(extract_paths_from_shell(cmd));
                }
            }
            "git" => {
                // Git operations on specific paths
                if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                    resources.push(path.to_string());
                }
            }
            _ => {}
        }

        resources
    }

    /// Check if this tool type requires a claim
    fn requires_claim(&self, call: &ToolCall) -> bool {
        match call.name.as_str() {
            // Write operations require claims
            "write_file" | "append_file" | "edit_file" | "shell" | "bash" => true,
            // Read operations don't require claims
            "read_file" | "list_files" | "git" => false,
            _ => false,
        }
    }
}

/// Simple path extraction from shell commands
fn extract_paths_from_shell(cmd: &str) -> Vec<String> {
    // Very basic extraction - looks for common patterns
    // In production, use proper shell parsing
    let mut paths = Vec::new();

    // Pattern: command > path or command >> path
    for part in cmd.split_whitespace() {
        if part.starts_with('/') || part.starts_with("./") || part.starts_with("../") {
            paths.push(part.to_string());
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::orchestrator::commonbox::Commonbox;
    use serde_json::json;

    #[tokio::test]
    async fn test_write_file_requires_claim() {
        let commonbox = Arc::new(Commonbox::new());
        let enforcer = ClaimEnforcer::new(commonbox);

        let call = ToolCall {
            name: "write_file".to_string(),
            arguments: json!({"path": "src/main.rs", "content": "fn main() {}"}),
            tool_use_id: Some("test-1".to_string()),
        };

        let agent = AgentId::worker("test");
        let result = enforcer.check_tool_call(&agent, &call).await;

        assert!(
            matches!(result, ClaimEnforcement::RequiresClaim { resource } if resource == "src/main.rs"),
            "Expected RequiresClaim, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_claimed_by_other_denied() {
        let commonbox = Arc::new(Commonbox::new());
        let enforcer = ClaimEnforcer::new(commonbox.clone());

        let other_agent = AgentId::worker("other");
        commonbox
            .claim_resource(other_agent.clone(), "src/main.rs")
            .await
            .unwrap();

        let call = ToolCall {
            name: "write_file".to_string(),
            arguments: json!({"path": "src/main.rs", "content": "fn main() {}"}),
            tool_use_id: Some("test-2".to_string()),
        };

        let my_agent = AgentId::worker("me");
        let result = enforcer.check_tool_call(&my_agent, &call).await;

        assert!(
            matches!(result, ClaimEnforcement::Deny { resource, claimed_by } 
                if resource == "src/main.rs" && claimed_by == "WORKER-other"),
            "Expected Deny, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_read_file_allowed_without_claim() {
        let commonbox = Arc::new(Commonbox::new());
        let enforcer = ClaimEnforcer::new(commonbox);

        let call = ToolCall {
            name: "read_file".to_string(),
            arguments: json!({"path": "src/main.rs"}),
            tool_use_id: Some("test-3".to_string()),
        };

        let agent = AgentId::worker("test");
        let result = enforcer.check_tool_call(&agent, &call).await;

        assert!(
            matches!(result, ClaimEnforcement::Allow),
            "Expected Allow, got {:?}",
            result
        );
    }
}
