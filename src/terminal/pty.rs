use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize, MasterPty};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::mpsc;

pub struct PtyManager {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
}

impl Clone for PtyManager {
    fn clone(&self) -> Self {
        Self {
            writer: self.writer.clone(),
            master: self.master.clone(),
        }
    }
}

impl PtyManager {
    pub fn new(tx: mpsc::UnboundedSender<Vec<u8>>, cwd: Option<std::path::PathBuf>) -> Result<Self> {
        mylm_core::debug_log!("PtyManager::new: initializing PTY with cwd: {:?}", cwd);
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }).context("Failed to open PTY")?;
        mylm_core::debug_log!("PtyManager::new: PTY opened successfully");

        let shell = if cfg!(target_os = "windows") {
            "powershell.exe"
        } else {
            "bash"
        };

        let mut cmd = CommandBuilder::new(shell);
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }

        #[cfg(unix)]
        {
            cmd.env("HISTFILE", "/dev/null");
            cmd.env("TERM", "xterm-256color");
            cmd.env("PS1", "\\u@\\h:\\w$ ");
            cmd.arg("--norc");
            cmd.arg("--noprofile");
        }
        #[cfg(windows)]
        {
            cmd.env("TERM", "dumb");
            cmd.args(["-NoProfile", "-NonInteractive"]);
        }

        let _child = pair.slave.spawn_command(cmd).context("Failed to spawn PTY command")?;
        mylm_core::debug_log!("PtyManager::new: PTY command spawned successfully");

        // Move the reader to a separate thread
        let mut reader = pair.master.try_clone_reader().context("Failed to clone PTY reader")?;
        thread::spawn(move || {
            mylm_core::debug_log!("PTY reader thread started");
            let mut buffer = [0u8; 1024];
            loop {
                match std::io::Read::read(&mut reader, &mut buffer) {
                    Ok(n) => {
                        if n == 0 {
                            mylm_core::debug_log!("PTY reader: EOF reached");
                            break;
                        }
                        if tx.send(buffer[..n].to_vec()).is_err() {
                            mylm_core::debug_log!("PTY reader: Channel closed, exiting thread");
                            break;
                        }
                    }
                    Err(e) => {
                        mylm_core::error_log!("PTY reader error: {}", e);
                        break;
                    }
                }
            }
        });

        let writer = Arc::new(Mutex::new(pair.master.take_writer().context("Failed to take PTY writer")?));
        let master = Arc::new(Mutex::new(pair.master));

        // Redundant init log removed - mylm_core::info_log!("PtyManager::new: PTY initialized successfully");
        Ok(Self { writer, master })
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        mylm_core::debug_log!("PtyManager::resize: {}x{}", rows, cols);
        let master = self.master.lock().map_err(|_| anyhow::anyhow!("Failed to lock PTY master"))?;
        master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }).context("Failed to resize PTY")?;
        mylm_core::debug_log!("PtyManager::resize: successful");
        Ok(())
    }

    pub fn write_all(&self, data: &[u8]) -> Result<()> {
        mylm_core::trace_log!("PtyManager::write_all: writing {} bytes", data.len());
        let mut writer = self.writer.lock().map_err(|_| anyhow::anyhow!("Failed to lock PTY writer"))?;
        writer.write_all(data).context("Failed to write to PTY")?;
        writer.flush().context("Failed to flush PTY writer")?;
        mylm_core::trace_log!("PtyManager::write_all: write successful");
        Ok(())
    }
}

pub fn spawn_pty(cwd: Option<std::path::PathBuf>) -> Result<(PtyManager, mpsc::UnboundedReceiver<Vec<u8>>)> {
    mylm_core::debug_log!("spawn_pty: creating PTY");
    let (tx, rx) = mpsc::unbounded_channel();
    let manager = PtyManager::new(tx, cwd)?;
    // Redundant init log removed - mylm_core::info_log!("spawn_pty: PTY spawned successfully");
    Ok((manager, rx))
}
