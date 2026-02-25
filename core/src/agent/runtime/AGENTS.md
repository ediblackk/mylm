# Runtime Module

**Purpose:** Async capability execution. All side effects live here.

This layer interprets `AgentDecision` from cognition into actual actions.

## Directory Structure

| Path | Purpose | Key Items |
|------|---------|-----------|
| `mod.rs` | Module exports | Re-exports all runtime types |
| `core/` | Core types/traits | Fundamental definitions |
| `core/mod.rs` | Core exports | `RuntimeContext`, `TraceId` |
| `core/capability.rs` | Capability traits | `LLMCapability`, `ToolCapability`, etc. |
| `core/context.rs` | Runtime context | `RuntimeContext`, execution context |
| `core/error.rs` | Error types | `RuntimeError`, `LLMError`, etc. |
| `core/terminal.rs` | Terminal execution | `TerminalExecutor` trait |
| `executor/` | Decision interpretation | |
| `executor/mod.rs` | Executor exports | `AgentRuntime`, `CapabilityGraph` |
| `executor/runtime.rs` | Main runtime | `AgentRuntime::interpret()` |
| `executor/graph.rs` | Capability container | `CapabilityGraph` |
| `capabilities/` | Capability implementations | |
| `capabilities/mod.rs` | Cap exports | All capability impls |
| `capabilities/llm.rs` | LLM capability | `LlmClientCapability` |
| `capabilities/local.rs` | Local tools | `ToolRegistryCapability` |
| `capabilities/approval.rs` | Approval | `TerminalApprovalCapability`, `AutoApproveCapability` |
| `capabilities/worker.rs` | Workers | `LocalWorkerCapability` |
| `capabilities/telemetry.rs` | Telemetry | `ConsoleTelemetry`, `MemoryCapability` |
| `capabilities/memory.rs` | Memory | `MemoryCapability` |
| `capabilities/retry.rs` | Retry wrapper | Retry decorators |
| `governance/` | Policy enforcement | |
| `governance/mod.rs` | Gov exports | Enforcers |
| `governance/enforcer.rs` | Main enforcer | `GovernanceEnforcer` |
| `governance/authority.rs` | Authority checks | Permission validation |
| `governance/claim_enforcer.rs` | Claim enforcement | Resource claim validation |
| `governance/worker_stall.rs` | Worker monitoring | Stall detection |
| `orchestrator/` | Orchestration layer | |
| `orchestrator/mod.rs` | Orch exports | `Session`, `UserInput`, `OutputEvent` |
| `orchestrator/orchestrator.rs` | Main session | `AgencySession`, event loop |
| `orchestrator/dag_executor.rs` | DAG execution | Intent graph execution |
| `orchestrator/contract_bridge.rs` | Contract bridge | Legacy compatibility |
| `orchestrator/commonbox/` | Coordination | Inter-agent coordination |
| `orchestrator/transport/` | Transport | Event transport |
| `stubs/` | Test utilities | |
| `stubs/mod.rs` | Stub exports | `StubLLM`, `StubTools`, etc. |

## Capability Traits

| Trait | Purpose | Methods |
|-------|---------|---------|
| `LLMCapability` | Text completion | `complete(ctx, req) -> LLMResponse` |
| `ToolCapability` | Tool execution | `execute(ctx, call) -> ToolResult` |
| `ApprovalCapability` | User approval | `request(ctx, req) -> ApprovalOutcome` |
| `WorkerCapability` | Spawn workers | `spawn(ctx, spec) -> WorkerHandle` |
| `TelemetryCapability` | Logging/metrics | `record_decision()`, `record_result()` |

## Dependencies

- Uses `crate::agent::types` (primitive types)
- Uses `crate::agent::cognition` (for `AgentDecision`, `AgentState`)
- Uses `crate::agent::tools` (tool implementations)
- NO dependency on session/ (session uses runtime)
