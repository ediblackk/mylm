use crate::scheduler::model::ScheduledJob;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobsFile {
    pub schema_version: u32,
    pub jobs: Vec<ScheduledJob>,
}

impl Default for JobsFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            jobs: Vec::new(),
        }
    }
}

pub struct JobStore {
    root_dir: PathBuf,
    jobs_path: PathBuf,
}

impl JobStore {
    pub fn new() -> Result<Self> {
        let root_dir = dirs::data_dir()
            .context("Could not find data directory")?
            .join("mylm")
            .join("scheduled_jobs");
        Self::new_in(root_dir)
    }

    pub fn new_in(root_dir: PathBuf) -> Result<Self> {
        let jobs_path = root_dir.join("jobs.json");
        Ok(Self { root_dir, jobs_path })
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn jobs_path(&self) -> &Path {
        &self.jobs_path
    }

    pub fn load_jobs(&self) -> Result<JobsFile> {
        if !self.jobs_path.exists() {
            return Ok(JobsFile::default());
        }

        let content = fs::read_to_string(&self.jobs_path)
            .with_context(|| format!("Failed to read jobs file: {:?}", self.jobs_path))?;

        if content.trim().is_empty() {
            return Ok(JobsFile::default());
        }

        let parsed: JobsFile = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse jobs file: {:?}", self.jobs_path))?;

        Ok(parsed)
    }

    pub fn save_jobs(&self, jobs_file: &JobsFile) -> Result<()> {
        fs::create_dir_all(&self.root_dir)
            .with_context(|| format!("Failed to create scheduled jobs dir: {:?}", self.root_dir))?;

        let content = serde_json::to_string_pretty(jobs_file)
            .context("Failed to serialize jobs file")?;

        atomic_write(&self.jobs_path, content.as_bytes()).with_context(|| {
            format!(
                "Failed to atomically write jobs file: {:?}",
                self.jobs_path
            )
        })?;

        Ok(())
    }
}

fn atomic_write(dest: &Path, bytes: &[u8]) -> Result<()> {
    let parent = dest
        .parent()
        .context("Destination path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create parent dir: {:?}", parent))?;

    let tmp = dest.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));

    fs::write(&tmp, bytes).with_context(|| format!("Failed to write temp file: {:?}", tmp))?;

    // Best-effort cleanup on failure.
    if let Err(rename_err) = fs::rename(&tmp, dest) {
        let _ = fs::remove_file(&tmp);
        return Err(rename_err).context("Failed to rename temp file into place");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{JobStore, JobsFile};
    use crate::scheduler::model::{
        DurationSpec, IntervalSchedule, JobAction, JobSchedule, JobTimezone, ScheduledJob, ShellAction,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn unique_temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("mylm-test-scheduler-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn jobs_round_trip_save_load() {
        let dir = unique_temp_dir();
        let store = JobStore::new_in(dir.clone()).expect("store");

        let job = ScheduledJob::new_now(
            "defensive-check",
            JobSchedule::Interval(IntervalSchedule {
                every: DurationSpec::from("5m"),
                offset: None,
                timezone: JobTimezone::Local,
            }),
            JobAction::Shell(ShellAction {
                program: "bash".to_string(),
                args: vec!["-lc".to_string(), "echo hello".to_string()],
                cwd: None,
                env: BTreeMap::new(),
                timeout_secs: Some(30),
            }),
        );

        let jf = JobsFile {
            schema_version: 1,
            jobs: vec![job.clone()],
        };

        store.save_jobs(&jf).expect("save");
        let loaded = store.load_jobs().expect("load");
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.jobs.len(), 1);
        assert_eq!(loaded.jobs[0].id, job.id);
        assert_eq!(loaded.jobs[0].name, job.name);

        // Cleanup
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let dir = unique_temp_dir();
        let store = JobStore::new_in(dir.clone()).expect("store");
        let loaded = store.load_jobs().expect("load");
        assert_eq!(loaded.schema_version, 1);
        assert!(loaded.jobs.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}

