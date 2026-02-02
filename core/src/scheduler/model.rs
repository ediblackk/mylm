use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

pub type JobId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: JobId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub schedule: JobSchedule,
    pub action: JobAction,
    #[serde(default = "ScheduledJob::default_enabled")]
    pub enabled: bool,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Scheduler can compute these; persisted for convenience.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<DateTime<Utc>>,

    #[serde(default)]
    pub policy: JobPolicy,
}

impl ScheduledJob {
    fn default_enabled() -> bool {
        true
    }

    pub fn new_now(name: impl Into<String>, schedule: JobSchedule, action: JobAction) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            schedule,
            action,
            enabled: true,
            created_at: now,
            updated_at: now,
            last_run_at: None,
            next_run_at: None,
            policy: JobPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum JobSchedule {
    Cron(CronSchedule),
    Interval(IntervalSchedule),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSchedule {
    pub expression: String,
    #[serde(default)]
    pub timezone: JobTimezone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntervalSchedule {
    pub every: DurationSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<DurationSpec>,
    #[serde(default)]
    pub timezone: JobTimezone,
}

/// For Phase 1, keep duration specs as user-provided strings (e.g. "5m", "2h").
/// Parsing/validation happens in the scheduler module in a later phase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DurationSpec {
    pub raw: String,
}

impl From<&str> for DurationSpec {
    fn from(value: &str) -> Self {
        Self {
            raw: value.to_string(),
        }
    }
}

impl From<String> for DurationSpec {
    fn from(value: String) -> Self {
        Self { raw: value }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobTimezone {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "utc")]
    Utc,
}

impl Default for JobTimezone {
    fn default() -> Self {
        Self::Local
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum JobAction {
    Shell(ShellAction),
    AgentTask(AgentTaskAction),
    Delegate(DelegateAction),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellAction {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTaskAction {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default)]
    pub allow_execution: bool,
    #[serde(default)]
    pub initial_context: AgentContextSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    pub task: AgentTaskAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentContextSpec {
    None,
    SystemInfoOnly,
    TerminalSnapshot,
}

impl Default for AgentContextSpec {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobPolicy {
    #[serde(default = "JobPolicy::default_max_concurrent_runs")]
    pub max_concurrent_runs: u32,
    #[serde(default)]
    pub overlap: OverlapPolicy,
    #[serde(default)]
    pub misfire: MisfirePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter_secs: Option<u32>,
}

impl Default for JobPolicy {
    fn default() -> Self {
        Self {
            max_concurrent_runs: Self::default_max_concurrent_runs(),
            overlap: OverlapPolicy::default(),
            misfire: MisfirePolicy::default(),
            jitter_secs: None,
        }
    }
}

impl JobPolicy {
    fn default_max_concurrent_runs() -> u32 {
        1
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OverlapPolicy {
    Skip,
    Queue,
    Allow,
}

impl Default for OverlapPolicy {
    fn default() -> Self {
        Self::Skip
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MisfirePolicy {
    Skip,
    CatchUp(u32),
}

impl Default for MisfirePolicy {
    fn default() -> Self {
        Self::Skip
    }
}

