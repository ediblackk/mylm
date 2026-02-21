//! Internal state for Commonbox

use crate::agent::identity::AgentId;
use crate::agent::runtime::orchestrator::commonbox::agent::CommonboxEntry;
use crate::agent::runtime::orchestrator::commonbox::coordination::CoordinationBoard;
use crate::agent::runtime::orchestrator::commonbox::id::JobId;
use crate::agent::runtime::orchestrator::commonbox::job::Job;
use std::collections::HashMap;

/// Internal state protected by single RwLock.
#[derive(Debug)]
pub struct CommonboxState {
    /// Agent entries
    pub entries: HashMap<AgentId, CommonboxEntry>,
    /// Job tracking
    pub jobs: HashMap<JobId, Job>,
    /// Agent to current job mapping
    pub agent_to_job: HashMap<AgentId, JobId>,
    /// Coordination board for inter-agent communication
    pub coordination: CoordinationBoard,
    /// Cached LLM snapshot (regenerated on every write)
    pub llm_snapshot: String,
}

impl CommonboxState {
    /// Create empty state.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            jobs: HashMap::new(),
            agent_to_job: HashMap::new(),
            coordination: CoordinationBoard::default(),
            llm_snapshot: String::new(),
        }
    }

    /// Regenerate semantic snapshot for LLM consumption.
    ///
    /// Format per entry: "{short_name} {s:status,h:health,cm:comment}"
    pub fn regenerate_snapshot(&mut self) {
        let mut lines: Vec<String> = Vec::new();

        // Sort by agent type for deterministic output
        let mut agents: Vec<_> = self.entries.iter().collect();
        agents.sort_by_key(|(id, _)| {
            if id.is_main() {
                (0, id.instance_id.clone())
            } else {
                (1, id.instance_id.clone())
            }
        });

        for (agent_id, entry) in agents {
            let health = Self::classify_health(entry);
            let line = format!(
                "{} {{s:{},h:{},cm:{}}}",
                agent_id.short_name(),
                entry.status.abbrev(),
                health,
                entry.comment
            );
            lines.push(line);
        }

        self.llm_snapshot = lines.join("\n");
    }

    /// Classify agent health based on metrics.
    fn classify_health(entry: &CommonboxEntry) -> &'static str {
        if entry.step_count >= entry.max_steps {
            "stalled"
        } else if entry.ctx_tokens > 15000 {
            "heavy"
        } else {
            "good"
        }
    }
}

impl Default for CommonboxState {
    fn default() -> Self {
        Self::new()
    }
}
