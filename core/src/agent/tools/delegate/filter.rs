//! Event filter for worker output - implements "selection before amplification"
//!
//! This filter prevents event flooding by:
//! 1. Deduplicating Status events within a time window
//! 2. Preserving semantic event types (not flattening to Status)
//! 3. Dropping diagnostic/noise events

use crate::agent::runtime::session::OutputEvent;
use crate::agent::types::events::WorkerId;

/// Event filter decision for worker forwarding
#[derive(Debug, Clone)]
pub enum FilterDecision {
    /// Pass event through unchanged
    Forward(OutputEvent),
    /// Transform event (preserve semantics)
    Transform(OutputEvent),
    /// Drop event with reason
    Drop(&'static str),
}

/// Worker event filter - implements "selection before amplification"
/// 
/// This filter prevents the 1.2M event flood by:
/// 1. Deduplicating Status events within a time window
/// 2. Preserving semantic event types (not flattening to Status)
/// 3. Dropping diagnostic/noise events
pub struct WorkerEventFilter {
    worker_id: WorkerId,
    last_status: Option<String>,
    last_status_time: std::time::Instant,
    status_dedup_window: std::time::Duration,
}

impl WorkerEventFilter {
    /// Create a new filter for a specific worker
    pub fn new(worker_id: WorkerId) -> Self {
        Self {
            worker_id,
            last_status: None,
            last_status_time: std::time::Instant::now(),
            status_dedup_window: std::time::Duration::from_millis(100),
        }
    }

    /// Decide whether to forward an event
    /// 
    /// Implements the selection logic:
    /// - Real actions (ToolExecuting, ToolCompleted) always pass
    /// - Status events are deduplicated within 100ms window
    /// - Semantic events are preserved (not flattened to Status)
    /// - Noise events are dropped
    pub fn filter(&mut self, event: OutputEvent) -> FilterDecision {
        match &event {
            // ALWAYS FORWARD: Real actions with payload
            // These represent actual work being done
            OutputEvent::ToolExecuting { .. } => {
                FilterDecision::Forward(event)
            }
            OutputEvent::ToolCompleted { .. } => {
                FilterDecision::Forward(event)
            }
            OutputEvent::WorkerCompleted { .. } => {
                FilterDecision::Forward(event)
            }
            OutputEvent::Error { .. } => {
                FilterDecision::Forward(event)
            }
            
            // CONDITIONAL: Status with deduplication
            // This is the primary spam source - filter aggressively
            OutputEvent::Status { message } => {
                // Deduplicate: Same message = drop
                if self.last_status.as_ref() == Some(message) {
                    return FilterDecision::Drop("duplicate_status");
                }
                
                // Rate limit: Too frequent = drop
                if self.last_status_time.elapsed() < self.status_dedup_window {
                    return FilterDecision::Drop("rate_limited");
                }
                
                self.last_status = Some(message.clone());
                self.last_status_time = std::time::Instant::now();
                FilterDecision::Forward(event)
            }
            
            // PRESERVE SEMANTICS: Thinking events
            // Don't flatten to Status - keep the semantic type
            OutputEvent::Thinking { intent_id: _ } => {
                // Forward Thinking as-is (with worker_id in message prefix from forwarder)
                FilterDecision::Forward(event)
            }
            
            // COALESCE: Response chunks
            // Let the forwarder batch these - we'll emit periodic updates
            OutputEvent::ResponseChunk { content } => {
                if content.is_empty() {
                    FilterDecision::Drop("empty_chunk")
                } else {
                    FilterDecision::Forward(event)
                }
            }
            
            // DROP: Noise/diagnostic events
            OutputEvent::ResponseComplete => {
                FilterDecision::Drop("noise")
            }
            
            // PASS THROUGH: Worker events (already have worker_id)
            OutputEvent::WorkerSpawned { .. } => FilterDecision::Forward(event),
            OutputEvent::WorkerFailed { .. } => FilterDecision::Forward(event),
            
            // PASS THROUGH: Approval (needs Main attention)
            OutputEvent::ApprovalRequested { .. } => FilterDecision::Forward(event),
            
            // PASS THROUGH: Context pruning
            OutputEvent::ContextPruned { .. } => FilterDecision::Forward(event),
            
            // DEFAULT: Pass through unknown variants
            _ => FilterDecision::Forward(event),
        }
    }
}
