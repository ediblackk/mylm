//! Transport types and configuration

use crate::agent::types::envelope::OrderingGuarantee;
use crate::agent::types::error::ContractError;
use crate::agent::types::ids::EventId;

/// Errors that can occur in transport
#[derive(Debug, Clone)]
pub enum TransportError {
    /// Connection lost
    Disconnected { reason: String },

    /// Publish failed
    PublishFailed { event_id: EventId, error: String },

    /// Receive failed
    ReceiveFailed { error: String },

    /// Serialization error
    Serialization { error: String },

    /// Transport not available
    NotAvailable { reason: String },

    /// Timeout
    Timeout { operation: String, duration_ms: u64 },

    /// Internal error
    Internal { message: String },
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::Disconnected { reason } => {
                write!(f, "Transport disconnected: {}", reason)
            }
            TransportError::PublishFailed { event_id, error } => {
                write!(f, "Failed to publish event {:?}: {}", event_id, error)
            }
            TransportError::ReceiveFailed { error } => write!(f, "Receive failed: {}", error),
            TransportError::Serialization { error } => {
                write!(f, "Serialization error: {}", error)
            }
            TransportError::NotAvailable { reason } => {
                write!(f, "Transport not available: {}", reason)
            }
            TransportError::Timeout {
                operation,
                duration_ms,
            } => write!(f, "Timeout after {}ms: {}", duration_ms, operation),
            TransportError::Internal { message } => {
                write!(f, "Transport internal error: {}", message)
            }
        }
    }
}

impl std::error::Error for TransportError {}

impl From<ContractError> for TransportError {
    fn from(e: ContractError) -> Self {
        TransportError::Internal {
            message: e.to_string(),
        }
    }
}

/// Delivery guarantee level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryGuarantee {
    /// Fire and forget, no guarantee
    AtMostOnce,

    /// Will be delivered at least once (may duplicate)
    AtLeastOnce,

    /// Delivered exactly once
    ExactlyOnce,
}

/// Capabilities of a transport implementation
#[derive(Debug, Clone)]
pub struct TransportCapabilities {
    /// Can publish events
    pub can_publish: bool,

    /// Can receive events
    pub can_receive: bool,

    /// Delivery guarantee
    pub delivery_guarantee: DeliveryGuarantee,

    /// Ordering guarantee
    pub ordering_guarantee: OrderingGuarantee,

    /// Supports persistence (replay)
    pub supports_persistence: bool,

    /// Supports multiple subscribers
    pub supports_broadcast: bool,

    /// Maximum message size (0 = unlimited)
    pub max_message_size: usize,

    /// Batch support
    pub supports_batching: bool,
}

impl TransportCapabilities {
    /// Full-featured transport
    pub fn full() -> Self {
        Self {
            can_publish: true,
            can_receive: true,
            delivery_guarantee: DeliveryGuarantee::AtLeastOnce,
            ordering_guarantee: OrderingGuarantee::Causal,
            supports_persistence: true,
            supports_broadcast: true,
            max_message_size: 0,
            supports_batching: true,
        }
    }

    /// Read-only transport (replay)
    pub fn read_only() -> Self {
        Self {
            can_publish: false,
            can_receive: true,
            delivery_guarantee: DeliveryGuarantee::AtLeastOnce,
            ordering_guarantee: OrderingGuarantee::Total,
            supports_persistence: true,
            supports_broadcast: false,
            max_message_size: 0,
            supports_batching: true,
        }
    }

    /// Write-only transport (logging)
    pub fn write_only() -> Self {
        Self {
            can_publish: true,
            can_receive: false,
            delivery_guarantee: DeliveryGuarantee::AtLeastOnce,
            ordering_guarantee: OrderingGuarantee::None,
            supports_persistence: true,
            supports_broadcast: false,
            max_message_size: 0,
            supports_batching: true,
        }
    }
}

/// Configuration for transport
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Batch size for next_batch()
    pub batch_size: usize,

    /// Polling interval for new events
    pub poll_interval_ms: u64,

    /// Timeout for operations
    pub timeout_ms: u64,

    /// Buffer size for internal channels
    pub buffer_size: usize,

    /// Enable compression
    pub compression: bool,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            poll_interval_ms: 10,
            timeout_ms: 30000,
            buffer_size: 1000,
            compression: false,
        }
    }
}
