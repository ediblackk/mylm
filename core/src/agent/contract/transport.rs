//! EventTransport trait - pluggable event queue
//!
//! The transport abstracts how events flow between components.
//! It enables:
//! - Local in-memory transport (for TUI)
//! - Distributed transport (for multi-node)
//! - Persistent transport (for replay/debugging)
//! - Bridging between different transport types

use async_trait::async_trait;

use super::{
    envelope::KernelEventEnvelope,
    ContractError,
};

/// EventTransport trait - abstracts event source/sink
///
/// The transport is the boundary between:
/// - Session and external world
/// - Multiple nodes in distributed setup
/// - Current execution and replay
///
/// # Implementations
/// - InMemoryTransport: Single process, channels
/// - ChannelTransport: Multi-threaded, mpsc
/// - FileTransport: Read/write from log file
/// - WebSocketTransport: Network connected
/// - RedisTransport: Redis streams
/// - KafkaTransport: Kafka topics
/// - HybridTransport: Combines multiple
#[async_trait]
pub trait EventTransport: Send + Sync {
    /// Get next batch of events
    ///
    /// Returns a batch of events ready to be processed.
    /// The batch may be empty if no events are available.
    /// This method should not block indefinitely.
    ///
    /// # Returns
    /// Batch of event envelopes, or empty if none available
    async fn next_batch(&mut self) -> Result<Vec<KernelEventEnvelope>, TransportError>;

    /// Publish an event to the transport
    ///
    /// The event will be delivered to all subscribers.
    /// Delivery guarantees depend on transport implementation.
    ///
    /// # Arguments
    /// * `event` - The event envelope to publish
    async fn publish(&mut self, event: KernelEventEnvelope) -> Result<(), TransportError>;

    /// Flush any buffered events
    ///
    /// Ensures all published events are persisted/sent.
    async fn flush(&mut self) -> Result<(), TransportError>;

    /// Close the transport gracefully
    async fn close(&mut self) -> Result<(), TransportError>;

    /// Get transport capabilities
    fn capabilities(&self) -> TransportCapabilities;
}

/// Errors that can occur in transport
#[derive(Debug, Clone)]
pub enum TransportError {
    /// Connection lost
    Disconnected { reason: String },

    /// Publish failed
    PublishFailed { event_id: super::ids::EventId, error: String },

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
            TransportError::Disconnected { reason } => write!(f, "Transport disconnected: {}", reason),
            TransportError::PublishFailed { event_id, error } => {
                write!(f, "Failed to publish event {:?}: {}", event_id, error)
            }
            TransportError::ReceiveFailed { error } => write!(f, "Receive failed: {}", error),
            TransportError::Serialization { error } => write!(f, "Serialization error: {}", error),
            TransportError::NotAvailable { reason } => write!(f, "Transport not available: {}", reason),
            TransportError::Timeout { operation, duration_ms } => {
                write!(f, "Timeout after {}ms: {}", duration_ms, operation)
            }
            TransportError::Internal { message } => write!(f, "Transport internal error: {}", message),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<ContractError> for TransportError {
    fn from(e: ContractError) -> Self {
        TransportError::Internal { message: e.to_string() }
    }
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
    pub ordering_guarantee: super::envelope::OrderingGuarantee,

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
            ordering_guarantee: super::envelope::OrderingGuarantee::Causal,
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
            ordering_guarantee: super::envelope::OrderingGuarantee::Total,
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
            ordering_guarantee: super::envelope::OrderingGuarantee::None,
            supports_persistence: true,
            supports_broadcast: false,
            max_message_size: 0,
            supports_batching: true,
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

/// Transport factory for creating transports
pub trait TransportFactory: Send + Sync {
    /// Create a new transport instance
    fn create(&self, config: TransportConfig) -> Result<Box<dyn EventTransport>, TransportError>;
    
    /// Transport name
    fn name(&self) -> &str;
}

/// Composite transport that wraps multiple transports
/// 
/// Useful for:
/// - Logging all events to file while processing
/// - Mirroring to multiple destinations
pub struct CompositeTransport {
    transports: Vec<Box<dyn EventTransport>>,
}

impl CompositeTransport {
    pub fn new(transports: Vec<Box<dyn EventTransport>>) -> Self {
        Self { transports }
    }

    /// Add a transport
    pub fn add(&mut self, transport: Box<dyn EventTransport>) {
        self.transports.push(transport);
    }
}

#[async_trait]
impl EventTransport for CompositeTransport {
    async fn next_batch(&mut self) -> Result<Vec<KernelEventEnvelope>, TransportError> {
        // Use first transport that supports receiving
        for transport in &mut self.transports {
            let caps = transport.capabilities();
            if caps.can_receive {
                return transport.next_batch().await;
            }
        }
        Ok(Vec::new())
    }

    async fn publish(&mut self, event: KernelEventEnvelope) -> Result<(), TransportError> {
        // Publish to all transports that support publishing
        for transport in &mut self.transports {
            if transport.capabilities().can_publish {
                transport.publish(event.clone()).await?;
            }
        }
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), TransportError> {
        for transport in &mut self.transports {
            transport.flush().await?;
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        for transport in &mut self.transports {
            transport.close().await?;
        }
        Ok(())
    }

    fn capabilities(&self) -> TransportCapabilities {
        // Aggregate capabilities
        TransportCapabilities {
            can_publish: self.transports.iter().any(|t| t.capabilities().can_publish),
            can_receive: self.transports.iter().any(|t| t.capabilities().can_receive),
            delivery_guarantee: DeliveryGuarantee::AtLeastOnce,
            ordering_guarantee: super::envelope::OrderingGuarantee::None,
            supports_persistence: self.transports.iter().any(|t| t.capabilities().supports_persistence),
            supports_broadcast: self.transports.iter().any(|t| t.capabilities().supports_broadcast),
            max_message_size: 0,
            supports_batching: self.transports.iter().all(|t| t.capabilities().supports_batching),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_capabilities() {
        let full = TransportCapabilities::full();
        assert!(full.can_publish);
        assert!(full.can_receive);
        assert!(full.supports_persistence);

        let read_only = TransportCapabilities::read_only();
        assert!(!read_only.can_publish);
        assert!(read_only.can_receive);
    }

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.poll_interval_ms, 10);
    }
}
