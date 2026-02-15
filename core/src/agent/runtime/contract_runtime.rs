//! Contract Runtime Implementation
//!
//! Bridges the new contract's AgencyRuntime trait to existing runtime capabilities.

use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::broadcast;
use std::time::Instant;
use futures::StreamExt;

use crate::agent::contract::{
    AgencyRuntime,
    Intent,
    IntentGraph,
    ids::IntentId,
    observations::{Observation, ExecutionError},
    runtime::{RuntimeError, TelemetryEvent, HealthStatus},
    session::OutputEvent,
};

use crate::agent::runtime::impls::{
    DagExecutor,
    LlmClientCapability,
    LocalWorkerCapability,
    ConsoleTelemetry,
    AutoApproveCapability,
};
use crate::agent::runtime::tools::ToolRegistry;
use crate::agent::runtime::capability::{WorkerCapability, LLMCapability, ToolCapability, ApprovalCapability};
use crate::agent::runtime::terminal::{TerminalExecutor, DefaultTerminalExecutor};

use crate::agent::runtime::context::RuntimeContext;
use crate::llm::LlmClient;

/// Full runtime implementation that fulfills the AgencyRuntime contract
///
/// This wires together all the existing capability implementations
/// to fulfill the new contract's AgencyRuntime trait.
pub struct ContractRuntime {
    /// Tool execution
    tools: Arc<ToolRegistry>,
    /// LLM completion
    llm: Arc<LlmClientCapability>,
    /// Worker spawning
    workers: Arc<LocalWorkerCapability>,
    /// Approval handling
    approval: Arc<dyn ApprovalCapability>,
    /// Telemetry
    telemetry: Arc<ConsoleTelemetry>,
    /// Telemetry sender
    telemetry_tx: broadcast::Sender<TelemetryEvent>,
    /// Output sender for streaming events (optional)
    output_tx: Option<broadcast::Sender<OutputEvent>>,
    /// Terminal executor for shell commands
    terminal: Arc<dyn TerminalExecutor>,
}

impl ContractRuntime {
    /// Create a new runtime with the given LLM client
    /// 
    /// Uses a default terminal executor (std::process::Command).
    /// Uses auto-approve for approval (suitable for testing/non-interactive use).
    /// Use `with_terminal()` and `with_approval()` for custom configuration.
    pub fn new(llm_client: Arc<LlmClient>) -> Self {
        let tools = Arc::new(ToolRegistry::new());
        let llm = Arc::new(LlmClientCapability::new(llm_client));
        let workers = Arc::new(LocalWorkerCapability::new());
        let approval: Arc<dyn ApprovalCapability> = Arc::new(AutoApproveCapability::new());
        let telemetry = Arc::new(ConsoleTelemetry::new());
        let (telemetry_tx, _) = broadcast::channel(100);

        Self {
            tools,
            llm,
            workers,
            approval,
            telemetry,
            telemetry_tx,
            output_tx: None,
            terminal: Arc::new(DefaultTerminalExecutor::new()),
        }
    }
    
    /// Create a new runtime with custom tool registry
    /// 
    /// Uses a default terminal executor (std::process::Command).
    /// Uses auto-approve for approval.
    pub fn with_tools(llm_client: Arc<LlmClient>, tools: Arc<ToolRegistry>) -> Self {
        let llm = Arc::new(LlmClientCapability::new(llm_client));
        let workers = Arc::new(LocalWorkerCapability::new());
        let approval: Arc<dyn ApprovalCapability> = Arc::new(AutoApproveCapability::new());
        let telemetry = Arc::new(ConsoleTelemetry::new());
        let (telemetry_tx, _) = broadcast::channel(100);

        Self {
            tools,
            llm,
            workers,
            approval,
            telemetry,
            telemetry_tx,
            output_tx: None,
            terminal: Arc::new(DefaultTerminalExecutor::new()),
        }
    }
    
    /// Set a custom approval capability
    /// 
    /// This allows the TUI to provide an interactive approval capability
    /// that shows approval dialogs and waits for user confirmation.
    pub fn with_approval(mut self, approval: Arc<dyn ApprovalCapability>) -> Self {
        self.approval = approval;
        self
    }
    
    /// Set a custom terminal executor
    /// 
    /// This allows the TUI to provide a PTY-based terminal executor
    /// so agent commands run in the shared terminal session.
    pub fn with_terminal(mut self, terminal: Arc<dyn TerminalExecutor>) -> Self {
        self.terminal = terminal;
        self
    }
    
