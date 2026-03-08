//! Contract Runtime Implementation
//!
//! Bridges the new contract's AgencyRuntime trait to existing runtime capabilities.

use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::broadcast;
use std::time::Instant;
use futures::StreamExt;
use tokio::time::{timeout, Duration};

use crate::agent::types::intents::{Intent, ExitReason};
use crate::agent::types::graph::IntentGraph;
use crate::agent::types::ids::IntentId;
use crate::agent::types::observations::{Observation, ExecutionError, HaltReason};
use crate::agent::runtime::orchestrator::orchestrator::OutputEvent;
use crate::agent::runtime::core::{
    AgencyRuntime, AgencyRuntimeError, TelemetryEvent, HealthStatus,
};
use crate::agent::runtime::governance::{ClaimEnforcer, ClaimEnforcement};

use crate::agent::runtime::capabilities::{
    LlmClientCapability,
    LocalWorkerCapability,
    ConsoleTelemetry,
    AutoApproveCapability,
};
use crate::agent::runtime::core::LLMCapability;
use crate::agent::runtime::orchestrator::dag_executor::DagExecutor;
use crate::agent::tools::ToolRegistry;
use crate::agent::runtime::core::{WorkerCapability, ToolCapability, ApprovalCapability};
use crate::agent::runtime::core::terminal::{TerminalExecutor, DefaultTerminalExecutor};

use crate::agent::runtime::core::RuntimeContext;
use crate::agent::memory::MemoryProvider;
use crate::conversation::ContextManager;
use crate::provider::LlmClient;

/// Output sender enum to support both broadcast (main session) and mpsc (workers)
#[derive(Clone)]
pub enum OutputSender {
    /// Broadcast sender for main session (multiple subscribers)
    Broadcast(broadcast::Sender<OutputEvent>),
    /// MPSC sender for workers (single consumer)
    Mpsc(tokio::sync::mpsc::Sender<OutputEvent>),
}

impl OutputSender {
    /// Send an output event
    pub fn send(&self, event: OutputEvent) -> Result<(), String> {
        match self {
            OutputSender::Broadcast(tx) => {
                tx.send(event).map_err(|_| "Broadcast send failed: no receivers".to_string())?;
                Ok(())
            }
            OutputSender::Mpsc(tx) => {
                tx.try_send(event).map_err(|e| format!("MPSC send failed: {:?}", e))?;
                Ok(())
            }
        }
    }
}

/// Full runtime implementation that fulfills the AgencyRuntime contract
///
/// This wires together all the existing capability implementations
/// to fulfill the new contract's AgencyRuntime trait.
pub struct ContractRuntime {
    /// Tool execution
    tools: Arc<ToolRegistry>,
    /// LLM completion (trait object for mockability)
    llm: Arc<dyn LLMCapability>,
    /// Worker spawning
    workers: Arc<LocalWorkerCapability>,
    /// Approval handling
    approval: Arc<dyn ApprovalCapability>,
    /// Telemetry
    telemetry: Arc<ConsoleTelemetry>,
    /// Memory provider for saving/retrieving memories
    memory_provider: Option<Arc<dyn MemoryProvider>>,
    /// Telemetry sender
    telemetry_tx: broadcast::Sender<TelemetryEvent>,
    /// Output sender for streaming events (optional)
    output_tx: Option<broadcast::Sender<OutputEvent>>,
    /// Terminal executor for shell commands
    terminal: Arc<dyn TerminalExecutor>,
    /// Claim enforcer for resource coordination (optional)
    claim_enforcer: Option<Arc<ClaimEnforcer>>,
}

