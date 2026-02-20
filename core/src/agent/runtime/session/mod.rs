//! Session Orchestration
//!
//! Coordinates cognition and runtime layers.
//! Main event loop, input/output handling, and session lifecycle.

pub mod session;
pub mod transport;
pub mod contract_bridge;
pub mod dag_executor;

pub use session::{
    Session, UserInput, OutputEvent, SessionStatus, SessionResult, SessionError,
    AgencySession,
};
pub use transport::{EventTransport, TransportError, TransportCapabilities, DeliveryGuarantee};
pub use contract_bridge::{ContractRuntime, OutputSender};
