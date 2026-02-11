//! Rate Limiter Module
//!
//! Provides per-endpoint rate limiting with separate quotas for main agent and workers.
//! Respects Retry-After headers from providers.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
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

    /// Conservative limits for basic tier providers (default)
    pub fn conservative() -> Self {
        Self {
            main_rpm: 60,      // 1 req/sec
            workers_rpm: 30,   // 0.5 req/sec shared
            main_tpm: 100_000,
            workers_tpm: 50_000,
            burst_size: 3,
        }
    }

    /// Standard limits for mid-tier providers
    pub fn standard() -> Self {
        Self {
            main_rpm: 120,      // 2 req/sec
            workers_rpm: 300,   // 5 req/sec shared (100 workers = 3 req/min each)
            main_tpm: 250_000,
            workers_tpm: 500_000,
            burst_size: 10,
        }
    }

    /// High-tier limits for providers with generous rate limits
    pub fn high_tier() -> Self {
        Self {
            main_rpm: 300,       // 5 req/sec
            workers_rpm: 1200,   // 20 req/sec shared (100 workers = 12 req/min each)
            main_tpm: 1_000_000,
            workers_tpm: 5_000_000,
            burst_size: 25,
        }
    }

    /// Enterprise/unlimited tier for providers with very high limits
    pub fn enterprise() -> Self {
        Self {
            main_rpm: 600,       // 10 req/sec
            workers_rpm: 6000,   // 100 req/sec shared (100 workers = 60 req/min each)
            main_tpm: 5_000_000,
            workers_tpm: 50_000_000,
            burst_size: 100,
        }
    }

    /// Select configuration based on provider tier
    pub fn for_tier(tier: &str) -> Self {
        match tier.to_lowercase().as_str() {
            "conservative" | "basic" | "free" => Self::conservative(),
            "standard" | "pro" => Self::standard(),
            "high" | "premium" | "business" => Self::high_tier(),
            "enterprise" | "unlimited" => Self::enterprise(),
            _ => Self::default(),
        }
    }
}

/// Circuit breaker state for a specific agent type
#[derive(Debug)]
struct CircuitState {
    /// Currently blocked until (from Retry-After)
    blocked_until: Option<Instant>,
    /// Consecutive rate limit errors
    consecutive_429s: u32,
    /// Circuit breaker state
    circuit_open: bool,
    /// Circuit breaker reset time
    circuit_reset_at: Option<Instant>,
}

impl CircuitState {
    fn new() -> Self {
        Self {
            blocked_until: None,
            consecutive_429s: 0,
            circuit_open: false,
            circuit_reset_at: None,
        }
    }
}

/// Endpoint state tracking requests and tokens
/// Separates main agent and worker requests for proper rate limiting
#[derive(Debug)]
struct EndpointState {
    /// Request timestamps for sliding window - Main agent
    request_times_main: Vec<Instant>,
    /// Request timestamps for sliding window - Workers (shared pool)
    request_times_workers: Vec<Instant>,
    /// Token usage timestamps and counts - Main agent
    token_usage_main: Vec<(Instant, u32)>,
    /// Token usage timestamps and counts - Workers
    token_usage_workers: Vec<(Instant, u32)>,
    /// Circuit breaker state - Main agent
    circuit_main: CircuitState,
    /// Circuit breaker state - Workers
    circuit_workers: CircuitState,
}

impl EndpointState {
    fn new() -> Self {
        Self {
            request_times_main: Vec::new(),
            request_times_workers: Vec::new(),
            token_usage_main: Vec::new(),
            token_usage_workers: Vec::new(),
            circuit_main: CircuitState::new(),
            circuit_workers: CircuitState::new(),
        }
    }

    /// Get circuit state for a specific agent type
    fn circuit(&mut self, is_worker: bool) -> &mut CircuitState {
        if is_worker {
            &mut self.circuit_workers
        } else {
            &mut self.circuit_main
        }
    }

