use anyhow::{Context, Result};
use mylm_core::scheduler::JobStore;
use mylm_core::scheduler::SchedulerDaemon;
use std::fs;
use std::process::Command;

pub async fn handle_daemon_run() -> Result<()> {
    let store = JobStore::new()?;
    let daemon = SchedulerDaemon::new(store);
    
    tokio::select! {
        res = daemon.start_loop() => {
            if let Err(e) = res {
                eprintln!("Daemon error: {:?}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down daemon...");
        }
    }

    daemon.cleanup();
    Ok(())
}

pub fn handle_daemon_start() -> Result<()> {
    let exe = std::env::current_exe()?;
    let store = JobStore::new()?;
    let pid_path = store.root_dir().join("daemon.pid");

    if pid_path.exists() {
        let pid = fs::read_to_string(&pid_path)?;
        println!("Daemon already running (PID: {})", pid);
        return Ok(());
    }

    // Spawn detached
    Command::new(exe)
        .arg("daemon")
        .arg("run")
        .spawn()
        .context("Failed to spawn daemon process")?;

    println!("Daemon started in background.");
    Ok(())
}

pub fn handle_daemon_stop() -> Result<()> {
    let store = JobStore::new()?;
    let pid_path = store.root_dir().join("daemon.pid");

    if !pid_path.exists() {
        println!("Daemon is not running.");
        return Ok(());
    }

    let pid_str = fs::read_to_string(&pid_path)?;
    let pid: i32 = pid_str.parse().context("Invalid PID in file")?;

    println!("Stopping daemon (PID: {})...", pid);

    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("kill")
            .arg(pid.to_string())
            .status()
            .context("Failed to execute kill command")?;
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("taskkill")
            .arg("/F")
            .arg("/PID")
            .arg(pid.to_string())
            .status()
            .context("Failed to execute taskkill command")?;
    }

    // Cleanup PID file if the process didn't (best effort)
    let _ = fs::remove_file(pid_path);

    Ok(())
}
