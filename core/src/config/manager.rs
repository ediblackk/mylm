//! Configuration Manager with hot-reload and rate limiting
//!
//! Provides centralized configuration management loaded from `$HOME/.mylm/config.toml`,
//! with support for hot-reloading and token bucket rate limiting.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// IO error occurred while reading/writing config file
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parsing error
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    /// TOML serialization error
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    /// Invalid configuration value
    #[error("Invalid configuration value: {0}")]
    InvalidValue(String),
}

/// Rate limit error with retry information
#[derive(Debug, thiserror::Error)]
#[error("Rate limit exceeded. Retry after {retry_after:?}")]
pub struct RateLimitError {
    /// Duration to wait before retrying
    pub retry_after: Duration,
    /// Current available tokens
    pub available_tokens: usize,
    /// Requested tokens
    pub requested_tokens: usize,
}

impl RateLimitError {
    /// Create a new rate limit error
    pub fn new(retry_after: Duration, available_tokens: usize, requested_tokens: usize) -> Self {
        Self {
            retry_after,
            available_tokens,
            requested_tokens,
        }
    }
}

/// Cost per token for a model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostPerToken {
    /// Input price per million tokens
    pub input_price_per_million: f64,
    /// Output price per million tokens
    pub output_price_per_million: f64,
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Maximum context tokens
    pub max_context_tokens: usize,
    /// Condense threshold (0.0 - 1.0)
    pub condense_threshold: f64,
    /// Maximum output tokens
    pub max_output_tokens: usize,
    /// Worker limit for concurrent tasks
    pub worker_limit: usize,
    /// Rate limit: tokens per minute
    pub rate_limit_tokens_per_minute: usize,
    /// Rate limit: requests per minute
    pub rate_limit_requests_per_minute: usize,
    /// Model costs mapping (key: "provider/model")
    pub model_costs: HashMap<String, CostPerToken>,
}

impl Default for Config {
    fn default() -> Self {
        let mut model_costs = HashMap::new();
        model_costs.insert(
            "gemini-2.0-flash".to_string(),
            CostPerToken {
                input_price_per_million: 0.10,
                output_price_per_million: 0.40,
            },
        );
        model_costs.insert(
            "gemini-1.5-pro".to_string(),
            CostPerToken {
                input_price_per_million: 1.25,
                output_price_per_million: 5.00,
            },
        );

        Self {
            max_context_tokens: 128_000,
            condense_threshold: 0.8,
            max_output_tokens: 4096,
            worker_limit: 5,
            rate_limit_tokens_per_minute: 100_000,
            rate_limit_requests_per_minute: 100,
            model_costs,
        }
    }
}