    /// Set output sender for streaming events
    pub fn with_output_sender(mut self, output_tx: broadcast::Sender<OutputEvent>) -> Self {
        self.output_tx = Some(output_tx);
        self
    }

    /// Get a reference to the tool registry
    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    /// Execute a single intent with the given intent_id
    async fn execute_intent(
        &self,
        intent_id: IntentId,
        intent: Intent,
    ) -> Result<Observation, RuntimeError> {
        let start_time = Instant::now();
        
        // Emit telemetry: intent started
        let _ = self.telemetry_tx.send(TelemetryEvent::IntentStarted {
            intent_id,
            intent_type: intent_type_name(&intent).to_string(),
            timestamp: std::time::SystemTime::now(),
        });

        let result = match intent {
            Intent::CallTool(call) => {
                // Create runtime context for tool execution
                // Include terminal executor so shell tool can use PTY
                let ctx = RuntimeContext::new()
                    .with_terminal(Arc::clone(&self.terminal));
                
                // Execute tool via registry
                let tool_start = Instant::now();
                let result = self.tools.execute(&ctx, call.clone()).await
                    .map_err(|e| RuntimeError::ToolExecutionFailed {
                        tool: call.name.clone(),
                        error: e.to_string(),
                    })?;
                
                let execution_time_ms = tool_start.elapsed().as_millis() as u64;
                
                // Emit telemetry: tool executed
                let success = matches!(result, crate::agent::types::events::ToolResult::Success { .. });
                let _ = self.telemetry_tx.send(TelemetryEvent::ToolExecuted {
                    tool: call.name.clone(),
                    duration_ms: execution_time_ms,
                    success,
                });
                
                Ok(Observation::ToolCompleted {
                    intent_id,
                    result,
                    execution_time_ms,
                })
            }
            Intent::RequestLLM(req) => {
                // Create runtime context for LLM execution
                let ctx = RuntimeContext::new()
                    .with_terminal(Arc::clone(&self.terminal));
                
                // Use streaming if output sender is available
                if let Some(ref output_tx) = self.output_tx {
                    // Clone request for potential fallback
                    let req_clone = req.clone();
                    let mut stream = self.llm.complete_stream(&ctx, req);
                    let mut full_content = String::new();
                    let mut stream_failed = false;
                    let mut stream_error = String::new();
                    
                    crate::info_log!("[RUNTIME] Starting LLM stream...");
                    let mut chunk_count = 0;
                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                // Skip logging empty keep-alive chunks
                                if !chunk.content.is_empty() {
                                    // crate::debug_log!("[RUNTIME] Got chunk: len={}, is_final={}", chunk.content.len(), chunk.is_final);
                                }
                                if chunk.is_final {
                                    break;
                                }
                                if !chunk.content.is_empty() {
                                    chunk_count += 1;
                                    full_content.push_str(&chunk.content);
                                    // Emit streaming chunk
                                    if let Err(e) = output_tx.send(OutputEvent::ResponseChunk {
                                        content: chunk.content.clone(),
                                    }) {
                                        crate::error_log!("[RUNTIME] Failed to send chunk: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                crate::error_log!("[RUNTIME] Stream error: {}", e.message);
                                stream_failed = true;
                                stream_error = e.message;
                                break;
                            }
                        }
                    }
                    
                    // If streaming failed (e.g., 405 Method Not Allowed), fallback to non-streaming
                    if stream_failed {
                        crate::warn_log!("[RUNTIME] Streaming failed ({}), falling back to non-streaming", stream_error);
                        
                        // Notify user that we're falling back
                        let _ = output_tx.send(OutputEvent::Status {
                            message: "Streaming unavailable, using fallback...".to_string(),
                        });
                        
                        // Try non-streaming request
                        match self.llm.complete(&ctx, req_clone).await {
                            Ok(response) => {
                                crate::info_log!("[RUNTIME] Non-streaming fallback successful");
                                
                                // Emit the full response as a single chunk
                                if !response.content.is_empty() {
                                    let _ = output_tx.send(OutputEvent::ResponseChunk {
                                        content: response.content.clone(),
                                    });
                                }
                                let _ = output_tx.send(OutputEvent::ResponseComplete);
                                
                                return Ok(Observation::LLMCompleted {
                                    intent_id,
                                    response,
                                });
                            }
                            Err(e) => {
                                crate::error_log!("[RUNTIME] Non-streaming fallback also failed: {}", e);
                                
                                // Emit error event so UI can show it
                                let _ = output_tx.send(OutputEvent::Error {
                                    message: format!("LLM API error: {} (streaming: {})", e, stream_error),
                                });
                                
                                return Err(RuntimeError::LLMRequestFailed {
                                    provider: "llm".to_string(),
                                    error: format!("{} (streaming failed: {})", e, stream_error),
                                });
                            }
                        }
                    }
                    
                    crate::info_log!("[RUNTIME] Stream complete: {} chunks, {} bytes", chunk_count, full_content.len());
                    
                    // Emit completion
                    let _ = output_tx.send(OutputEvent::ResponseComplete);
                    
                    Ok(Observation::LLMCompleted {
                        intent_id,
                        response: crate::agent::types::events::LLMResponse {
                            content: full_content,
                            usage: crate::agent::types::events::TokenUsage::default(),
                            model: "unknown".to_string(),
                            provider: "unknown".to_string(),
                            finish_reason: crate::agent::types::events::FinishReason::Stop,
                            structured: None,
                        },
                    })
                } else {
                    // Non-streaming fallback
                    let response = self.llm.complete(&ctx, req).await
                        .map_err(|e| RuntimeError::LLMRequestFailed {
                            provider: "llm".to_string(),
                            error: e.to_string(),
                        })?;
                    
                    // Emit telemetry: token usage
                    let _ = self.telemetry_tx.send(TelemetryEvent::TokenUsage {
                        intent_id,
                        prompt_tokens: response.usage.prompt_tokens,
                        completion_tokens: response.usage.completion_tokens,
                    });
                    
                    Ok(Observation::LLMCompleted {
                        intent_id,
                        response,
                    })
                }
            }
            Intent::RequestApproval(req) => {
                // Emit approval requested event for UI
                if let Some(ref output_tx) = self.output_tx {
                    let _ = output_tx.send(OutputEvent::ApprovalRequested {
                        intent_id,
                        tool: req.tool.clone(),
                        args: req.args.clone(),
                    });
                }
                
                // Use approval capability for user confirmation
                let ctx = RuntimeContext::new()
                    .with_terminal(Arc::clone(&self.terminal));
                
                match self.approval.request(&ctx, req).await {
                    Ok(outcome) => {
                        Ok(Observation::ApprovalCompleted {
                            intent_id,
                            outcome,
                        })
                    }
                    Err(e) => {
                        Err(RuntimeError::Internal {
                            message: format!("Approval request failed: {}", e),
                        })
                    }
                }
            }
            Intent::SpawnWorker(spec) => {
                // Spawn worker via capability
                let ctx = RuntimeContext::new()
                    .with_terminal(Arc::clone(&self.terminal));
                let handle = self.workers.spawn(&ctx, spec.clone()).await
                    .map_err(|e| RuntimeError::Internal { message: e.to_string() })?;
                
                // Emit telemetry: worker spawned
                let _ = self.telemetry_tx.send(TelemetryEvent::WorkerSpawned {
                    worker_id: handle.id,
                    objective: spec.objective.clone(),
                });
                
                Ok(Observation::WorkerSpawned {
                    intent_id,
                    worker_id: handle.id,
                })
            }
            Intent::EmitResponse(text) => {
                Ok(Observation::ResponseEmitted {
                    intent_id,
                    content: text,
                    is_partial: false,
                })
            }
            Intent::Halt(reason) => {
                crate::info_log!("[RUNTIME] Halt intent executed: {:?}", reason);
                Ok(Observation::Halted {
                    intent_id,
                    reason: match reason {
                        crate::agent::contract::intents::ExitReason::Completed => {
                            crate::agent::contract::observations::HaltReason::Completed
                        }
                        crate::agent::contract::intents::ExitReason::StepLimit => {
                            crate::agent::contract::observations::HaltReason::StepLimitReached { max_steps: 0 }
                        }
                        crate::agent::contract::intents::ExitReason::UserRequest => {
                            crate::agent::contract::observations::HaltReason::UserRequest
                        }
                        crate::agent::contract::intents::ExitReason::Error(msg) => {
                            crate::agent::contract::observations::HaltReason::Error(msg)
                        }
                        crate::agent::contract::intents::ExitReason::Interrupted => {
                            crate::agent::contract::observations::HaltReason::Interrupted
                        }
                    },
                })
            }
        };

        // Emit telemetry: intent completed
        let duration_ms = start_time.elapsed().as_millis() as u64;
        let success = result.is_ok();
        let _ = self.telemetry_tx.send(TelemetryEvent::IntentCompleted {
            intent_id,
            duration_ms,
            success,
        });

        result
    }
}

