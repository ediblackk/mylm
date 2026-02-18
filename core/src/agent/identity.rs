//! Agent Identity
//!
//! Unified agent identification for the multi-agent system.
//!
//! Every agent (Main or Worker) has a unique, immutable identity that is
//! runtime-provided and unspoofable. The LLM cannot forge or modify this identity.
//!
//! # Key Properties
//!
//! - **Runtime-enforced**: Set by Runtime, not from LLM output
//! - **Unique**: instance_id is a UUID, globally unique
//! - **Typed**: AgentType discriminates Main from Worker at compile time
//! - **Serializable**: Can be persisted and transmitted
//!
//! # Usage
//!
//! ```rust
//! use mylm_core::agent::identity::{AgentId, AgentType};
//!
//! // Main agent
//! let main = AgentId::main();
//!
//! // Worker agent
//! let worker = AgentId::worker("refactor-task-7");
//!
//! // Check type
//! if main.is_main() {
//!     // Has full authority
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique agent identifier.
///
/// Composed of an agent type (Main or Worker) and a unique instance identifier.
/// The instance_id is a UUID string for global uniqueness.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId {
    /// The type of agent (Main or Worker)
    pub agent_type: AgentType,

    /// Unique instance identifier (UUID)
    pub instance_id: String,
}

/// Agent type classification.
///
/// Distinguishes between the main orchestrator agent and worker sub-agents.
/// Workers are identified by their task ID for traceability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentType {
    /// Main orchestrator agent.
    ///
    /// There is exactly one Main agent per session hierarchy.
    /// The Main has full authority and can:
    /// - Spawn and manage workers
    /// - Approve escalated worker actions
    /// - Access all tools without restriction
    Main,

    /// Worker sub-agent.
    ///
    /// Workers are spawned by Main for parallel task execution.
    /// Workers have restricted capabilities and must escalate:
    /// - Write/shell tools require Main approval
    /// - Limited context window
    /// - Cannot spawn sub-workers
    Worker(String),
}

