//! PTY (Pseudo-Terminal) Management Module
//!
//! Handles spawning and managing pseudo-terminals for the TUI.
//! Uses portable-pty for cross-platform PTY support.

use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, PtySize, MasterPty};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::mpsc;

/// PTY Manager - handles a pseudo-terminal session
pub struct PtyManager {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
}

impl PtyManager {
    /// Create a new PTY manager with the given working directory
    pub fn new(tx: mpsc::UnboundedSender<Vec<u8>>, cwd: Option<PathBuf>) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell = if cfg!(target_os = "windows") {
            "powershell.exe"
        } else {
            "bash"
        };

        let mut cmd = CommandBuilder::new(shell);
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        let _child = pair.slave.spawn_command(cmd)?;

        // Move the reader to a separate thread
        let mut reader = pair.master.try_clone_reader()?;
        thread::spawn(move || {
            let mut buffer = [0u8; 1024];
            while let Ok(n) = std::io::Read::read(&mut reader, &mut buffer) {
                if n == 0 {
                    break;
                }
                if tx.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });

        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let master = Arc::new(Mutex::new(pair.master));

        Ok(Self { writer, master })
    }

    /// Resize the PTY to new dimensions
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let master = self.master.lock().map_err(|_| anyhow::anyhow!("Failed to lock PTY master"))?;
        master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// Write data to the PTY
    pub fn write_all(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().map_err(|_| anyhow::anyhow!("Failed to lock PTY writer"))?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }
}

/// Spawn a new PTY with the given working directory
pub fn spawn_pty(cwd: Option<PathBuf>) -> Result<(PtyManager, mpsc::UnboundedReceiver<Vec<u8>>)> {
    let (tx, rx) = mpsc::unbounded_channel();
    let manager = PtyManager::new(tx, cwd)?;
    Ok((manager, rx))
}
