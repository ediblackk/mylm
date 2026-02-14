//! In-memory transport implementation
//!
//! For single-process use. Uses mpsc channels for communication.
//! Preserves FIFO ordering per session.

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::agent::contract::{
    transport::{EventTransport, TransportError, TransportCapabilities, DeliveryGuarantee},
    envelope::{KernelEventEnvelope, OrderingGuarantee},

};

/// In-memory transport for single-process use
///
/// This is the simplest transport implementation, suitable for:
/// - Unit tests
/// - Single-process CLI applications
/// - Development and debugging
///
/// # Ordering Guarantee
/// FIFO per session (single producer, single consumer).
///
/// # Example
/// ```rust,ignore
/// let (mut transport, mut receiver) = InMemoryTransport::new(100);
///
/// // Publish an event
/// transport.publish(envelope).await?;
///
/// // Receive events
/// let batch = transport.next_batch().await?;
/// ```
pub struct InMemoryTransport {
    /// Channel for receiving events
    rx: mpsc::Receiver<KernelEventEnvelope>,
    /// Channel for sending events
    tx: mpsc::Sender<KernelEventEnvelope>,
    /// Buffer for batching
    buffer: Vec<KernelEventEnvelope>,
    /// Batch size limit
    batch_size: usize,
    /// Whether transport is closed
    closed: bool,
}

impl InMemoryTransport {
    /// Create a new in-memory transport
    ///
    /// # Arguments
    /// * `buffer_size` - Size of the internal channel buffer
    pub fn new(buffer_size: usize) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);
        Self {
            rx,
            tx,
            buffer: Vec::new(),
            batch_size: 100,
            closed: false,
        }
    }

    /// Create a new transport with custom batch size
    pub fn with_batch_size(buffer_size: usize, batch_size: usize) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);
        Self {
            rx,
            tx,
            buffer: Vec::new(),
            batch_size,
            closed: false,
        }
    }

    /// Get a sender handle for this transport
    ///
    /// This can be used by other tasks to publish events.
    pub fn sender(&self) -> mpsc::Sender<KernelEventEnvelope> {
        self.tx.clone()
    }

    /// Get the channel capacity
    pub fn capacity(&self) -> usize {
        self.tx.max_capacity()
    }

    /// Check if channel is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty() && self.rx.is_empty()
    }
}

#[async_trait]
impl EventTransport for InMemoryTransport {
    async fn next_batch(&mut self) -> Result<Vec<KernelEventEnvelope>, TransportError> {
        if self.closed {
            return Ok(Vec::new());
        }

        // If we have buffered events, return them first
        if !self.buffer.is_empty() {
            let batch = std::mem::take(&mut self.buffer);
            return Ok(batch);
        }

        // Wait for at least one event
        match self.rx.recv().await {
            Some(envelope) => {
                let mut batch = vec![envelope];
                
                // Collect more events up to batch_size (non-blocking)
                while batch.len() < self.batch_size {
                    match self.rx.try_recv() {
                        Ok(envelope) => batch.push(envelope),
                        Err(_) => break, // Channel empty
                    }
                }
                
                Ok(batch)
            }
            None => {
                // Channel closed
                self.closed = true;
                Ok(Vec::new())
            }
        }
    }

    async fn publish(&mut self, event: KernelEventEnvelope) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::NotAvailable {
                reason: "Transport closed".to_string(),
            });
        }

        match self.tx.send(event).await {
            Ok(_) => Ok(()),
            Err(_) => Err(TransportError::Disconnected {
                reason: "Receiver dropped".to_string(),
            })
        }
    }

    async fn flush(&mut self) -> Result<(), TransportError> {
        // In-memory transport is synchronous, nothing to flush
        Ok(())
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.closed = true;
        // Drop the sender to signal channel closure
        drop(self.tx.clone());
        Ok(())
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            can_publish: true,
            can_receive: true,
            delivery_guarantee: DeliveryGuarantee::AtLeastOnce,
            ordering_guarantee: OrderingGuarantee::Fifo,
            supports_persistence: false,
            supports_broadcast: false,
            max_message_size: 0, // Unlimited
            supports_batching: true,
        }
    }
}

/// Creates a pair of connected in-memory transports
///
/// Useful for testing bidirectional communication.
pub fn connected_pair(buffer_size: usize) -> (InMemoryTransport, InMemoryTransport) {
    let (tx1, rx1) = mpsc::channel(buffer_size);
    let (tx2, rx2) = mpsc::channel(buffer_size);

    let transport1 = InMemoryTransport {
        rx: rx1,
        tx: tx2,
        buffer: Vec::new(),
        batch_size: 100,
        closed: false,
    };

    let transport2 = InMemoryTransport {
        rx: rx2,
        tx: tx1,
        buffer: Vec::new(),
        batch_size: 100,
        closed: false,
    };

    (transport1, transport2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::contract::{
        ids::{EventId, NodeId, SessionId, LogicalClock},
        events::KernelEvent,
    };

    #[tokio::test]
    async fn test_basic_publish_receive() {
        let mut transport = InMemoryTransport::new(10);

        // Create a test event
        let event = KernelEventEnvelope::new(
            EventId::new(1),
            NodeId::new(1),
            LogicalClock::new(1),
            SessionId::new("test"),
            KernelEvent::UserMessage {
                content: "hello".to_string(),
            },
            1,
        );

        // Publish
        transport.publish(event.clone()).await.unwrap();

        // Receive
        let batch = transport.next_batch().await.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].id.0, 1);
    }

    #[tokio::test]
    async fn test_batching() {
        let mut transport = InMemoryTransport::with_batch_size(10, 5);

        // Publish 10 events
        for i in 0..10 {
            let event = KernelEventEnvelope::new(
                EventId::new(i),
                NodeId::new(1),
                LogicalClock::new(i as u64),
                SessionId::new("test"),
                KernelEvent::UserMessage {
                    content: format!("msg{}", i),
                },
                i as u64,
            );
            transport.publish(event).await.unwrap();
        }

        // Should receive in batches
        let batch1 = transport.next_batch().await.unwrap();
        assert_eq!(batch1.len(), 5); // First batch of 5

        let batch2 = transport.next_batch().await.unwrap();
        assert_eq!(batch2.len(), 5); // Second batch of 5
    }

    #[tokio::test]
    async fn test_fifo_ordering() {
        let mut transport = InMemoryTransport::new(10);

        // Publish events in order
        for i in 0..5 {
            let event = KernelEventEnvelope::new(
                EventId::new(i),
                NodeId::new(1),
                LogicalClock::new(i as u64),
                SessionId::new("test"),
                KernelEvent::UserMessage {
                    content: format!("msg{}", i),
                },
                i as u64,
            );
            transport.publish(event).await.unwrap();
        }

        // Receive and verify order
        let batch = transport.next_batch().await.unwrap();
        assert_eq!(batch.len(), 5);
        for (i, envelope) in batch.iter().enumerate() {
            assert_eq!(envelope.id.0, i as u64);
        }
    }
}