impl Config {
    /// Validate configuration values
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.max_context_tokens == 0 {
            return Err(ConfigError::InvalidValue(
                "max_context_tokens must be greater than 0".to_string(),
            ));
        }
        if self.condense_threshold < 0.0 || self.condense_threshold > 1.0 {
            return Err(ConfigError::InvalidValue(
                "condense_threshold must be between 0.0 and 1.0".to_string(),
            ));
        }
        if self.max_output_tokens == 0 {
            return Err(ConfigError::InvalidValue(
                "max_output_tokens must be greater than 0".to_string(),
            ));
        }
        if self.worker_limit == 0 {
            return Err(ConfigError::InvalidValue(
                "worker_limit must be greater than 0".to_string(),
            ));
        }
        if self.rate_limit_tokens_per_minute == 0 {
            return Err(ConfigError::InvalidValue(
                "rate_limit_tokens_per_minute must be greater than 0".to_string(),
            ));
        }
        if self.rate_limit_requests_per_minute == 0 {
            return Err(ConfigError::InvalidValue(
                "rate_limit_requests_per_minute must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }
}

/// Token bucket state for rate limiting
#[derive(Debug, Clone)]
struct TokenBucket {
    /// Available tokens
    tokens_available: f64,
    /// Last refill time
    last_refill: Instant,
    /// Maximum capacity
    max_capacity: usize,
    /// Refill rate (tokens per second)
    refill_rate: f64,
}

impl TokenBucket {
    fn new(max_capacity: usize, tokens_per_minute: usize) -> Self {
        let refill_rate = tokens_per_minute as f64 / 60.0;
        Self {
            tokens_available: max_capacity as f64,
            last_refill: Instant::now(),
            max_capacity,
            refill_rate,
        }
    }

    /// Refill the bucket based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let tokens_to_add = elapsed * self.refill_rate;
        self.tokens_available = (self.tokens_available + tokens_to_add).min(self.max_capacity as f64);
        self.last_refill = now;
    }

    /// Try to consume tokens, returns true if successful
    fn try_consume(&mut self, tokens: usize) -> bool {
        self.refill();
        if self.tokens_available >= tokens as f64 {
            self.tokens_available -= tokens as f64;
            true
        } else {
            false
        }
    }

    /// Get current available tokens
    fn available(&self) -> usize {
        self.tokens_available as usize
    }

    /// Calculate wait time for given tokens
    fn wait_time_for(&self, tokens: usize) -> Duration {
        if self.tokens_available >= tokens as f64 {
            return Duration::ZERO;
        }
        let tokens_needed = tokens as f64 - self.tokens_available;
        let seconds_needed = tokens_needed / self.refill_rate;
        Duration::from_secs_f64(seconds_needed)
    }
}

/// Request counter for per-minute request limiting
#[derive(Debug, Clone)]
struct RequestCounter {
    /// Number of requests in current window
    count: usize,
    /// Window start time
    window_start: Instant,
    /// Maximum requests per window
    max_requests: usize,
    /// Window duration
    window_duration: Duration,
}

impl RequestCounter {
    fn new(max_requests_per_minute: usize) -> Self {
        Self {
            count: 0,
            window_start: Instant::now(),
            max_requests: max_requests_per_minute,
            window_duration: Duration::from_secs(60),
        }
    }

    /// Record a request, returns true if allowed
    fn record(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_start) >= self.window_duration {
            // Reset window
            self.count = 0;
            self.window_start = now;
        }

        if self.count < self.max_requests {
            self.count += 1;
            true
        } else {
            false
        }
    }

    /// Get remaining requests in current window
    fn remaining(&self) -> usize {
        let now = Instant::now();
        if now.duration_since(self.window_start) >= self.window_duration {
            self.max_requests
        } else {
            self.max_requests.saturating_sub(self.count)
        }
    }

    /// Get time until window resets
    fn time_until_reset(&self) -> Duration {
        let now = Instant::now();
        let elapsed = now.duration_since(self.window_start);
        if elapsed >= self.window_duration {
            Duration::ZERO
        } else {
            self.window_duration - elapsed
        }
    }
}

/// Rate limiter state
#[derive(Debug, Clone)]
struct RateLimiter {
    token_bucket: TokenBucket,
    request_counter: RequestCounter,
}

impl RateLimiter {
    fn new(tokens_per_minute: usize, requests_per_minute: usize) -> Self {
        Self {
            token_bucket: TokenBucket::new(tokens_per_minute, tokens_per_minute),
            request_counter: RequestCounter::new(requests_per_minute),
        }
    }

    /// Update rate limits from config
    fn update_limits(&mut self, tokens_per_minute: usize, requests_per_minute: usize) {
        self.token_bucket.max_capacity = tokens_per_minute;
        self.token_bucket.refill_rate = tokens_per_minute as f64 / 60.0;
        self.request_counter.max_requests = requests_per_minute;
    }
}

/// Configuration Manager with hot-reload support
pub struct ConfigManager {
    /// Current configuration
    config: RwLock<Config>,
    /// Path to config file
    config_path: PathBuf,
    /// Rate limiter state
    rate_limiter: RwLock<RateLimiter>,
    /// Last known modification time for file watching
    last_modified: RwLock<Option<std::time::SystemTime>>,
}

impl ConfigManager {
    /// Create a new ConfigManager, loading from `$HOME/.mylm/config.toml`
    /// Creates default config if file doesn't exist
    pub async fn new() -> Result<Arc<Self>, ConfigError> {
        let config_path = Self::default_config_path()?;

        // Ensure config directory exists
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Load or create config
        let (config, last_modified) = if config_path.exists() {
            let content = fs::read_to_string(&config_path).await?;
            let config: Config = toml::from_str(&content)?;
            config.validate()?;
            let metadata = fs::metadata(&config_path).await?;
            let modified = metadata.modified().ok();
            (config, modified)
        } else {
            let config = Config::default();
            config.validate()?;
            // Save default config
            let toml_string = toml::to_string_pretty(&config)?;
            fs::write(&config_path, toml_string).await?;
            let metadata = fs::metadata(&config_path).await?;
            let modified = metadata.modified().ok();
            eprintln!("Created default config at {:?}", config_path);
            (config, modified)
        };

        let rate_limiter = RateLimiter::new(
            config.rate_limit_tokens_per_minute,
            config.rate_limit_requests_per_minute,
        );

        Ok(Arc::new(Self {
            config: RwLock::new(config),
            config_path,
            rate_limiter: RwLock::new(rate_limiter),
            last_modified: RwLock::new(last_modified),
        }))
    }