    /// Clean up old entries outside the window for both main and workers
    fn cleanup_old_entries(&mut self, window: Duration) {
        let cutoff = Instant::now() - window;
        self.request_times_main.retain(|&t| t > cutoff);
        self.request_times_workers.retain(|&t| t > cutoff);
        self.token_usage_main.retain(|(t, _)| *t > cutoff);
        self.token_usage_workers.retain(|(t, _)| *t > cutoff);
    }

    /// Get request times for a specific agent type
    fn request_times(&mut self, is_worker: bool) -> &mut Vec<Instant> {
        if is_worker {
            &mut self.request_times_workers
        } else {
            &mut self.request_times_main
        }
    }

    /// Get token usage for a specific agent type
    fn token_usage(&mut self, is_worker: bool) -> &mut Vec<(Instant, u32)> {
        if is_worker {
            &mut self.token_usage_workers
        } else {
            &mut self.token_usage_main
        }
    }

    /// Check if circuit breaker is open for a specific agent type
    fn is_circuit_open(&mut self, is_worker: bool) -> bool {
        let circuit = self.circuit(is_worker);
        if circuit.circuit_open {
            // Check if we should try resetting
            if let Some(reset_at) = circuit.circuit_reset_at {
                if Instant::now() > reset_at {
                    circuit.circuit_open = false;
                    circuit.consecutive_429s = 0;
                    circuit.circuit_reset_at = None;
                    return false;
                }
            }
            true
        } else {
            false
        }
    }

    /// Record a rate limit hit (429 error) for a specific agent type
    fn record_rate_limit(&mut self, is_worker: bool, retry_after: Option<Duration>) {
        let circuit = self.circuit(is_worker);
        circuit.consecutive_429s += 1;
        
        // Block endpoint temporarily
        let block_duration = retry_after.unwrap_or_else(|| {
            // Exponential backoff: 5s, 10s, 20s, 40s, max 60s
            let base = Duration::from_secs(5);
            let multiplier = 2u32.pow(circuit.consecutive_429s.min(5));
            base * multiplier
        });
        
        circuit.blocked_until = Some(Instant::now() + block_duration);
        
        // Open circuit after 5 consecutive 429s
        if circuit.consecutive_429s >= 5 {
            circuit.circuit_open = true;
            circuit.circuit_reset_at = Some(Instant::now() + Duration::from_secs(60));
        }
    }

    /// Record successful request for specific agent type
    fn record_success(&mut self, is_worker: bool) {
        self.circuit(is_worker).consecutive_429s = 0;
        self.request_times(is_worker).push(Instant::now());
    }

    /// Record token usage for specific agent type
    fn record_tokens(&mut self, tokens: u32, is_worker: bool) {
        self.token_usage(is_worker).push((Instant::now(), tokens));
    }

    /// Correct usage based on actual token count
    fn correct_usage(&mut self, excess_tokens: u32, is_worker: bool) {
        // Just record the excess as a new usage event at current time
        self.record_tokens(excess_tokens, is_worker);
    }

    /// Get current RPM for specific agent type
    fn current_rpm(&mut self, is_worker: bool) -> u32 {
        self.cleanup_old_entries(Duration::from_secs(60));
        self.request_times(is_worker).len() as u32
    }

    /// Get current TPM for specific agent type
    fn current_tpm(&mut self, is_worker: bool) -> u32 {
        self.cleanup_old_entries(Duration::from_secs(60));
        self.token_usage(is_worker).iter().map(|(_, t)| t).sum()
    }

