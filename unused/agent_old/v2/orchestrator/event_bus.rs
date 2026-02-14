use tokio::sync::broadcast;
use crate::llm::TokenUsage;

/// Events that the core agent emits during execution.
/// These events are published to the EventBus and consumed by subscribers (e.g., terminal UI).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent {
    /// Agent is thinking/processing
    AgentThinking { model: String },
    /// Agent is about to execute a tool
    ToolExecuting { tool: String, args: String },
    /// A tool is awaiting user approval
    ToolAwaitingApproval { tool: String, args: String, approval_id: String },
    /// A worker job was spawned (via DelegateTool)
    WorkerSpawned { job_id: String, description: String },
    /// A worker job completed
    WorkerCompleted { job_id: String, result: String },
    /// A worker job stalled (exceeded action budget without final answer)
    WorkerStalled { job_id: String, reason: String },
    /// Worker status update (per-job status message)
    WorkerStatusUpdate { job_id: String, message: String },
    /// Worker metrics update (real-time token usage, progress)
    WorkerMetricsUpdate { job_id: String, prompt_tokens: u32, completion_tokens: u32, total_tokens: u32, context_tokens: usize },
    /// PaCoRe reasoning progress
    PaCoReProgress { round: usize, total: usize },
    /// Agent sent a response message
    AgentResponse { content: String, usage: TokenUsage },
    /// Status update (informational message)
    StatusUpdate { message: String },
    /// Internal observation data (e.g., tool output, terminal screen)
    InternalObservation { data: Vec<u8> },
    /// Suggest a command to the user
    SuggestCommand { command: String },
}

/// EventBus provides a publish-subscribe mechanism for core events.
/// Uses a broadcast channel so multiple subscribers can receive events.
/// Events are delivered asynchronously; if the channel buffer is full, oldest events are dropped.
#[derive(Debug, Clone)]
pub struct EventBus {
    tx: broadcast::Sender<CoreEvent>,
}

impl EventBus {
    /// Create a new EventBus with a default buffer size (100 events).
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self { tx }
    }

    /// Create a new EventBus with a custom buffer size.
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Subscribe to events. Returns a receiver that will receive all published events.
    /// Each subscriber gets its own receiver; multiple subscribers are supported.
    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.tx.subscribe()
    }

    /// Publish an event to all subscribers.
    /// Returns the number of subscribers that received the event.
    /// If no subscribers are available, the event is dropped.
    pub fn publish(&self, event: CoreEvent) -> usize {
        self.tx.send(event).unwrap_or(0)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
