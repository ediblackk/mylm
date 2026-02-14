//! Capability traits
//!
//! Base trait + specialized async capability traits.
//! No decision logic. Pure side-effect execution.

use crate::agent::runtime::context::RuntimeContext;
use crate::agent::runtime::error::{LLMError, ToolError, ApprovalError, WorkerError};
use crate::agent::types::intents::{LLMRequest, ToolCall, ApprovalRequest, WorkerSpec};
use crate::agent::types::events::{LLMResponse, ToolResult, ApprovalOutcome};
use crate::agent::types::events::WorkerId;
use crate::agent::cognition::{AgentDecision, InputEvent};
use std::pin::Pin;
use futures::Stream;

/// Base capability trait for identity
pub trait Capability: Send + Sync {
    fn name(&self) -> &'static str;
}

/// Stream chunk for LLM streaming
#[derive(Debug, Clone)]
pub struct StreamChunk {
    pub content: String,
    pub is_final: bool,
}

/// LLM capability - text completion
#[async_trait::async_trait]
pub trait LLMCapability: Capability {
    async fn complete(
        &self,
        ctx: &RuntimeContext,
        req: LLMRequest,
    ) -> Result<LLMResponse, LLMError>;
    
    /// Stream completion - returns a stream of chunks
    fn complete_stream<'a>(
        &'a self,
        ctx: &'a RuntimeContext,
        req: LLMRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk, LLMError>> + Send + 'a>>;
}

/// Tool capability - tool execution
#[async_trait::async_trait]
pub trait ToolCapability: Capability {
    async fn execute(
        &self,
        ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError>;
}

/// Approval capability - user approval requests
#[async_trait::async_trait]
pub trait ApprovalCapability: Capability {
    async fn request(
        &self,
        ctx: &RuntimeContext,
        req: ApprovalRequest,
    ) -> Result<ApprovalOutcome, ApprovalError>;
}

/// Worker capability - spawn background workers
#[async_trait::async_trait]
pub trait WorkerCapability: Capability {
    async fn spawn(
        &self,
        ctx: &RuntimeContext,
        spec: WorkerSpec,
    ) -> Result<WorkerHandle, WorkerError>;
}

/// Telemetry capability - observability
#[async_trait::async_trait]
pub trait TelemetryCapability: Capability {
    async fn record_decision(&self, ctx: &RuntimeContext, decision: &AgentDecision);
    async fn record_result(&self, ctx: &RuntimeContext, event: &InputEvent);
}

/// Worker handle returned by spawn
#[derive(Debug, Clone)]
pub struct WorkerHandle {
    pub id: WorkerId,
}
