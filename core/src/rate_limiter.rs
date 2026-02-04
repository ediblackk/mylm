//! Rate Limiter Module
//!
//! Provides per-endpoint rate limiting with separate quotas for main agent and workers.
//! Respects Retry-After headers from providers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::time::sleep;

/// Configuration for rate limiting
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per minute for main agent
    pub main_rpm: u32,
    /// Maximum requests per minute for workers (shared pool)
    pub workers_rpm: u32,
    /// Maximum tokens per minute for main agent
    pub main_tpm: u32,
    /// Maximum tokens per minute for workers (shared pool)
    pub workers_tpm: u32,
    /// Burst allowance (requests that can exceed RPM temporarily)
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            main_rpm: 60,      // 1 req/sec for main
            workers_rpm: 30,   // 0.5 req/sec shared for workers
            main_tpm: 100_000,
            workers_tpm: 50_000,
            burst_size: 3,
        }
    }
}

impl RateLimitConfig {
    /// Load from config file or environment
    pub fn from_settings(main_rpm: Option<u32>, workers_rpm: Option<u32>) -> Self {
        let mut config = Self::default();
        if let Some(rpm) = main_rpm {
            config.main_rpm = rpm;
        }
        if let Some(rpm) = workers_rpm {
            config.workers_rpm = rpm;
        }
        config
    }
}

/// Endpoint state tracking requests and tokens
#[derive(Debug)]
struct EndpointState {
    /// Base URL of the endpoint (for debugging)
    #[allow(dead_code)]
    base_url: String,
    /// Request timestamps for sliding window
    request_times: Vec<Instant>,
    /// Token usage timestamps and counts
    token_usage: Vec<(Instant, u32)>,
    /// Currently blocked until (from Retry-After)
    blocked_until: Option<Instant>,
    /// Consecutive rate limit errors
    consecutive_429s: u32,
    /// Circuit breaker state
    circuit_open: bool,
    /// Circuit breaker reset time
    circuit_reset_at: Option<Instant>,
}

impl EndpointState {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            request_times: Vec::new(),
            token_usage: Vec::new(),
            blocked_until: None,
            consecutive_429s: 0,
            circuit_open: false,
            circuit_reset_at: None,
        }
    }

    /// Clean up old entries outside the window
    fn cleanup_old_entries(&mut self, window: Duration) {
        let cutoff = Instant::now() - window;
        self.request_times.retain(|&t| t > cutoff);
        self.token_usage.retain(|(t, _)| *t > cutoff);
    }

    /// Check if circuit breaker is open
    fn is_circuit_open(&mut self) -> bool {
        if self.circuit_open {
            // Check if we should try resetting
            if let Some(reset_at) = self.circuit_reset_at {
                if Instant::now() > reset_at {
                    self.circuit_open = false;
                    self.consecutive_429s = 0;
                    self.circuit_reset_at = None;
                    return false;
                }
            }
            true
        } else {
            false
        }
    }

    /// Record a rate limit hit (429 error)
    fn record_rate_limit(&mut self, retry_after: Option<Duration>) {
        self.consecutive_429s += 1;
        
        // Block endpoint temporarily
        let block_duration = retry_after.unwrap_or_else(|| {
            // Exponential backoff: 5s, 10s, 20s, 40s, max 60s
            let base = Duration::from_secs(5);
            let multiplier = 2u32.pow(self.consecutive_429s.min(5));
            base * multiplier
        });
        
        self.blocked_until = Some(Instant::now() + block_duration);
        
        // Open circuit after 5 consecutive 429s
        if self.consecutive_429s >= 5 {
            self.circuit_open = true;
            self.circuit_reset_at = Some(Instant::now() + Duration::from_secs(60));
        }
    }

    /// Record successful request
    fn record_success(&mut self) {
        self.consecutive_429s = 0;
        self.request_times.push(Instant::now());
    }

    /// Record token usage
    fn record_tokens(&mut self, tokens: u32) {
        self.token_usage.push((Instant::now(), tokens));
    }

    /// Get current RPM
    fn current_rpm(&mut self) -> u32 {
        self.cleanup_old_entries(Duration::from_secs(60));
        self.request_times.len() as u32
    }

    /// Get current TPM
    fn current_tpm(&mut self) -> u32 {
        self.cleanup_old_entries(Duration::from_secs(60));
        self.token_usage.iter().map(|(_, t)| t).sum()
    }

    /// Time until next request is allowed
    fn time_until_available(&self, _max_rpm: u32, _max_tpm: u32, _burst: u32) -> Duration {
        let now = Instant::now();
        
        // Check if blocked by Retry-After
        if let Some(blocked_until) = self.blocked_until {
            if now < blocked_until {
                return blocked_until - now;
            }
        }
        
        // Check circuit breaker
        if self.circuit_open {
            if let Some(reset_at) = self.circuit_reset_at {
                if now < reset_at {
                    return reset_at - now;
                }
            }
        }
        
        Duration::ZERO
    }
}

