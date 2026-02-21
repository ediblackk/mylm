//! Orchestrator Layer
//!
//! Coordinates cognition and runtime layers.
//! Main event loop, input/output handling, DAG execution, and orchestration lifecycle.

pub mod orchestrator;
pub mod transport;
pub mod contract_bridge;
pub mod dag_executor;
pub mod commonbox;

pub use orchestrator::{
    Session, UserInput, OutputEvent, SessionStatus, SessionResult, SessionError,
    AgencySession,
};
pub use transport::{
    EventTransport, TransportError, TransportCapabilities, DeliveryGuarantee, TransportConfig,
};
pub use contract_bridge::{ContractRuntime, OutputSender};
pub use commonbox::{
    Commonbox, CommonboxEntry, CommonboxEvent, CommonboxError,
    Job, JobId, JobStatus, JobResult,
    AgentStatus, EntryUpdate, RoutedQuery,
    CoordinationBoard, CoordinationEntry,
};
