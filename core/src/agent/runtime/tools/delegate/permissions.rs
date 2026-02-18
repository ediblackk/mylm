//! Worker permissions builder

use crate::agent::runtime::tools::worker_shell::WorkerShellPermissions;
use super::types::WorkerConfig;

/// Build worker permissions from config
pub fn build_worker_permissions(config: &WorkerConfig) -> WorkerShellPermissions {
    let mut perms = WorkerShellPermissions::standard();
    
    // Apply allowed commands override
    if let Some(allowed) = &config.allowed_commands {
        perms.allowed_patterns = allowed.clone();
    }
    
    // Apply forbidden commands
    if let Some(forbidden) = &config.forbidden_commands {
        perms.forbidden_patterns.extend(forbidden.iter().cloned());
    }
    
    perms
}