/// Rate limiter for a specific agent type (main or worker)
#[derive(Debug)]
struct AgentRateLimiter {
    max_rpm: u32,
    max_tpm: u32,
    burst_size: u32,
    /// Semaphore for concurrent request limiting
    semaphore: Semaphore,
}

impl AgentRateLimiter {
    fn new(max_rpm: u32, max_tpm: u32, burst_size: u32) -> Self {
        // Allow burst_size concurrent requests
        let semaphore = Semaphore::new(burst_size as usize);
        
        Self {
            max_rpm,
            max_tpm,
            burst_size,
            semaphore,
        }
    }

    async fn acquire_permit(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.semaphore.acquire().await.expect("Semaphore closed")
    }
}

/// Global rate limiter managing all endpoints and agent types
#[derive(Debug, Clone)]
pub struct RateLimiter {
    #[allow(dead_code)]
    config: RateLimitConfig,
    /// Endpoint-specific state
    endpoints: Arc<Mutex<HashMap<String, EndpointState>>>,
    /// Main agent rate limiter
    main_limiter: Arc<AgentRateLimiter>,
    /// Worker rate limiter (shared across all workers)
    worker_limiter: Arc<AgentRateLimiter>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config: config.clone(),
            endpoints: Arc::new(Mutex::new(HashMap::new())),
            main_limiter: Arc::new(AgentRateLimiter::new(
                config.main_rpm,
                config.main_tpm,
                config.burst_size,
            )),
            worker_limiter: Arc::new(AgentRateLimiter::new(
                config.workers_rpm,
                config.workers_tpm,
                config.burst_size,
            )),
        }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }

    /// Get or create endpoint state - returns a clone since we can't hold the lock
    #[allow(dead_code)]
    fn with_endpoint<F, R>(&self, base_url: &str, f: F) -> R
    where
        F: FnOnce(&mut EndpointState) -> R,
    {
        let mut endpoints = self.endpoints.lock().unwrap();
        let endpoint = endpoints.entry(base_url.to_string())
            .or_insert_with(|| EndpointState::new(base_url.to_string()));
        f(endpoint)
    }

    /// Acquire permission to make a request
    /// 
    /// # Arguments
    /// * `base_url` - The endpoint URL
    /// * `is_worker` - Whether this is a worker request (true) or main agent (false)
    /// * `estimated_tokens` - Estimated token count for this request
    /// 
    /// # Returns
    /// Ok(()) if request can proceed, Err with wait duration if rate limited
    pub async fn acquire(
        &self,
        base_url: &str,
        is_worker: bool,
        estimated_tokens: u32,
    ) -> Result<(), RateLimitError> {
        let limiter = if is_worker {
            &self.worker_limiter
        } else {
            &self.main_limiter
        };

        // Acquire semaphore permit (limits concurrent requests)
        let _permit = limiter.acquire_permit().await;

        // Check endpoint-specific limits
        let mut endpoints = self.endpoints.lock().unwrap();
        let endpoint = endpoints.entry(base_url.to_string())
            .or_insert_with(|| EndpointState::new(base_url.to_string()));

        // Check circuit breaker
        if endpoint.is_circuit_open() {
            let wait = endpoint.time_until_available(limiter.max_rpm, limiter.max_tpm, limiter.burst_size);
            return Err(RateLimitError::CircuitOpen { retry_after: wait });
        }

        // Check if blocked by Retry-After
        let wait = endpoint.time_until_available(limiter.max_rpm, limiter.max_tpm, limiter.burst_size);
        if wait > Duration::ZERO {
            return Err(RateLimitError::Blocked { retry_after: wait });
        }

        // Check RPM limit
        let current_rpm = endpoint.current_rpm();
        if current_rpm >= limiter.max_rpm {
            // Calculate time until oldest request falls out of window
            let wait = Duration::from_secs(60) / limiter.max_rpm.max(1);
            return Err(RateLimitError::RateLimitExceeded { 
                resource: "requests",
                current: current_rpm,
                limit: limiter.max_rpm,
                retry_after: wait,
            });
        }

        // Check TPM limit
        let current_tpm = endpoint.current_tpm();
        if current_tpm + estimated_tokens > limiter.max_tpm {
            let wait = Duration::from_secs(60) / limiter.max_rpm.max(1);
            return Err(RateLimitError::RateLimitExceeded {
                resource: "tokens",
                current: current_tpm,
                limit: limiter.max_tpm,
                retry_after: wait,
            });
        }

        // Record the request
        endpoint.record_success();
        endpoint.record_tokens(estimated_tokens);

        Ok(())
    }

    /// Record a rate limit error (429) from the provider
    pub fn record_rate_limit_error(&self, base_url: &str, retry_after: Option<Duration>) {
        let mut endpoints = self.endpoints.lock().unwrap();
        if let Some(endpoint) = endpoints.get_mut(base_url) {
            endpoint.record_rate_limit(retry_after);
        }
    }

    /// Get current rate limit status for an endpoint
    pub fn get_status(&self, base_url: &str) -> Option<EndpointStatus> {
        let mut endpoints = self.endpoints.lock().unwrap();
        let endpoint = endpoints.get_mut(base_url)?;
        
        Some(EndpointStatus {
            current_rpm_main: endpoint.current_rpm(),
            current_rpm_workers: endpoint.current_rpm(), // Same endpoint, different limits
            current_tpm: endpoint.current_tpm(),
            blocked_until: endpoint.blocked_until,
            circuit_open: endpoint.circuit_open,
            consecutive_429s: endpoint.consecutive_429s,
        })
    }

    /// Wait and retry with exponential backoff
    pub async fn wait_and_retry<F, Fut, T>(
        &self,
        base_url: &str,
        is_worker: bool,
        estimated_tokens: u32,
        max_retries: u32,
        operation: F,
    ) -> Result<T, RateLimitError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, ProviderError>>,
    {
        let mut attempt = 0;

        loop {
            // Try to acquire rate limit
            match self.acquire(base_url, is_worker, estimated_tokens).await {
                Ok(()) => {
                    // Execute the operation
                    match operation().await {
                        Ok(result) => return Ok(result),
                        Err(ProviderError::RateLimit { retry_after }) => {
                            // Record the 429
                            self.record_rate_limit_error(base_url, retry_after);
                            let last_error = RateLimitError::Blocked { 
                                retry_after: retry_after.unwrap_or(Duration::from_secs(5))
                            };
                            
                            if attempt >= max_retries {
                                return Err(last_error);
                            }
                            
                            // Wait before retry
                            let wait = retry_after.unwrap_or_else(|| {
                                let base = Duration::from_secs(1);
                                base * 2u32.pow(attempt)
                            });
                            sleep(wait).await;
                        }
                        Err(e) => {
                            return Err(RateLimitError::ProviderError(e));
                        }
                    }
                }
                Err(e) => {
                    let last_error = e.clone();
                    if attempt >= max_retries {
                        return Err(e);
                    }
                    
                    // Wait and retry
                    let wait = match &last_error {
                        RateLimitError::Blocked { retry_after } => *retry_after,
                        RateLimitError::RateLimitExceeded { retry_after, .. } => *retry_after,
                        _ => Duration::from_secs(1) * 2u32.pow(attempt),
                    };
                    sleep(wait).await;
                }
            }
            
            attempt += 1;
        }
    }
}

