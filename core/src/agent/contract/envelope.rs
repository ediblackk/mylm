//! Event envelope for distributed event transport
//!
//! The envelope wraps kernel events with metadata needed for:
//! - Distributed ordering (logical clocks)
//! - Source tracking (which node produced the event)
//! - Replayability (deterministic ordering)

use serde::{Deserialize, Serialize};

use super::ids::{EventId, NodeId, LogicalClock, SessionId};
use super::events::KernelEvent;

/// An envelope wrapping a kernel event with metadata
/// 
/// This is what flows through the EventTransport, not raw KernelEvents.
/// It enables distributed execution, replay, and debugging.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelEventEnvelope {
    /// Unique ID for this event
    pub id: EventId,
    
    /// Which node produced this event
    pub source: NodeId,
    
    /// Logical timestamp for ordering
    pub timestamp: LogicalClock,
    
    /// Session this event belongs to
    pub session_id: SessionId,
    
    /// The actual event payload
    pub payload: KernelEvent,
    
    /// Sequence number within this session (for strict ordering)
    pub sequence: u64,
    
    /// Parent event ID (for causal relationships)
    pub parent_id: Option<EventId>,
    
    /// Trace context for distributed tracing
    pub trace: TraceContext,
}

impl KernelEventEnvelope {
    /// Create a new envelope
    pub fn new(
        id: EventId,
        source: NodeId,
        timestamp: LogicalClock,
        session_id: SessionId,
        payload: KernelEvent,
        sequence: u64,
    ) -> Self {
        Self {
            id,
            source,
            timestamp,
            session_id,
            payload,
            sequence,
            parent_id: None,
            trace: TraceContext::new(),
        }
    }

    /// Set parent event ID
    pub fn with_parent(mut self, parent_id: EventId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Set trace context
    pub fn with_trace(mut self, trace: TraceContext) -> Self {
        self.trace = trace;
        self
    }

    /// Get the intent ID this event relates to (if any)
    pub fn intent_id(&self) -> Option<super::ids::IntentId> {
        use super::events::KernelEvent;
        match &self.payload {
            KernelEvent::ToolCompleted { intent_id, .. } => Some(*intent_id),
            KernelEvent::LLMCompleted { intent_id, .. } => Some(*intent_id),
            KernelEvent::ApprovalGiven { intent_id, .. } => Some(*intent_id),
            _ => None,
        }
    }

    /// Check if this event is from a specific node
    pub fn is_from(&self, node: NodeId) -> bool {
        self.source == node
    }

    /// Create a child event with updated trace
    pub fn child(&self, payload: KernelEvent, next_id: EventId, next_seq: u64) -> Self {
        let mut trace = self.trace.clone();
        trace.add_span(self.id, self.timestamp);
        
        Self {
            id: next_id,
            source: self.source, // Same node for now
            timestamp: self.timestamp.next(),
            session_id: self.session_id.clone(),
            payload,
            sequence: next_seq,
            parent_id: Some(self.id),
            trace,
        }
    }
}

/// Trace context for distributed tracing
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TraceContext {
    /// Root event that started this trace
    pub root_id: Option<EventId>,
    
    /// Span stack - events that led to this one
    pub spans: Vec<TraceSpan>,
}

impl TraceContext {
    /// Create a new empty trace context
    pub fn new() -> Self {
        Self {
            root_id: None,
            spans: Vec::new(),
        }
    }

    /// Create a root trace context
    pub fn root(root_id: EventId) -> Self {
        Self {
            root_id: Some(root_id),
            spans: vec![TraceSpan {
                event_id: root_id,
                timestamp: LogicalClock::zero(),
            }],
        }
    }

    /// Add a span to the trace
    pub fn add_span(&mut self, event_id: EventId, timestamp: LogicalClock) {
        self.spans.push(TraceSpan {
            event_id,
            timestamp,
        });
    }

    /// Get the depth of this trace
    pub fn depth(&self) -> usize {
        self.spans.len()
    }

