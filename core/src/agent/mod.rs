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
//! │  - CognitiveEngine: (state, input) -> Transition            │
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
//!     .build_with_llm_engine();
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
pub mod builder;
pub mod worker;
pub mod factory;
pub mod memory;

/// Contract module - Core type definitions and traits
///
/// This module defines the stable contracts between:
/// - AgencyKernel (pure, sync, deterministic)
/// - AgencyRuntime (async, side effects)
/// - EventTransport (pluggable event queue)
/// - Session (orchestration with dynamic DAG expansion)
pub mod contract;

// Selective re-exports to avoid ambiguity
pub use types::{
    TaskId, JobId, SessionId,
    TokenUsage, ToolResult, Approval,
};

pub use cognition::{
    AgentState, Message, WorkerId,
    InputEvent, ApprovalOutcome, LLMResponse,
    AgentDecision, Transition, CognitiveEngine,
    CognitiveError,
    LLMBasedEngine, ResponseParser,
};

pub use runtime::{
    AgentRuntime, RuntimeContext, RuntimeError, TraceId,
    CapabilityGraph,
    LLMCapability, ToolCapability, ApprovalCapability, WorkerCapability, TelemetryCapability,
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
};

pub use memory::{
    AgentMemoryManager, MemoryMode, MemoryStats,
    MemoryContextBuilder, InjectionStrategy,
    MemoryExtractor, ExtractedMemory, extract_memories,
};

// MemoryProvider temporarily removed - TODO: restore with new architecture
// pub use cognition::llm_engine::MemoryProvider;

#[cfg(test)]
mod test_architecture;

#[cfg(test)]
mod example_integration;

#[cfg(test)]
mod integration_tests;
