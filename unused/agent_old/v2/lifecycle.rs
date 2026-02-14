//! Agent lifecycle management (reset, state management)
use crate::llm::chat::{ChatMessage, MessageRole};
use crate::llm::TokenUsage;
use crate::agent_old::prompt::PromptBuilder;
use crate::agent_old::tools::StructuredScratchpad;
use std::sync::RwLock;

/// Manages agent lifecycle: reset, state management.
#[derive(Default)]
pub struct LifecycleManager {
    pub history: Vec<ChatMessage>,
    pub iteration_count: usize,
    pub total_usage: TokenUsage,
    pub pending_decision: Option<crate::agent_old::v2::protocol::AgentDecision>,
    pub last_tool_call: Option<(String, String)>,
    pub repetition_count: usize,
    pub pending_tool_call_id: Option<String>,
    pub parse_failure_count: usize,
    pub max_steps: usize,
    pub budget: usize,
}

impl LifecycleManager {
    /// Reset the agent's state for a new task.
    pub fn reset(&mut self, history: Vec<ChatMessage>) {
        self.history = history;
        self.iteration_count = 0;
        self.total_usage = TokenUsage::default();
        self.pending_decision = None;
        self.last_tool_call = None;
        self.repetition_count = 0;
        self.parse_failure_count = 0;
        self.pending_tool_call_id = None;
        self.max_steps = self.budget;
    }

    /// Increase the step budget by a specified amount.
    pub fn increase_budget(&mut self, additional_steps: usize) {
        self.max_steps += additional_steps;
    }

    /// Get current budget usage statistics.
    pub fn budget_stats(&self) -> (usize, usize, usize) {
        (self.iteration_count, self.max_steps, self.budget)
    }

    /// Check if the agent has a pending decision to be returned.
    pub fn has_pending_decision(&self) -> bool {
        self.pending_decision.is_some()
    }
}

/// Reset helper that ensures system prompt is present.
pub async fn reset_with_system_prompt(
    history: &mut Vec<ChatMessage>,
    scratchpad: &RwLock<StructuredScratchpad>,
    prompt_builder: &mut PromptBuilder,
) {
    // Ensure system prompt is present with capability awareness
    if history.is_empty() || history[0].role != MessageRole::System {
        let scratchpad_guard = scratchpad.read().unwrap_or_else(|e| e.into_inner());
        let scratchpad_content = scratchpad_guard.to_string();
        history.insert(0, ChatMessage::system(prompt_builder.build(&scratchpad_content)));
    }
}