    /// Get the default config path: `$HOME/.mylm/config.toml`
    fn default_config_path() -> Result<PathBuf, ConfigError> {
        let home_dir = dirs::home_dir().ok_or_else(|| {
            ConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine home directory",
            ))
        })?;
        Ok(home_dir.join(".mylm").join("config.toml"))
    }

    /// Get an immutable reference to the current config
    pub async fn get_config(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Get config synchronously (use with caution - may block)
    /// Prefer `get_config()` for async contexts
    pub fn get_config_blocking(&self) -> Option<Config> {
        // Try to get read lock without blocking
        if let Ok(guard) = self.config.try_read() {
            Some(guard.clone())
        } else {
            None
        }
    }

    /// Reload configuration from disk
    pub async fn reload(&self) -> Result<(), ConfigError> {
        if !self.config_path.exists() {
            return Err(ConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Config file not found: {:?}", self.config_path),
            )));
        }

        let content = fs::read_to_string(&self.config_path).await?;
        let new_config: Config = toml::from_str(&content)?;
        new_config.validate()?;

        // Update rate limiter with new limits
        let mut rate_limiter = self.rate_limiter.write().await;
        rate_limiter.update_limits(
            new_config.rate_limit_tokens_per_minute,
            new_config.rate_limit_requests_per_minute,
        );

        // Update config
        let mut config = self.config.write().await;
        *config = new_config;

        // Update last modified time
        let metadata = fs::metadata(&self.config_path).await?;
        let mut last_modified = self.last_modified.write().await;
        *last_modified = metadata.modified().ok();

        eprintln!("Config reloaded successfully from {:?}", self.config_path);
        Ok(())
    }

    /// Check rate limit for a request with given input tokens
    /// Returns Ok(()) if allowed, Err(RateLimitError) if rate limited
    pub async fn check_rate_limit(&self, input_tokens: usize) -> Result<(), RateLimitError> {
        let mut rate_limiter = self.rate_limiter.write().await;

        // Check request limit first
        if !rate_limiter.request_counter.record() {
            let retry_after = rate_limiter.request_counter.time_until_reset();
            return Err(RateLimitError::new(
                retry_after,
                rate_limiter.request_counter.remaining(),
                input_tokens,
            ));
        }

        // Check token bucket
        if !rate_limiter.token_bucket.try_consume(input_tokens) {
            let retry_after = rate_limiter.token_bucket.wait_time_for(input_tokens);
            return Err(RateLimitError::new(
                retry_after,
                rate_limiter.token_bucket.available(),
                input_tokens,
            ));
        }

        Ok(())
    }

    /// Check rate limit synchronously (for use in non-async contexts)
    /// Returns None if lock cannot be acquired
    pub fn check_rate_limit_blocking(&self, input_tokens: usize) -> Option<Result<(), RateLimitError>> {
        if let Ok(mut rate_limiter) = self.rate_limiter.try_write() {
            // Check request limit
            if !rate_limiter.request_counter.record() {
                let retry_after = rate_limiter.request_counter.time_until_reset();
                return Some(Err(RateLimitError::new(
                    retry_after,
                    rate_limiter.request_counter.remaining(),
                    input_tokens,
                )));
            }

            // Check token bucket
            if !rate_limiter.token_bucket.try_consume(input_tokens) {
                let retry_after = rate_limiter.token_bucket.wait_time_for(input_tokens);
                return Some(Err(RateLimitError::new(
                    retry_after,
                    rate_limiter.token_bucket.available(),
                    input_tokens,
                )));
            }

            Some(Ok(()))
        } else {
            None
        }
    }

    /// Record a request (increments request counter)
    pub async fn record_request(&self) {
        let mut rate_limiter = self.rate_limiter.write().await;
        rate_limiter.request_counter.record();
    }

    /// Get current rate limit status
    pub async fn get_rate_limit_status(&self) -> (usize, usize, usize, usize) {
        let rate_limiter = self.rate_limiter.read().await;
        (
            rate_limiter.token_bucket.available(),
            rate_limiter.token_bucket.max_capacity,
            rate_limiter.request_counter.remaining(),
            rate_limiter.request_counter.max_requests,
        )
    }

    /// Start a file watcher task that polls for config file changes
    /// Returns a JoinHandle that can be awaited or dropped
    pub fn start_watcher(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let manager = Arc::clone(self);

        tokio::spawn(async move {
            let poll_interval = Duration::from_secs(2);

            loop {
                sleep(poll_interval).await;

                // Check if file has been modified
                match fs::metadata(&manager.config_path).await {
                    Ok(metadata) => {
                        if let Ok(modified) = metadata.modified() {
                            let should_reload = {
                                let last_modified = manager.last_modified.read().await;
                                match *last_modified {
                                    Some(last) => modified > last,
                                    None => true,
                                }
                            };

                            if should_reload {
                                if let Err(e) = manager.reload().await {
                                    eprintln!("Config reload failed: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to read config file metadata: {}", e);
                    }
                }
            }
        })
    }

    /// Get the current worker limit
    pub async fn get_worker_limit(&self) -> usize {
        self.config.read().await.worker_limit
    }

    /// Get cost per token for a model
    pub async fn get_model_cost(&self, model: &str) -> Option<CostPerToken> {
        self.config.read().await.model_costs.get(model).cloned()
    }

    /// Get config path
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.max_context_tokens, 128_000);
        assert_eq!(config.condense_threshold, 0.8);
        assert_eq!(config.max_output_tokens, 4096);
        assert_eq!(config.worker_limit, 5);
        assert_eq!(config.rate_limit_tokens_per_minute, 100_000);
        assert_eq!(config.rate_limit_requests_per_minute, 100);
        assert!(config.model_costs.contains_key("gemini-2.0-flash"));
        assert!(config.model_costs.contains_key("gemini-1.5-pro"));
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        assert!(config.validate().is_ok());

        config.max_context_tokens = 0;
        assert!(config.validate().is_err());

        config = Config::default();
        config.condense_threshold = 1.5;
        assert!(config.validate().is_err());

        config = Config::default();
        config.worker_limit = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_token_bucket() {
        let mut bucket = TokenBucket::new(100, 60); // 100 max, 1 per second
        assert_eq!(bucket.available(), 100);

        // Consume some tokens
        assert!(bucket.try_consume(30));
        assert_eq!(bucket.available(), 70);

        // Try to consume more than available
        assert!(!bucket.try_consume(80));
        assert_eq!(bucket.available(), 70); // Should not change

        // Consume remaining
        assert!(bucket.try_consume(70));
        assert_eq!(bucket.available(), 0);
    }

    #[test]
    fn test_request_counter() {
        let mut counter = RequestCounter::new(10);
        assert_eq!(counter.remaining(), 10);

        // Record some requests
        for _ in 0..5 {
            assert!(counter.record());
        }
        assert_eq!(counter.remaining(), 5);

        // Fill up
        for _ in 0..5 {
            assert!(counter.record());
        }
        assert_eq!(counter.remaining(), 0);

        // Next request should fail
        assert!(!counter.record());
    }

    #[test]
    fn test_rate_limit_error() {
        let err = RateLimitError::new(Duration::from_secs(5), 10, 100);
        assert_eq!(err.retry_after, Duration::from_secs(5));
        assert_eq!(err.available_tokens, 10);
        assert_eq!(err.requested_tokens, 100);
        assert!(err.to_string().contains("Rate limit exceeded"));
    }
}
