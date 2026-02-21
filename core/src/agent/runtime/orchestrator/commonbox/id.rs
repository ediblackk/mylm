//! Job identifier types

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique job identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

impl JobId {
    /// Generate a new unique job ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Short format: first 8 chars of UUID
        write!(f, "{}", self.0.to_string().split('-').next().unwrap_or(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_id_display() {
        let job_id = JobId::new();
        let display = format!("{}", job_id);
        assert_eq!(display.len(), 8); // First 8 chars of UUID
    }
}