/// Get a human-readable name for an intent type
fn intent_type_name(intent: &Intent) -> &'static str {
    match intent {
        Intent::CallTool(_) => "CallTool",
        Intent::RequestLLM(_) => "RequestLLM",
        Intent::RequestApproval(_) => "RequestApproval",
        Intent::SpawnWorker(_) => "SpawnWorker",
        Intent::EmitResponse(_) => "EmitResponse",
        Intent::Halt(_) => "Halt",
    }
}

#[async_trait]
impl AgencyRuntime for ContractRuntime {
    async fn execute(&self, intent: Intent) -> Result<Observation, RuntimeError> {
        // For single intent execution, generate a new intent_id
        // In practice, this should come from the caller
        let intent_id = IntentId::new(0);
        self.execute_intent(intent_id, intent).await
    }

    async fn execute_with_id(&self, intent_id: IntentId, intent: Intent) -> Result<Observation, RuntimeError> {
        self.execute_intent(intent_id, intent).await
    }

    async fn execute_dag(
        &self,
        graph: &IntentGraph,
    ) -> Result<Vec<(IntentId, Observation)>, RuntimeError> {
        // Use the DAG executor
        let result = match DagExecutor::execute(Arc::new(self.clone()), graph).await {
            Ok(result) => result,
            Err(e) => {
                // Convert error to RuntimeError observation so it flows back to the engine
                crate::error_log!("[RUNTIME] DagExecutor error: {}", e);
                
                // Create a single RuntimeError observation for the error
                // The kernel will handle this and halt the session
                let error_obs = Observation::RuntimeError {
                    intent_id: IntentId::new(0),
                    error: crate::agent::contract::observations::ExecutionError::new(e.to_string()),
                };
                return Ok(vec![(IntentId::new(0), error_obs)]);
            }
        };
        
        // Convert errors to RuntimeError observations so they flow back to the engine
        let mut observations = result.observations;
        for (intent_id, error) in &result.errors {
            crate::error_log!("[RUNTIME] Error for intent {:?}: {}", intent_id, error);
            
            let _ = self.telemetry_tx.send(TelemetryEvent::Error {
                intent_id: Some(*intent_id),
                error: error.to_string(),
            });
            
            // Create an observation for the error so the kernel can handle it
            observations.push((
                *intent_id,
                Observation::RuntimeError {
                    intent_id: *intent_id,
                    error: ExecutionError::new(error.to_string()),
                },
            ));
        }
        
        Ok(observations)
    }

