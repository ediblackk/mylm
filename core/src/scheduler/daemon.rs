use crate::scheduler::model::{JobAction, JobSchedule, ScheduledJob};
use crate::scheduler::store::JobStore;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::fs;
use std::path::PathBuf;
use std::process;
use tokio::time::{sleep, Duration};

pub struct SchedulerDaemon {
    store: JobStore,
    pid_path: PathBuf,
}

impl SchedulerDaemon {
    pub fn new(store: JobStore) -> Self {
        let pid_path = store.root_dir().join("daemon.pid");
        Self { store, pid_path }
    }

    pub async fn start_loop(&self) -> Result<()> {
        self.write_pid()?;
        println!("Scheduler daemon started (PID: {})", process::id());

        loop {
            if let Err(e) = self.tick().await {
                eprintln!("Error in daemon tick: {:?}", e);
            }
            sleep(Duration::from_secs(60)).await;
        }
    }

    fn write_pid(&self) -> Result<()> {
        fs::write(&self.pid_path, process::id().to_string())
            .with_context(|| format!("Failed to write PID file: {:?}", self.pid_path))
    }

    pub fn cleanup(&self) {
        if self.pid_path.exists() {
            let _ = fs::remove_file(&self.pid_path);
        }
    }

    async fn tick(&self) -> Result<()> {
        let mut jobs_file = self.store.load_jobs()?;
        let now = Utc::now();
        let mut changed = false;

        for job in jobs_file.jobs.iter_mut() {
            if !job.enabled {
                continue;
            }

            if self.is_due(job, now) {
                println!("Executing job: {} ({})", job.name, job.id);
                match self.execute_job(job).await {
                    Ok(_) => {
                        job.last_run_at = Some(now);
                        job.next_run_at = self.calculate_next_run(job, now);
                        job.updated_at = now;
                        changed = true;
                    }
                    Err(e) => {
                        eprintln!("Failed to execute job {}: {:?}", job.name, e);
                    }
                }
            }
        }

        if changed {
            self.store.save_jobs(&jobs_file)?;
        }

        Ok(())
    }

    fn is_due(&self, job: &ScheduledJob, now: DateTime<Utc>) -> bool {
        match job.next_run_at {
            Some(next) => now >= next,
            None => {
                // If never run and no next_run_at, it's due now if we want to start it immediately
                // For v1, let's assume if next_run_at is None, we should compute it or run now.
                // Let's run it now to initialize.
                true
            }
        }
    }

    fn calculate_next_run(&self, job: &ScheduledJob, last_run: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match &job.schedule {
            JobSchedule::Interval(interval) => {
                let duration = self.parse_duration(&interval.every.raw).ok()?;
                Some(last_run + chrono::Duration::from_std(duration).ok()?)
            }
            JobSchedule::Cron(_) => {
                // Cron not supported in v1 without deps
                None
            }
        }
    }

    async fn execute_job(&self, job: &ScheduledJob) -> Result<()> {
        match &job.action {
            JobAction::Shell(shell) => {
                let mut cmd = tokio::process::Command::new(&shell.program);
                cmd.args(&shell.args);
                
                if let Some(cwd) = &shell.cwd {
                    cmd.current_dir(cwd);
                }
                
                for (k, v) in &shell.env {
                    cmd.env(k, v);
                }

                let output = cmd.output().await?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!("Command failed with status {}: {}", output.status, stderr);
                }
                Ok(())
            }
            _ => {
                anyhow::bail!("Action type not supported in v1 daemon");
            }
        }
    }

    fn parse_duration(&self, raw: &str) -> Result<Duration> {
        let (num_str, unit) = raw.split_at(raw.len() - 1);
        let num: u64 = num_str.parse().context("Invalid duration number")?;
        match unit {
            "s" => Ok(Duration::from_secs(num)),
            "m" => Ok(Duration::from_secs(num * 60)),
            "h" => Ok(Duration::from_secs(num * 3600)),
            "d" => Ok(Duration::from_secs(num * 86400)),
            _ => anyhow::bail!("Invalid duration unit: {}", unit),
        }
    }
}
