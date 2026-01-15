use std::time::{Duration, Instant};
use mylm_core::llm::TokenUsage;
use mylm_core::llm::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Session data for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub history: Vec<ChatMessage>,
    pub metadata: SessionMetadata,
    #[serde(default)]
    pub terminal_history: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub last_message_preview: String,
    pub message_count: usize,
    pub total_tokens: u32,
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    pub cost: f64,
    #[serde(default)]
    pub elapsed_seconds: u64,
}

/// Session statistics for the current TUI session
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    pub cost: f64,
    pub start_time: Instant,
    pub base_duration: Duration,
    pub active_context_tokens: u32,
    pub max_context_tokens: u32,
}

impl Default for SessionStats {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost: 0.0,
            start_time: Instant::now(),
            base_duration: Duration::from_secs(0),
            active_context_tokens: 0,
            max_context_tokens: 32768,
        }
    }
}

pub struct SessionMonitor {
    stats: SessionStats,
}

impl SessionMonitor {
    pub fn new() -> Self {
        Self {
            stats: SessionStats::default(),
        }
    }

    /// Set initial stats for resumed session
    pub fn resume_stats(&mut self, metadata: &SessionMetadata) {
        self.stats.input_tokens = metadata.input_tokens;
        self.stats.output_tokens = metadata.output_tokens;
        self.stats.total_tokens = metadata.total_tokens;
        self.stats.cost = metadata.cost;
        self.stats.base_duration = Duration::from_secs(metadata.elapsed_seconds);
        self.stats.start_time = Instant::now();
    }

    /// Add usage from a single LLM interaction
    pub fn add_usage(&mut self, usage: &TokenUsage, input_price_1m: f64, output_price_1m: f64) {
        self.stats.input_tokens += usage.prompt_tokens;
        self.stats.output_tokens += usage.completion_tokens;
        self.stats.total_tokens += usage.total_tokens;

        // Active context is what the LLM just processed (prompt + completion)
        self.stats.active_context_tokens = usage.total_tokens;

        // Cost per token = price_per_1m / 1,000,000
        let input_cost = usage.prompt_tokens as f64 * (input_price_1m / 1_000_000.0);
        let output_cost = usage.completion_tokens as f64 * (output_price_1m / 1_000_000.0);
        self.stats.cost += input_cost + output_cost;
    }

    pub fn set_max_context(&mut self, max_tokens: u32) {
        self.stats.max_context_tokens = max_tokens;
    }

    pub fn get_context_ratio(&self) -> f64 {
        if self.stats.max_context_tokens == 0 {
            return 0.0;
        }
        self.stats.active_context_tokens as f64 / self.stats.max_context_tokens as f64
    }

    /// Get current session statistics
    pub fn get_stats(&self) -> &SessionStats {
        &self.stats
    }

    /// Get session duration
    pub fn duration(&self) -> Duration {
        self.stats.base_duration + self.stats.start_time.elapsed()
    }

    /// Format duration as MM:SS or HH:MM:SS
    pub fn format_duration(&self) -> String {
        let elapsed = self.duration().as_secs();
        let hours = elapsed / 3600;
        let minutes = (elapsed % 3600) / 60;
        let seconds = elapsed % 60;

        if hours > 0 {
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{:02}:{:02}", minutes, seconds)
        }
    }
}