    fn subscribe_telemetry(&self) -> broadcast::Receiver<TelemetryEvent> {
        self.telemetry_tx.subscribe()
    }

    async fn health_check(&self) -> HealthStatus {
        // Check if all components are healthy
        // For now, always return healthy
        HealthStatus::Healthy
    }

    async fn shutdown(&self) -> Result<(), RuntimeError> {
        // Graceful shutdown of all components
        // TODO: Implement actual shutdown logic for workers, etc.
        Ok(())
    }
}

impl Clone for ContractRuntime {
    fn clone(&self) -> Self {
        Self {
            tools: self.tools.clone(),
            llm: self.llm.clone(),
            workers: self.workers.clone(),
            approval: self.approval.clone(),
            telemetry: self.telemetry.clone(),
            telemetry_tx: self.telemetry_tx.clone(),
            output_tx: self.output_tx.clone(),
            terminal: Arc::clone(&self.terminal),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::contract::{
        intents::{ToolCall, LLMRequest, Context},
    };

    #[tokio::test]
    async fn test_tool_execution() {
        // This would need a mock LlmClient
        // For now, just verify the structure compiles
    }

    #[test]
    fn test_intent_type_name() {
        let tool_intent = Intent::CallTool(ToolCall::new(
            "test",
            serde_json::json!({}),
        ));
        assert_eq!(intent_type_name(&tool_intent), "CallTool");

        let llm_intent = Intent::RequestLLM(LLMRequest::new(Context::new("test")));
        assert_eq!(intent_type_name(&llm_intent), "RequestLLM");
    }
}
