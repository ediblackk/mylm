//! Local capability stubs
//!
//! Placeholder implementations for local execution.

use crate::agent::runtime::core::{
    Capability, LLMCapability, ToolCapability, ApprovalCapability, WorkerCapability, 
    TelemetryCapability, WorkerSpawnHandle, RuntimeContext, LLMError, ToolError, 
    ApprovalError, WorkerError, StreamChunk,
};
use crate::agent::types::intents::{LLMRequest, ToolCall, ApprovalRequest, WorkerSpec};
use crate::agent::types::events::{LLMResponse, ToolResult, ApprovalOutcome, WorkerId};
use crate::agent::cognition::{AgentDecision, InputEvent};
use futures::Stream;

/// Local LLM stub
pub struct LocalLLMStub;

impl LocalLLMStub {
    pub fn new() -> Self {
        Self
    }
}

impl Capability for LocalLLMStub {
    fn name(&self) -> &'static str {
        "local_llm_stub"
    }
}

#[async_trait::async_trait]
impl LLMCapability for LocalLLMStub {
    async fn complete(
        &self,
        _ctx: &RuntimeContext,
        _req: LLMRequest,
    ) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse {
            content: "Local LLM stub response".to_string(),
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
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<StreamChunk, LLMError>> + Send + 'a>> {
        Box::pin(futures::stream::once(async {
            Ok(StreamChunk {
                content: "Local LLM stub response".to_string(),
                is_final: true,
                usage: None,
            })
        }))
    }
}

/// Simple tool executor for testing
pub struct SimpleToolExecutor;

impl SimpleToolExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SimpleToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl Capability for SimpleToolExecutor {
    fn name(&self) -> &'static str {
        "simple-tools"
    }
}

#[async_trait::async_trait]
impl ToolCapability for SimpleToolExecutor {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        match call.name.as_str() {
            "shell" => Ok(ToolResult::Success {
                output: format!("Would execute: {}", call.arguments),
                structured: None,
            }),
            "read_file" | "cat" => Ok(ToolResult::Success {
                output: "Stub file content".to_string(),
                structured: None,
            }),
            "list_files" | "ls" => Ok(ToolResult::Success {
                output: "file1.txt\nfile2.txt".to_string(),
                structured: None,
            }),
            _ => Ok(ToolResult::Error {
                message: format!("Unknown tool: {}", call.name),
                code: Some("UNKNOWN_TOOL".to_string()),
                retryable: false,
            }),
        }
    }
}

/// Local approval stub (auto-approves)
pub struct LocalApprovalStub {
    auto_approve: bool,
}

impl LocalApprovalStub {
    pub fn new(auto_approve: bool) -> Self {
        Self { auto_approve }
    }
}

impl Capability for LocalApprovalStub {
    fn name(&self) -> &'static str {
        "local_approval_stub"
    }
}

#[async_trait::async_trait]
impl ApprovalCapability for LocalApprovalStub {
    async fn request(
        &self,
        _ctx: &RuntimeContext,
        _req: ApprovalRequest,
    ) -> Result<ApprovalOutcome, ApprovalError> {
        if self.auto_approve {
            Ok(ApprovalOutcome::Granted)
        } else {
            Ok(ApprovalOutcome::Denied { reason: None })
        }
    }
}

/// Local worker stub
pub struct LocalWorkerStub;

impl LocalWorkerStub {
    pub fn new() -> Self {
        Self
    }
}

impl Capability for LocalWorkerStub {
    fn name(&self) -> &'static str {
        "local_worker_stub"
    }
}

#[async_trait::async_trait]
impl WorkerCapability for LocalWorkerStub {
    async fn spawn(
        &self,
        _ctx: &RuntimeContext,
        _spec: WorkerSpec,
    ) -> Result<WorkerSpawnHandle, WorkerError> {
        Ok(WorkerSpawnHandle {
            id: WorkerId(1),
        })
    }
}

/// Local telemetry stub (no-op)
pub struct LocalTelemetryStub;

impl LocalTelemetryStub {
    pub fn new() -> Self {
        Self
    }
}

impl Capability for LocalTelemetryStub {
    fn name(&self) -> &'static str {
        "local_telemetry_stub"
    }
}

#[async_trait::async_trait]
impl TelemetryCapability for LocalTelemetryStub {
    async fn record_decision(&self, _ctx: &RuntimeContext, _decision: &AgentDecision) {}
    async fn record_result(&self, _ctx: &RuntimeContext, _event: &InputEvent) {}
}