/// Status of an endpoint
#[derive(Debug, Clone)]
pub struct EndpointStatus {
    pub current_rpm_main: u32,
    pub current_rpm_workers: u32,
    pub current_tpm: u32,
    pub blocked_until: Option<Instant>,
    pub circuit_open: bool,
    pub consecutive_429s: u32,
}

/// Rate limit error types
#[derive(Debug, Clone)]
pub enum RateLimitError {
    /// Rate limit exceeded (RPM or TPM)
    RateLimitExceeded {
        resource: &'static str,
        current: u32,
        limit: u32,
        retry_after: Duration,
    },
    /// Blocked by Retry-After header or circuit breaker
    Blocked {
        retry_after: Duration,
    },
    /// Circuit breaker is open
    CircuitOpen {
        retry_after: Duration,
    },
    /// Provider error
    ProviderError(ProviderError),
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RateLimitError::RateLimitExceeded { resource, current, limit, retry_after } => {
                write!(f, "Rate limit exceeded for {} ({}/{}), retry after {:?}", 
                    resource, current, limit, retry_after)
            }
            RateLimitError::Blocked { retry_after } => {
                write!(f, "Blocked by provider, retry after {:?}", retry_after)
            }
            RateLimitError::CircuitOpen { retry_after } => {
                write!(f, "Circuit breaker open, retry after {:?}", retry_after)
            }
            RateLimitError::ProviderError(e) => {
                write!(f, "Provider error: {:?}", e)
            }
        }
    }
}

impl std::error::Error for RateLimitError {}

/// Provider error types
#[derive(Debug, Clone)]
pub enum ProviderError {
    RateLimit { retry_after: Option<Duration> },
    Authentication,
    Network(String),
    Other(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::RateLimit { retry_after } => {
                write!(f, "Rate limit exceeded")?;
                if let Some(d) = retry_after {
                    write!(f, " (retry after {:?})", d)?;
                }
                Ok(())
            }
            ProviderError::Authentication => write!(f, "Authentication failed"),
            ProviderError::Network(msg) => write!(f, "Network error: {}", msg),
            ProviderError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ProviderError {}
