//! Capability graph
//!
//! Strongly typed capability composition. No dynamic lookup.

use std::sync::Arc;
use crate::agent::runtime::core::{
    LLMCapability, ToolCapability, ApprovalCapability, WorkerCapability, TelemetryCapability,
};

/// Capability graph - all capabilities wired together
#[derive(Clone)]
pub struct CapabilityGraph {
    pub llm: Arc<dyn LLMCapability>,
    pub tools: Arc<dyn ToolCapability>,
    pub approval: Arc<dyn ApprovalCapability>,
    pub workers: Arc<dyn WorkerCapability>,
    pub telemetry: Arc<dyn TelemetryCapability>,
}

impl CapabilityGraph {
    /// Create a new capability graph
    pub fn new(
        llm: Arc<dyn LLMCapability>,
        tools: Arc<dyn ToolCapability>,
        approval: Arc<dyn ApprovalCapability>,
        workers: Arc<dyn WorkerCapability>,
        telemetry: Arc<dyn TelemetryCapability>,
    ) -> Self {
        Self {
            llm,
            tools,
            approval,
            workers,
            telemetry,
        }
    }
    
    /// Create a stub capability graph for testing
    pub fn stub() -> Self {
        use crate::agent::runtime::stubs::{StubLLM, StubTools, StubApproval, StubWorkers, StubTelemetry};
        Self {
            llm: Arc::new(StubLLM),
            tools: Arc::new(StubTools),
            approval: Arc::new(StubApproval),
            workers: Arc::new(StubWorkers),
            telemetry: Arc::new(StubTelemetry),
        }
    }
}
