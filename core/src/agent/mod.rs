//! Agent V3 - Capability Graph Architecture
//!
//! Clean, layered agent architecture with strict separation of concerns.
//!
//! # Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  SESSION          Orchestration layer (async)               │
//! │  - Session: Main event loop                                 │
//! │  - Input handlers: Chat, Task, Worker                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │  RUNTIME          Async capability execution (side effects) │
//! │  - AgentRuntime: Decision interpreter                       │
//! │  - CapabilityGraph: Trait-based capability container        │
//! │  - Capability traits: LLM, Tools, Approval, Workers, Telemetry│
//! ├─────────────────────────────────────────────────────────────┤
//! │  COGNITION        Pure state machine (no async/IO)          │
//! │  - StepEngine: (state, input) -> Transition                 │
//! │  - GraphEngine: events -> IntentGraph (DAG)                 │
//! │  - AgentState: Immutable snapshot                           │
//! │  - AgentDecision: Intent only (no execution)                │
//! ├─────────────────────────────────────────────────────────────┤
//! │  TYPES            Primitive types (no dependencies)         │
//! │  - IDs: TaskId, WorkerId, SessionId, TraceId               │
//! │  - Common: TokenUsage, ToolResult, Approval                │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Module Documentation
//!
//! - See `README.md` for architecture overview
//! - See `types/MOD.md` for types module
//! - See `cognition/MOD.md` for cognition module
//! - See `runtime/MOD.md` for runtime module
//! - See `runtime/impls/MOD.md` for capability implementations
//! - See `session/MOD.md` for session module
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use mylm_core::agent::{
//!     AgentBuilder, SessionInput, presets::testing_agent,
//! };
//! use tokio::sync::mpsc;
//!
//! // Quick testing agent (all stubs)
//! let mut agent = testing_agent();
//!
//! // Or build custom
//! let mut agent = AgentBuilder::new()
//!     .with_llm_client(llm_client)
//!     .with_tools(ToolRegistry::new())
//!     .with_terminal_approval()
//!     .build_with_planner();
//!
//! // Run session
//! let (tx, rx) = mpsc::channel(10);
//! tx.send(SessionInput::Chat("Hello".to_string())).await?;
//! drop(tx);
//!
//! let result = agent.run(rx).await?;
//! ```
//!
//! # Rules
//!
//! 1. **Cognition is pure**: No async, no IO, no external deps
//! 2. **Runtime handles side effects**: All IO, network, files
//! 3. **Session orchestrates**: Connects layers in a loop
//! 4. **Capabilities are swappable**: Implement traits to replace components
//! 5. **State is immutable**: Each step produces new state

#![forbid(unsafe_code)]

pub mod types;
pub mod cognition;
pub mod runtime;
pub mod session;
pub mod tools;
pub mod builder;
pub mod worker;
pub mod factory;
pub mod memory;
pub mod identity;


// Governance is now part of runtime::governance

/// Contract module - Core type definitions and traits
///
// Note: The contract module has been moved to runtime/ and types/.
// Use runtime::orchestrator for GraphEngine, runtime::orchestrator for Session,
// and runtime::orchestrator::transport for EventTransport.

// Selective re-exports to avoid ambiguity
pub use types::{
    TaskId, JobId, SessionId,
    TokenUsage, ToolResult, Approval,
    ResponseParser, ParsedResponse, ParseError,
};

pub use cognition::{
    AgentState, Message, WorkerId,
    InputEvent, ApprovalOutcome, LLMResponse,
    AgentDecision, Transition, StepEngine,
    CognitiveError,
    Planner, LlmEngine, GraphEngine, StubEngine, StubGraphEngine,
};

pub use runtime::{
    AgentRuntime, RuntimeContext, RuntimeError, TraceId,
    CapabilityGraph,
    LLMCapability, ToolCapability, ApprovalCapability, WorkerCapability, TelemetryCapability,
    UserInput, OutputEvent,
};

pub use builder::{AgentBuilder, presets};

pub use worker::{
    WorkerManager, WorkerHandle, WorkerResult, WorkerSpawnParams,
};

pub use factory::{
    AgentSessionFactory, FactoryError,
};

pub use crate::config::agent::{
    AgentConfig, ToolConfig, LlmConfig,
    RetryConfig, MemoryConfig, WorkerConfig, TelemetryConfig, EnvConfig,
};

pub use session::{
    Session, SessionConfig, SessionError,
    SessionInput, WorkerEvent,
    SessionPersistence, PersistedSession, SessionMetadata,
    AgentStateCheckpoint, SessionBuilder,
    persistence::SessionData,
};

pub use memory::{
    AgentMemoryManager, MemoryMode, MemoryStats,
    MemoryContextBuilder, InjectionStrategy,
    MemoryExtractor, ExtractedMemory, extract_memories,
};

// MemoryProvider temporarily removed - TODO: restore with new architecture
// pub use cognition::llm_engine::MemoryProvider;

#[cfg(test)]
mod tests {
    mod test_architecture;
    mod example_integration;
    mod integration_tests;
    mod read_file_e2e;
    // TODO: Fix worker_tests compilation errors
    // mod worker_tests;
}