    /// Check if this is a root event
    pub fn is_root(&self) -> bool {
        self.root_id.is_none() || self.spans.len() <= 1
    }
}

/// A span in the trace
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TraceSpan {
    pub event_id: EventId,
    pub timestamp: LogicalClock,
}

/// Source information for events
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventSource {
    /// From the local user
    LocalUser,
    
    /// From a specific node
    Node(NodeId),
    
    /// From a worker
    Worker(super::events::WorkerId),
    
    /// From a replay/log
    Replay { log_file: String, position: u64 },
    
    /// From an external system
    External { system: String, id: String },
}

/// Ordering guarantee for a transport
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderingGuarantee {
    /// Strict FIFO per producer
    Fifo,
    
    /// Causal ordering (happens-before)
    Causal,
    
    /// Total ordering (all nodes see same order)
    Total,
    
    /// No ordering guarantees
    None,
}

/// Configuration for event envelope handling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeConfig {
    /// This node's ID
    pub node_id: NodeId,
    
    /// Whether to include trace context
    pub enable_tracing: bool,
    
    /// Maximum trace depth
    pub max_trace_depth: usize,
    
    /// Clock type for ordering
    pub clock_type: ClockType,
}

impl EnvelopeConfig {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            enable_tracing: true,
            max_trace_depth: 100,
            clock_type: ClockType::Lamport,
        }
    }
}

impl Default for EnvelopeConfig {
    fn default() -> Self {
        Self::new(NodeId::new(0))
    }
}

/// Type of logical clock to use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClockType {
    /// Lamport timestamps
    Lamport,
    
    /// Vector clocks
    Vector,
    
    /// Hybrid logical/physical
    Hybrid,
}

/// An envelope builder for ergonomic construction
pub struct EnvelopeBuilder {
    source: NodeId,
    session_id: SessionId,
    sequence: u64,
    clock: LogicalClock,
}

impl EnvelopeBuilder {
    /// Create a new builder
    pub fn new(source: NodeId, session_id: SessionId) -> Self {
        Self {
            source,
            session_id,
            sequence: 0,
            clock: LogicalClock::zero(),
        }
    }

    /// Set starting sequence
    pub fn starting_at(mut self, sequence: u64) -> Self {
        self.sequence = sequence;
        self
    }

    /// Set starting clock
    pub fn with_clock(mut self, clock: LogicalClock) -> Self {
        self.clock = clock;
        self
    }

    /// Build an envelope for a payload
    pub fn build(&mut self, payload: KernelEvent, id: EventId) -> KernelEventEnvelope {
        self.sequence += 1;
        self.clock.increment();
        
        KernelEventEnvelope::new(
            id,
            self.source,
            self.clock,
            self.session_id.clone(),
            payload,
            self.sequence,
        )
    }

    /// Build with auto-generated ID
    pub fn build_auto(&mut self, payload: KernelEvent, next_event_id: &mut impl FnMut() -> EventId) -> KernelEventEnvelope {
        self.build(payload, next_event_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::events::KernelEvent;

    #[test]
    fn test_envelope_creation() {
        let envelope = KernelEventEnvelope::new(
            EventId::new(1),
            NodeId::new(42),
            LogicalClock::new(5),
            SessionId::generate(),
            KernelEvent::UserMessage { content: "hello".to_string() },
            1,
        );

        assert_eq!(envelope.id.0, 1);
        assert_eq!(envelope.source.0, 42);
        assert_eq!(envelope.timestamp.0, 5);
        assert_eq!(envelope.sequence, 1);
    }

    #[test]
    fn test_child_envelope() {
        let parent = KernelEventEnvelope::new(
            EventId::new(1),
            NodeId::new(1),
            LogicalClock::new(5),
            SessionId::generate(),
            KernelEvent::UserMessage { content: "parent".to_string() },
            1,
        );

        let child = parent.child(
            KernelEvent::Interrupt,
            EventId::new(2),
            2,
        );

        assert_eq!(child.parent_id, Some(EventId::new(1)));
        assert_eq!(child.timestamp.0, 6);
        assert_eq!(child.trace.depth(), 1);
    }

    #[test]
    fn test_envelope_builder() {
        let mut id_gen = 1u64;
        let mut next_id = || { let id = EventId::new(id_gen); id_gen += 1; id };
        
        let mut builder = EnvelopeBuilder::new(
            NodeId::new(1),
            SessionId::generate(),
        );

        let env1 = builder.build_auto(
            KernelEvent::UserMessage { content: "first".to_string() },
            &mut next_id,
        );
        
        let env2 = builder.build_auto(
            KernelEvent::UserMessage { content: "second".to_string() },
            &mut next_id,
        );

        assert_eq!(env1.sequence, 1);
        assert_eq!(env2.sequence, 2);
        assert!(env1.timestamp < env2.timestamp);
    }
}
