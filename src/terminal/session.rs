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
    #[serde(default)]
    pub agent_session_id: String,
    #[serde(default)]
    pub agent_history: Vec<ChatMessage>,
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

impl Default for SessionMetadata {
    fn default() -> Self {
        Self {
            last_message_preview: String::new(),
            message_count: 0,
            total_tokens: 0,
            input_tokens: 0,
            output_tokens: 0,
            cost: 0.0,
            elapsed_seconds: 0,
        }
    }
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
    /// Input price per 1M tokens
    pub input_price_per_million: f64,
    /// Output price per 1M tokens
    pub output_price_per_million: f64,
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
            max_context_tokens: 0,
            input_price_per_million: 0.0,
            output_price_per_million: 0.0,
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self {
            id: String::new(),
            timestamp: chrono::Utc::now(),
            history: Vec::new(),
            metadata: SessionMetadata::default(),
            terminal_history: Vec::new(),
            agent_session_id: String::new(),
            agent_history: Vec::new(),
        }
    }
}

pub struct SessionMonitor {
    stats: SessionStats,
}

impl SessionMonitor {
    pub fn new(max_context_tokens: u32) -> Self {
        Self {
            stats: SessionStats {
                max_context_tokens,
                ..SessionStats::default()
            },
        }
    }

    /// Set initial stats for resumed session
    pub fn resume_stats(&mut self, metadata: &SessionMetadata, max_context_tokens: u32) {
        self.stats.input_tokens = metadata.input_tokens;
        self.stats.output_tokens = metadata.output_tokens;
        self.stats.total_tokens = metadata.total_tokens;
        self.stats.active_context_tokens = metadata.total_tokens;
        self.stats.max_context_tokens = max_context_tokens;
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
        // Use provided prices or fall back to stored prices
        let input_price = if input_price_1m > 0.0 { input_price_1m } else { self.stats.input_price_per_million };
        let output_price = if output_price_1m > 0.0 { output_price_1m } else { self.stats.output_price_per_million };
        
        let input_cost = usage.prompt_tokens as f64 * (input_price / 1_000_000.0);
        let output_cost = usage.completion_tokens as f64 * (output_price / 1_000_000.0);
        self.stats.cost += input_cost + output_cost;
    }

    /// Set pricing for cost calculation
    pub fn set_pricing(&mut self, input_price_per_million: f64, output_price_per_million: f64) {
        self.stats.input_price_per_million = input_price_per_million;
        self.stats.output_price_per_million = output_price_per_million;
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
}
