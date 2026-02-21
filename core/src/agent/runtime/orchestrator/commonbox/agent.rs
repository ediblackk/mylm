//! Agent status and entry types

use crate::agent::identity::AgentId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Agent status in the lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    /// Agent is idle, waiting for work
    Idle,
    /// Agent is actively processing
    Processing,
    /// Agent has stalled (needs Main resolution)
    Stalled,
    /// Agent has completed its task
    Completed,
    /// Agent has failed
    Failed,
}

impl AgentStatus {
    /// Get abbreviated form for LLM snapshot.
    pub fn abbrev(&self) -> &'static str {
        match self {
            AgentStatus::Idle => "Idle",
            AgentStatus::Processing => "Processing",
            AgentStatus::Stalled => "Stalled",
            AgentStatus::Completed => "Completed",
            AgentStatus::Failed => "Failed",
        }
    }

    /// Check if this is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, AgentStatus::Completed | AgentStatus::Failed)
    }
}

/// Entry in the Commonbox for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonboxEntry {
    /// The agent's unique identifier
    pub agent_id: AgentId,
    /// Current status in lifecycle
    pub status: AgentStatus,
    /// Current context token count (raw)
    pub ctx_tokens: usize,
    /// Current step count
    pub step_count: usize,
    /// Maximum allowed steps before stall
    pub max_steps: usize,
    /// Semantic comment for LLM
    pub comment: String,
    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
}

impl CommonboxEntry {
    /// Create a new entry for an agent.
    pub fn new(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            status: AgentStatus::Idle,
            ctx_tokens: 0,
            step_count: 0,
            max_steps: 20, // Default from plan
            comment: "Initialized".to_string(),
            last_updated: Utc::now(),
        }
    }

    /// Apply updates to this entry.
    pub fn apply(&mut self, updates: EntryUpdate) {
        if let Some(status) = updates.status {
            self.status = status;
        }
        if let Some(ctx_tokens) = updates.ctx_tokens {
            self.ctx_tokens = ctx_tokens;
        }
        if let Some(step_count) = updates.step_count {
            self.step_count = step_count;
        }
        if let Some(max_steps) = updates.max_steps {
            self.max_steps = max_steps;
        }
        if let Some(comment) = updates.comment {
            self.comment = comment;
        }
    }
}

/// Updates to apply to a CommonboxEntry.
#[derive(Debug, Clone)]
pub struct EntryUpdate {
    pub agent_id: AgentId,
    pub status: Option<AgentStatus>,
    pub ctx_tokens: Option<usize>,
    pub step_count: Option<usize>,
    pub max_steps: Option<usize>,
    pub comment: Option<String>,
}

impl Default for EntryUpdate {
    fn default() -> Self {
        // Note: agent_id must be set explicitly via for_agent()
        Self {
            agent_id: AgentId::main(), // Placeholder, should be overridden
            status: None,
            ctx_tokens: None,
            step_count: None,
            max_steps: None,
            comment: None,
        }
    }
}

impl EntryUpdate {
    /// Create an update for the given agent.
    pub fn for_agent(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            ..Default::default()
        }
    }

    /// Set status.
    pub fn with_status(mut self, status: AgentStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Set context tokens.
    pub fn with_ctx_tokens(mut self, tokens: usize) -> Self {
        self.ctx_tokens = Some(tokens);
        self
    }

    /// Set step count.
    pub fn with_step_count(mut self, count: usize) -> Self {
        self.step_count = Some(count);
        self
    }

    /// Set max steps.
    pub fn with_max_steps(mut self, max: usize) -> Self {
        self.max_steps = Some(max);
        self
    }

    /// Set comment.
    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }
}
