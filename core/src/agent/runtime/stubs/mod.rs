//! Test Stubs
//!
//! Stub implementations of capabilities for testing.

use std::sync::Arc;
use crate::agent::runtime::core::{
    Capability, LLMCapability, ToolCapability, ApprovalCapability, 
    WorkerCapability, TelemetryCapability, WorkerSpawnHandle,
    RuntimeContext, LLMError, ToolError, ApprovalError, WorkerError,
};
use crate::agent::types::intents::{LLMRequest, ToolCall, ApprovalRequest, WorkerSpec};
use crate::agent::types::events::{LLMResponse, ToolResult, ApprovalOutcome, WorkerId, TokenUsage, FinishReason};
use crate::agent::cognition::{AgentDecision, InputEvent};

/// Stub LLM capability
pub struct StubLLM;

impl Capability for StubLLM {
    fn name(&self) -> &'static str { "stub-llm" }
}

#[async_trait::async_trait]
impl LLMCapability for StubLLM {
    async fn complete(&self, _ctx: &RuntimeContext, _req: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse { 
            content: "stub".to_string(), 
            usage: TokenUsage::default(),
            model: "stub".to_string(),
            provider: "stub".to_string(),
            finish_reason: FinishReason::Stop,
            structured: None,
        })
    }
    
    fn complete_stream<'a>(
        &'a self,
        _ctx: &'a RuntimeContext,
        _req: LLMRequest,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = Result<crate::agent::runtime::core::StreamChunk, LLMError>> + Send + 'a>> {
        Box::pin(futures::stream::once(async {
            Ok(crate::agent::runtime::core::StreamChunk {
                content: "stub".to_string(),
                is_final: true,
                usage: None,
            })
        }))
    }
}

/// Stub tools capability
pub struct StubTools;

impl Capability for StubTools {
    fn name(&self) -> &'static str { "stub-tools" }
}

#[async_trait::async_trait]
impl ToolCapability for StubTools {
    async fn execute(&self, _ctx: &RuntimeContext, _call: ToolCall) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::Success { output: "stub".to_string(), structured: None })
    }
}

/// Stub approval capability
pub struct StubApproval;

impl Capability for StubApproval {
    fn name(&self) -> &'static str { "stub-approval" }
}

#[async_trait::async_trait]
impl ApprovalCapability for StubApproval {
    async fn request(&self, _ctx: &RuntimeContext, _req: ApprovalRequest) -> Result<ApprovalOutcome, ApprovalError> {
        Ok(ApprovalOutcome::Granted)
    }
}

/// Stub workers capability
pub struct StubWorkers;

impl Capability for StubWorkers {
    fn name(&self) -> &'static str { "stub-workers" }
}

#[async_trait::async_trait]
impl WorkerCapability for StubWorkers {
    async fn spawn(&self, _ctx: &RuntimeContext, _spec: WorkerSpec) -> Result<WorkerSpawnHandle, WorkerError> {
        Ok(WorkerSpawnHandle { id: WorkerId(0) })
    }
}

/// Stub telemetry capability
pub struct StubTelemetry;

impl Capability for StubTelemetry {
    fn name(&self) -> &'static str { "stub-telemetry" }
}

#[async_trait::async_trait]
impl TelemetryCapability for StubTelemetry {
    async fn record_decision(&self, _ctx: &RuntimeContext, _decision: &AgentDecision) {}
    async fn record_result(&self, _ctx: &RuntimeContext, _event: &InputEvent) {}
}

/// Create a stub capability graph for testing
pub fn stub_capability_graph() -> crate::agent::runtime::executor::CapabilityGraph {
    crate::agent::runtime::executor::CapabilityGraph::new(
        Arc::new(StubLLM),
        Arc::new(StubTools),
        Arc::new(StubApproval),
        Arc::new(StubWorkers),
        Arc::new(StubTelemetry),
    )
}