    /// Time until next request is allowed for a specific agent type
    fn time_until_available(&self, is_worker: bool) -> Duration {
        let now = Instant::now();
        let circuit = if is_worker {
            &self.circuit_workers
        } else {
            &self.circuit_main
        };
        
        // Check if blocked by Retry-After
        if let Some(blocked_until) = circuit.blocked_until {
            if now < blocked_until {
                return blocked_until - now;
            }
        }
        
        // Check circuit breaker
        if circuit.circuit_open {
            if let Some(reset_at) = circuit.circuit_reset_at {
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
    /// Semaphore for concurrent request limiting (controls burst)
    semaphore: Semaphore,
}

impl AgentRateLimiter {
    fn new(max_rpm: u32, max_tpm: u32, burst_size: u32) -> Self {
        // Allow burst_size concurrent requests
        let semaphore = Semaphore::new(burst_size as usize);
        
        Self {
            max_rpm,
            max_tpm,
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
    pub fn with_default_config() -> Self {
        Self::new(RateLimitConfig::default())
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
        let mut endpoints = self.endpoints.lock();
        let endpoint = endpoints.entry(base_url.to_string())
            .or_insert_with(EndpointState::new);

        // Check circuit breaker
        if endpoint.is_circuit_open(is_worker) {
            let wait = endpoint.time_until_available(is_worker);
            return Err(RateLimitError::CircuitOpen { retry_after: wait });
        }

        // Check if blocked by Retry-After
        let wait = endpoint.time_until_available(is_worker);
        if wait > Duration::ZERO {
            return Err(RateLimitError::Blocked { retry_after: wait });
        }

        // Check RPM limit (for specific agent type)
        let current_rpm = endpoint.current_rpm(is_worker);
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

        // Check TPM limit (for specific agent type)
        let current_tpm = endpoint.current_tpm(is_worker);
        if current_tpm + estimated_tokens > limiter.max_tpm {
            let wait = Duration::from_secs(60) / limiter.max_rpm.max(1);
            return Err(RateLimitError::RateLimitExceeded {
                resource: "tokens",
                current: current_tpm,
                limit: limiter.max_tpm,
                retry_after: wait,
            });
        }

        // Record the request (for specific agent type)
        endpoint.record_success(is_worker);
        endpoint.record_tokens(estimated_tokens, is_worker);

        Ok(())
    }

    /// Record a rate limit error (429) from the provider for a specific agent type
    pub fn record_rate_limit_error(&self, base_url: &str, is_worker: bool, retry_after: Option<Duration>) {
        let mut endpoints = self.endpoints.lock();
        if let Some(endpoint) = endpoints.get_mut(base_url) {
            endpoint.record_rate_limit(is_worker, retry_after);
        }
    }

    /// Record actual usage and correct if estimate was too low
    pub fn record_usage(&self, base_url: &str, is_worker: bool, actual_tokens: u32, estimated_tokens: u32) {
        if actual_tokens > estimated_tokens {
            let excess = actual_tokens - estimated_tokens;
            let mut endpoints = self.endpoints.lock();
            // Only correct if endpoint state exists
            if let Some(endpoint) = endpoints.get_mut(base_url) {
                endpoint.correct_usage(excess, is_worker);
            }
        }
    }

    /// Get current rate limit status for an endpoint
    pub fn get_status(&self, base_url: &str) -> Option<EndpointStatus> {
        let mut endpoints = self.endpoints.lock();
        let endpoint = endpoints.get_mut(base_url)?;
        
        Some(EndpointStatus {
            current_rpm_main: endpoint.current_rpm(false), // main agent
            current_rpm_workers: endpoint.current_rpm(true), // workers
            current_tpm_main: endpoint.current_tpm(false),
            current_tpm_workers: endpoint.current_tpm(true),
            blocked_until_main: endpoint.circuit_main.blocked_until,
            blocked_until_workers: endpoint.circuit_workers.blocked_until,
            circuit_open_main: endpoint.circuit_main.circuit_open,
            circuit_open_workers: endpoint.circuit_workers.circuit_open,
            consecutive_429s_main: endpoint.circuit_main.consecutive_429s,
            consecutive_429s_workers: endpoint.circuit_workers.consecutive_429s,
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
                            self.record_rate_limit_error(base_url, is_worker, retry_after);
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
    pub current_tpm_main: u32,
    pub current_tpm_workers: u32,
    pub blocked_until_main: Option<Instant>,
    pub blocked_until_workers: Option<Instant>,
    pub circuit_open_main: bool,
    pub circuit_open_workers: bool,
    pub consecutive_429s_main: u32,
    pub consecutive_429s_workers: u32,
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