impl ContractRuntime {
    /// Create a new runtime with the given LLM client
    /// 
    /// Uses a default terminal executor (std::process::Command).
    /// Uses auto-approve for approval (suitable for testing/non-interactive use).
    /// Use `with_terminal()` and `with_approval()` for custom configuration.
    pub fn new(llm_client: Arc<LlmClient>) -> Self {
        let tools = Arc::new(ToolRegistry::new());
        let context_config = crate::conversation::ContextConfig::default();
        let context_manager = Arc::new(tokio::sync::Mutex::new(ContextManager::new(context_config)));
        let llm: Arc<dyn LLMCapability> = Arc::new(LlmClientCapability::new(llm_client, context_manager));
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
            memory_provider: None,
            telemetry_tx,
            output_tx: None,
            terminal: Arc::new(DefaultTerminalExecutor::new()),
            claim_enforcer: None,
        }
    }
    
    /// Create a new runtime with custom tool registry
    /// 
    /// Uses a default terminal executor (std::process::Command).
    /// Uses auto-approve for approval.
    pub fn with_tools(llm_client: Arc<LlmClient>, tools: Arc<ToolRegistry>) -> Self {
        let context_config = crate::conversation::ContextConfig::default();
        let context_manager = Arc::new(tokio::sync::Mutex::new(ContextManager::new(context_config)));
        let llm: Arc<dyn LLMCapability> = Arc::new(LlmClientCapability::new(llm_client, context_manager));
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
            memory_provider: None,
            telemetry_tx,
            output_tx: None,
            terminal: Arc::new(DefaultTerminalExecutor::new()),
            claim_enforcer: None,
        }
    }
    
    /// Create a new runtime with custom tool registry and memory provider
    /// 
    /// Uses a default terminal executor (std::process::Command).
    /// Uses auto-approve for approval.
    /// Memory provider enables automatic context augmentation from agent memory.
    pub fn with_tools_and_memory(
        llm_client: Arc<LlmClient>, 
        tools: Arc<ToolRegistry>,
        memory_provider: Option<Arc<dyn MemoryProvider>>,
    ) -> Self {
        let context_config = crate::conversation::ContextConfig::default();
        let context_manager = Arc::new(tokio::sync::Mutex::new(ContextManager::new(context_config)));
        let llm_capability = LlmClientCapability::new(llm_client, context_manager);
        
        // Inject memory provider if available
        let llm: Arc<dyn LLMCapability> = if let Some(ref provider) = memory_provider {
            Arc::new(llm_capability.with_memory_provider(provider.clone()))
        } else {
            Arc::new(llm_capability)
        };
        
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
            memory_provider,
            telemetry_tx,
            output_tx: None,
            terminal: Arc::new(DefaultTerminalExecutor::new()),
            claim_enforcer: None,
        }
    }
    
    /// Create a new runtime with custom LLM capability (for testing)
    /// 
    /// This allows injecting mock LLM implementations for testing.
    pub fn with_llm_capability(llm: Arc<dyn LLMCapability>, tools: Arc<ToolRegistry>) -> Self {
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
            memory_provider: None,
            telemetry_tx,
            output_tx: None,
            terminal: Arc::new(DefaultTerminalExecutor::new()),
            claim_enforcer: None,
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
    
    /// Set claim enforcer for resource coordination
    /// 
    /// When set, tools that modify files will require a claim first.
    /// This prevents conflicts when multiple agents work in parallel.
    pub fn with_claim_enforcer(mut self, enforcer: Arc<ClaimEnforcer>) -> Self {
        self.claim_enforcer = Some(enforcer);
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
    ) -> Result<Observation, AgencyRuntimeError> {
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
                
                // CLAIM ENFORCEMENT: Check if agent has claimed the resource
                if let Some(ref claim_enforcer) = self.claim_enforcer {
                    // Get agent_id from context or use default
                    let agent_id = ctx.agent_id()
                        .unwrap_or_else(|| crate::agent::identity::AgentId::worker("unknown"));
                    
                    match claim_enforcer.check_tool_call(&agent_id, &call).await {
                        ClaimEnforcement::Allow => {
                            // Proceed with execution
                        }
                        ClaimEnforcement::Deny { resource, claimed_by } => {
                            return Ok(Observation::ToolCompleted {
                                intent_id,
                                tool: call.name.clone(),
                                result: crate::agent::types::events::ToolResult::Error {
                                    message: format!(
                                        "Access denied: '{}' is claimed by {}. \
                                         Use commonboard to check status or coordinate.",
                                        resource, claimed_by
                                    ),
                                    code: Some("RESOURCE_CLAIMED".to_string()),
                                    retryable: false,
                                },
                                execution_time_ms: 0,
                            });
                        }
                        ClaimEnforcement::RequiresClaim { resource } => {
                            return Ok(Observation::ToolCompleted {
                                intent_id,
                                tool: call.name.clone(),
                                result: crate::agent::types::events::ToolResult::Error {
                                    message: format!(
                                        "You must claim '{}' before modifying it. \
                                         Use: commonboard action=claim resource={}",
                                        resource, resource
                                    ),
                                    code: Some("REQUIRES_CLAIM".to_string()),
                                    retryable: true,
                                },
                                execution_time_ms: 0,
                            });
                        }
                    }
                }
                
                // Execute tool via registry
                let tool_start = Instant::now();
                let result = self.tools.execute(&ctx, call.clone()).await
                    .map_err(|e| AgencyRuntimeError::ToolExecutionFailed {
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
                    tool: call.name.clone(),
                    result,
                    execution_time_ms,
                })
            }
            Intent::RequestLLM(req) => {
                crate::info_log!("[RUNTIME] Executing RequestLLM intent: intent_id={}", intent_id.0);
                // Create runtime context for LLM execution
                let ctx = RuntimeContext::new()
                    .with_terminal(Arc::clone(&self.terminal));
                
                // Use streaming if output sender is available
                if let Some(ref output_tx) = self.output_tx {
                    // Clone request for potential fallback
                    let req_clone = req.clone();
                    
                    crate::info_log!("[RUNTIME] Creating LLM stream...");
                    let mut stream = self.llm.complete_stream(&ctx, req);
                    let mut full_content = String::new();
                    let mut stream_failed = false;
                    let mut stream_error = String::new();
                    let mut accumulated_usage: Option<crate::agent::types::events::TokenUsage> = None;
                    
                    crate::info_log!("[RUNTIME] Starting LLM stream polling (60s timeout)...");
                    let mut chunk_count = 0;
                    let stream_timeout = Duration::from_secs(60);
                    let stream_start = Instant::now();
                    
                    while let Ok(Some(chunk_result)) = timeout(stream_timeout, stream.next()).await {
                        match chunk_result {
                            Ok(chunk) => {
                                crate::debug_log!("[RUNTIME] Got chunk: is_final={}, content_len={}", chunk.is_final, chunk.content.len());
                                if chunk.is_final {
                                    accumulated_usage = chunk.usage;
                                    break;
                                }
                                if !chunk.content.is_empty() {
                                    chunk_count += 1;
                                    full_content.push_str(&chunk.content);
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
                        
                        // Check overall timeout
                        if stream_start.elapsed() > stream_timeout {
                            crate::error_log!("[RUNTIME] Stream timeout after 60s");
                            stream_failed = true;
                            stream_error = "Timeout: LLM stream took too long".to_string();
                            break;
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
                                let _ = output_tx.send(OutputEvent::ResponseComplete { 
                                    usage: Some(response.usage.clone()) 
                                });
                                
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
                                
                                return Err(AgencyRuntimeError::LLMRequestFailed {
                                    provider: "llm".to_string(),
                                    error: format!("{} (streaming failed: {})", e, stream_error),
                                });
                            }
                        }
                    }
                    
                    crate::info_log!("[RUNTIME] Stream complete: {} chunks, {} bytes", chunk_count, full_content.len());
                    
                    // Emit completion with usage
                    let usage = accumulated_usage.unwrap_or_default();
                    let _ = output_tx.send(OutputEvent::ResponseComplete { usage: Some(usage.clone()) });
                    
                    Ok(Observation::LLMCompleted {
                        intent_id,
                        response: crate::agent::types::events::LLMResponse {
                            content: full_content,
                            usage,
                            model: "unknown".to_string(),
                            provider: "unknown".to_string(),
                            finish_reason: crate::agent::types::events::FinishReason::Stop,
                            structured: None,
                        },
                    })
                } else {
                    // Non-streaming fallback
                    let response = self.llm.complete(&ctx, req).await
                        .map_err(|e| AgencyRuntimeError::LLMRequestFailed {
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
                        Err(AgencyRuntimeError::Internal {
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
                    .map_err(|e| AgencyRuntimeError::Internal { message: e.to_string() })?;
                
                // Emit telemetry: worker spawned
                let _ = self.telemetry_tx.send(TelemetryEvent::WorkerSpawned {
                    worker_id: handle.id,
                    objective: spec.objective.clone(),
                });
                
                // Note: job_id and objective come from delegate tool's direct OutputEvent emission
                // This Observation is for the contract bridge path which doesn't have Commonbox access
                Ok(Observation::WorkerSpawned {
                    intent_id,
                    worker_id: handle.id,
                    job_id: crate::agent::runtime::orchestrator::commonbox::JobId::new(), // Placeholder - real job_id comes from delegate
                    objective: spec.objective.clone(),
                    agent_id: handle.id.0.to_string(), // Use numeric worker ID as agent_id
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
                        ExitReason::Completed => {
                            HaltReason::Completed
                        }
                        ExitReason::StepLimit => {
                            HaltReason::StepLimitReached { max_steps: 0 }
                        }
                        ExitReason::UserRequest => {
                            HaltReason::UserRequest
                        }
                        ExitReason::Error(msg) => {
                            HaltReason::Error(msg)
                        }
                        ExitReason::Interrupted => {
                            HaltReason::Interrupted
                        }
                    },
                })
            }
            Intent::Remember { content } => {
                crate::info_log!("[RUNTIME] Remember intent executed: content={}", content);
                
                // Save to memory via memory provider
                if let Some(ref provider) = self.memory_provider {
                    provider.remember(&content);
                    crate::info_log!("[RUNTIME] Memory saved via provider: {}", &content[..content.len().min(50)]);
                } else {
                    crate::warn_log!("[RUNTIME] No memory provider available, cannot save memory");
                }
                
                Ok(Observation::Remembered {
                    intent_id,
                    content,
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
        Intent::Remember { .. } => "Remember",
        Intent::Halt(_) => "Halt",
    }
}

#[async_trait]
impl AgencyRuntime for ContractRuntime {
    async fn execute(&self, intent: Intent) -> Result<Observation, AgencyRuntimeError> {
        // For single intent execution, generate a new intent_id
        // In practice, this should come from the caller
        let intent_id = IntentId::new(0);
        self.execute_intent(intent_id, intent).await
    }

    async fn execute_with_id(&self, intent_id: IntentId, intent: Intent) -> Result<Observation, AgencyRuntimeError> {
        self.execute_intent(intent_id, intent).await
    }

    async fn execute_dag(
        &self,
        graph: &IntentGraph,
    ) -> Result<Vec<(IntentId, Observation)>, AgencyRuntimeError> {
        crate::debug_log!("[RUNTIME] execute_dag called with {} nodes", graph.len());
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
                    error: ExecutionError::new(e.to_string()),
                };
                return Ok(vec![(IntentId::new(0), error_obs)]);
            }
        };
        
        // Convert errors to RuntimeError observations so they flow back to the engine
        let mut observations = result.observations;
        crate::debug_log!("[RUNTIME] execute_dag: {} observations, {} errors", observations.len(), result.errors.len());
        for (intent_id, error) in &result.errors {
            crate::error_log!("[RUNTIME] Converting error for intent {:?}: {}", intent_id, error);
            
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

    async fn shutdown(&self) -> Result<(), AgencyRuntimeError> {
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
            memory_provider: self.memory_provider.clone(),
            telemetry_tx: self.telemetry_tx.clone(),
            output_tx: self.output_tx.clone(),
            terminal: Arc::clone(&self.terminal),
            claim_enforcer: self.claim_enforcer.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::types::intents::{ToolCall, LLMRequest, Context};

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
