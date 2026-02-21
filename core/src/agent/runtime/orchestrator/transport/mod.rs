//! EventTransport trait - pluggable event queue
//!
//! The transport abstracts how events flow between components.
//! It enables:
//! - Local in-memory transport (for TUI)
//! - Distributed transport (for multi-node)
//! - Persistent transport (for replay/debugging)
//! - Bridging between different transport types

pub mod composite;
pub mod memory;
pub mod types;

pub use composite::CompositeTransport;
pub use memory::{connected_pair, InMemoryTransport};
pub use types::{
    DeliveryGuarantee, TransportCapabilities, TransportConfig, TransportError,
};

use crate::agent::types::envelope::KernelEventEnvelope;
use async_trait::async_trait;

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

    /// Inject an event from outside the transport (e.g., user input)
    ///
    /// This allows external sources to inject events without going through
    /// the normal publish flow. Used for unified input handling.
    fn inject(&self, event: KernelEventEnvelope) -> Result<(), TransportError>;
    
    /// Get the unique instance ID for this transport
    ///
    /// Used for identity verification across session moves.
    /// Default implementation returns 0 for transports that don't support identity.
    fn instance_id(&self) -> u64 {
        0
    }
}

/// Transport factory for creating transports
pub trait TransportFactory: Send + Sync {
    /// Create a new transport instance
    fn create(&self, config: TransportConfig) -> Result<Box<dyn EventTransport>, TransportError>;

    /// Transport name
    fn name(&self) -> &str;
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