impl AgentId {
    /// Create a new Main agent ID.
    ///
    /// Generates a fresh UUID for the instance identifier.
    ///
    /// # Example
    ///
    /// ```
    /// use mylm_core::agent::identity::AgentId;
    ///
    /// let main = AgentId::main();
    /// assert!(main.is_main());
    /// ```
    pub fn main() -> Self {
        Self {
            agent_type: AgentType::Main,
            instance_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Create a new Worker agent ID.
    ///
    /// Takes a task identifier for traceability. The task_id should be
    /// descriptive and unique within the current session.
    ///
    /// # Arguments
    ///
    /// * `task_id` - A unique identifier for this worker's task
    ///
    /// # Example
    ///
    /// ```
    /// use mylm_core::agent::identity::AgentId;
    ///
    /// let worker = AgentId::worker("refactor-models-42");
    /// assert!(worker.is_worker());
    /// ```
    pub fn worker(task_id: impl Into<String>) -> Self {
        Self {
            agent_type: AgentType::Worker(task_id.into()),
            instance_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Check if this is the Main agent.
    ///
    /// # Example
    ///
    /// ```
    /// use mylm_core::agent::identity::{AgentId, AgentType};
    ///
    /// let main = AgentId::main();
    /// assert!(main.is_main());
    ///
    /// let worker = AgentId::worker("test");
    /// assert!(!worker.is_main());
    /// ```
    pub fn is_main(&self) -> bool {
        matches!(self.agent_type, AgentType::Main)
    }

    /// Check if this is a Worker agent.
    ///
    /// # Example
    ///
    /// ```
    /// use mylm_core::agent::identity::AgentId;
    ///
    /// let worker = AgentId::worker("test-task");
    /// assert!(worker.is_worker());
    ///
    /// let main = AgentId::main();
    /// assert!(!main.is_worker());
    /// ```
    pub fn is_worker(&self) -> bool {
        matches!(self.agent_type, AgentType::Worker(_))
    }

    /// Get the task ID if this is a Worker.
    ///
    /// Returns `Some(task_id)` for workers, `None` for Main.
    ///
    /// # Example
    ///
    /// ```
    /// use mylm_core::agent::identity::AgentId;
    ///
    /// let worker = AgentId::worker("my-task");
    /// assert_eq!(worker.task_id(), Some("my-task".to_string()));
    ///
    /// let main = AgentId::main();
    /// assert_eq!(main.task_id(), None);
    /// ```
    pub fn task_id(&self) -> Option<String> {
        match &self.agent_type {
            AgentType::Worker(task_id) => Some(task_id.clone()),
            AgentType::Main => None,
        }
    }

    /// Get a short display string for logging.
    ///
    /// Format: "MAIN" or "WORKER-{task_id}"
    ///
    /// # Example
    ///
    /// ```
    /// use mylm_core::agent::identity::AgentId;
    ///
    /// let main = AgentId::main();
    /// assert_eq!(main.short_name(), "MAIN");
    ///
    /// let worker = AgentId::worker("refactor");
    /// assert_eq!(worker.short_name(), "WORKER-refactor");
    /// ```
    pub fn short_name(&self) -> String {
        match &self.agent_type {
            AgentType::Main => "MAIN".to_string(),
            AgentType::Worker(task_id) => format!("WORKER-{}", task_id),
        }
    }
}

impl fmt::Display for AgentId {
    /// Full display format including instance ID.
    ///
    /// Format: "MAIN:{uuid}" or "WORKER-{task_id}:{uuid}"
    ///
    /// Use `short_name()` for concise logging.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.agent_type {
            AgentType::Main => {
                write!(f, "MAIN:{}", self.instance_id)
            }
            AgentType::Worker(task_id) => {
                write!(f, "WORKER-{}:{}", task_id, self.instance_id)
            }
        }
    }
}

impl fmt::Display for AgentType {
    /// Display just the type without instance ID.
    ///
    /// Format: "Main" or "Worker({task_id})"
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentType::Main => write!(f, "Main"),
            AgentType::Worker(task_id) => write!(f, "Worker({})", task_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_main_creation() {
        let main = AgentId::main();
        assert!(main.is_main());
        assert!(!main.is_worker());
        assert_eq!(main.task_id(), None);
        assert_eq!(main.agent_type, AgentType::Main);
        assert!(!main.instance_id.is_empty());
    }

    #[test]
    fn test_worker_creation() {
        let worker = AgentId::worker("test-task");
        assert!(worker.is_worker());
        assert!(!worker.is_main());
        assert_eq!(worker.task_id(), Some("test-task".to_string()));
        assert!(!worker.instance_id.is_empty());
    }

    #[test]
    fn test_worker_creation_string() {
        let task_id = String::from("owned-task");
        let worker = AgentId::worker(task_id);
        assert!(worker.is_worker());
    }

    #[test]
    fn test_short_name_main() {
        let main = AgentId::main();
        assert_eq!(main.short_name(), "MAIN");
    }

    #[test]
    fn test_short_name_worker() {
        let worker = AgentId::worker("refactor");
        assert_eq!(worker.short_name(), "WORKER-refactor");
    }

    #[test]
    fn test_display_main() {
        let main = AgentId::main();
        let display = format!("{}", main);
        assert!(display.starts_with("MAIN:"));
        assert!(display.len() > 40); // "MAIN:" + UUID (36 chars)
    }

    #[test]
    fn test_display_worker() {
        let worker = AgentId::worker("my-task");
        let display = format!("{}", worker);
        assert!(display.starts_with("WORKER-my-task:"));
    }

    #[test]
    fn test_display_agent_type() {
        assert_eq!(format!("{}", AgentType::Main), "Main");
        assert_eq!(
            format!("{}", AgentType::Worker("test".to_string())),
            "Worker(test)"
        );
    }

    #[test]
    fn test_uniqueness() {
        // Multiple creations should have different instance IDs
        let main1 = AgentId::main();
        let main2 = AgentId::main();
        assert_ne!(main1.instance_id, main2.instance_id);

        let worker1 = AgentId::worker("same-task");
        let worker2 = AgentId::worker("same-task");
        assert_ne!(worker1.instance_id, worker2.instance_id);
    }

    #[test]
    fn test_equality() {
        let main = AgentId::main();
        let main_clone = main.clone();
        assert_eq!(main, main_clone);

        let worker = AgentId::worker("task");
        let worker_clone = worker.clone();
        assert_eq!(worker, worker_clone);

        // Different types are never equal
        assert_ne!(AgentId::main(), AgentId::worker("task"));

        // Different instance IDs are never equal
        assert_ne!(AgentId::main(), AgentId::main());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let main = AgentId::main();
        let json = serde_json::to_string(&main).expect("serialization failed");
        let decoded: AgentId = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(main, decoded);

        let worker = AgentId::worker("test-task");
        let json = serde_json::to_string(&worker).expect("serialization failed");
        let decoded: AgentId = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(worker, decoded);
    }

    #[test]
    fn test_serialization_format() {
        let worker = AgentId::worker("my-task");
        let json = serde_json::to_string(&worker).expect("serialization failed");

        // Should contain both fields
        assert!(json.contains("agent_type"));
        assert!(json.contains("instance_id"));
        assert!(json.contains("my-task"));
    }

    #[test]
    fn test_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        let main = AgentId::main();
        set.insert(main.clone());

        assert!(set.contains(&main));
        assert!(!set.contains(&AgentId::main())); // Different instance_id
    }

    #[test]
    fn test_worker_task_id_with_special_chars() {
        // Task IDs can contain various characters
        let worker = AgentId::worker("task-with-dash");
        assert_eq!(worker.task_id(), Some("task-with-dash".to_string()));

        let worker2 = AgentId::worker("task.with.dots");
        assert_eq!(worker2.task_id(), Some("task.with.dots".to_string()));
    }
}
