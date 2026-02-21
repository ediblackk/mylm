//! Composite transport that wraps multiple transports
//!
//! Useful for:
//! - Logging all events to file while processing
//! - Mirroring to multiple destinations

use crate::agent::runtime::orchestrator::transport::{
    DeliveryGuarantee, EventTransport, TransportCapabilities, TransportError,
};
use crate::agent::types::envelope::KernelEventEnvelope;
use async_trait::async_trait;

/// Composite transport that wraps multiple transports
///
/// Useful for:
/// - Logging all events to file while processing
/// - Mirroring to multiple destinations
pub struct CompositeTransport {
    transports: Vec<Box<dyn EventTransport>>,
}

impl CompositeTransport {
    /// Create a new composite transport
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
            can_publish: self
                .transports
                .iter()
                .any(|t| t.capabilities().can_publish),
            can_receive: self
                .transports
                .iter()
                .any(|t| t.capabilities().can_receive),
            delivery_guarantee: DeliveryGuarantee::AtLeastOnce,
            ordering_guarantee: crate::agent::types::envelope::OrderingGuarantee::None,
            supports_persistence: self
                .transports
                .iter()
                .any(|t| t.capabilities().supports_persistence),
            supports_broadcast: self
                .transports
                .iter()
                .any(|t| t.capabilities().supports_broadcast),
            max_message_size: 0,
            supports_batching: self
                .transports
                .iter()
                .all(|t| t.capabilities().supports_batching),
        }
    }

    fn inject(&self, event: KernelEventEnvelope) -> Result<(), TransportError> {
        // Inject into all transports that support receiving
        for transport in &self.transports {
            if transport.capabilities().can_receive {
                transport.inject(event.clone())?;
            }
        }
        Ok(())
    }
}
