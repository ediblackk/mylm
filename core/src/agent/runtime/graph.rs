//! Capability graph
//!
//! Strongly typed capability composition. No dynamic lookup.

use std::sync::Arc;
use crate::agent::runtime::capability::{LLMCapability, ToolCapability, ApprovalCapability, WorkerCapability, TelemetryCapability, Capability, WorkerHandle};
use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::{LLMError, ToolError, ApprovalError, WorkerError};
use crate::agent::types::intents::{LLMRequest, ToolCall, ApprovalRequest, WorkerSpec};
use crate::agent::types::events::{LLMResponse, ToolResult, ApprovalOutcome, WorkerId};
use crate::agent::cognition::{AgentDecision, InputEvent};

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
        Self {
            llm: Arc::new(StubLLM),
            tools: Arc::new(StubTools),
            approval: Arc::new(StubApproval),
            workers: Arc::new(StubWorkers),
            telemetry: Arc::new(StubTelemetry),
        }
    }
}

// Stub implementations for testing

pub struct StubLLM;
impl Capability for StubLLM {
    fn name(&self) -> &'static str { "stub-llm" }
}
#[async_trait::async_trait]
impl LLMCapability for StubLLM {
    async fn complete(&self, _ctx: &RuntimeContext, _req: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse { 
            content: "stub".to_string(), 
            usage: crate::agent::types::events::TokenUsage::default(),
            model: "stub".to_string(),
            provider: "stub".to_string(),
            finish_reason: crate::agent::types::events::FinishReason::Stop,
            structured: None,
        })
    }
    
    fn complete_stream<'a>(
        &'a self,
        _ctx: &'a RuntimeContext,
        _req: LLMRequest,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = Result<crate::agent::runtime::capability::StreamChunk, LLMError>> + Send + 'a>> {
        Box::pin(futures::stream::once(async {
            Ok(crate::agent::runtime::capability::StreamChunk {
                content: "stub".to_string(),
                is_final: true,
            })
        }))
    }
}

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

pub struct StubWorkers;
impl Capability for StubWorkers {
    fn name(&self) -> &'static str { "stub-workers" }
}
#[async_trait::async_trait]
impl WorkerCapability for StubWorkers {
    async fn spawn(&self, _ctx: &RuntimeContext, _spec: WorkerSpec) -> Result<WorkerHandle, WorkerError> {
        // WorkerId is now u64-based, not string-based
        Ok(WorkerHandle { id: WorkerId(0) })
    }
}

pub struct StubTelemetry;
impl Capability for StubTelemetry {
    fn name(&self) -> &'static str { "stub-telemetry" }
}
#[async_trait::async_trait]
impl TelemetryCapability for StubTelemetry {
    async fn record_decision(&self, _ctx: &RuntimeContext, _decision: &AgentDecision) {}
    async fn record_result(&self, _ctx: &RuntimeContext, _event: &InputEvent) {}
}
