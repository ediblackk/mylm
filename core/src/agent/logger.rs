use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use chrono::Local;
use std::sync::OnceLock;

pub struct DebugLogEntry {
    pub timestamp: String,
    pub level: String,
    pub module: String,
    pub message: String,
}

pub struct DebugLogger {
    ring_buffer: VecDeque<DebugLogEntry>,
    max_entries: usize,
    file_path: Option<PathBuf>,
}

static LOGGER: OnceLock<Arc<Mutex<DebugLogger>>> = OnceLock::new();

fn get_logger() -> &'static Arc<Mutex<DebugLogger>> {
    LOGGER.get_or_init(|| Arc::new(Mutex::new(DebugLogger::new(1000))))
}

impl DebugLogger {
    pub fn new(max_entries: usize) -> Self {
        Self {
            ring_buffer: VecDeque::with_capacity(max_entries),
            max_entries,
            file_path: None,
        }
    }

    pub fn set_file_path(&mut self, path: PathBuf) {
        // Ensure directory exists
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.file_path = Some(path);
    }

    pub fn log(&mut self, level: &str, module: &str, message: &str) {
        let entry = DebugLogEntry {
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            level: level.to_string(),
            module: module.to_string(),
            message: message.to_string(),
        };

        // Write to file if configured
        if let Some(path) = &self.file_path {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(
                    file,
                    "[{}] [{}] [{}] {}",
                    entry.timestamp, entry.level, entry.module, entry.message
                );
            }
        }

        // Add to ring buffer
        if self.ring_buffer.len() >= self.max_entries {
            self.ring_buffer.pop_front();
        }
        self.ring_buffer.push_back(entry);
    }

    pub fn get_recent(&self, n: usize) -> Vec<String> {
        self.ring_buffer
            .iter()
            .rev()
            .take(n)
            .map(|e| {
                format!(
                    "[{}] [{}] [{}] {}",
                    e.timestamp, e.level, e.module, e.message
                )
            })
            .collect::<Vec<_>>()
    }
}

pub fn init(data_dir: PathBuf) {
    let logger = get_logger();
    let mut logger = logger.lock().unwrap();
    logger.set_file_path(data_dir.join("debug.log"));
}

pub fn log(level: &str, module: &str, message: impl Into<String>) {
    let logger = get_logger();
    let mut logger = logger.lock().unwrap();
    logger.log(level, module, &message.into());
}

#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        $crate::agent::logger::log("DEBUG", module_path!(), format!($($arg)*));
    };
}

#[macro_export]
macro_rules! info_log {
    ($($arg:tt)*) => {
        $crate::agent::logger::log("INFO", module_path!(), format!($($arg)*));
    };
}

#[macro_export]
macro_rules! error_log {
    ($($arg:tt)*) => {
        $crate::agent::logger::log("ERROR", module_path!(), format!($($arg)*));
    };
}

pub fn get_recent_logs(n: usize) -> Vec<String> {
    let logger = get_logger();
    let logger = logger.lock().unwrap();
    logger.get_recent(n)
}
