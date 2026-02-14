//! Retry Capabilities
//!
//! Wrappers that add retry logic with exponential backoff to any capability.

use crate::agent::runtime::{
    capability::*,
    context::RuntimeContext,
    error::{LLMError, ToolError},
};
use crate::agent::types::intents::{LLMRequest, ToolCall};
use crate::agent::types::events::{LLMResponse, ToolResult};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use futures::Stream;

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub exponential_base: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 10000,
            exponential_base: 2.0,
        }
    }
}

impl RetryConfig {
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            base_delay_ms: 0,
            max_delay_ms: 0,
            exponential_base: 2.0,
        }
    }
    
    pub fn aggressive() -> Self {
        Self {
            max_retries: 5,
            base_delay_ms: 500,
            max_delay_ms: 30000,
            exponential_base: 2.0,
        }
    }
    
    /// Calculate delay for retry attempt
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }
        
        let delay = self.base_delay_ms as f64 * self.exponential_base.powi(attempt as i32 - 1);
        let delay = delay.min(self.max_delay_ms as f64) as u64;
        Duration::from_millis(delay)
    }
}

/// Retry wrapper for LLM capability
pub struct RetryLLM {
    inner: Arc<dyn LLMCapability>,
    config: RetryConfig,
}

impl RetryLLM {
    pub fn new(inner: Arc<dyn LLMCapability>, config: RetryConfig) -> Self {
        Self { inner, config }
    }
}

impl Capability for RetryLLM {
    fn name(&self) -> &'static str {
        "retry-llm"
    }
}

#[async_trait::async_trait]
impl LLMCapability for RetryLLM {
    async fn complete(
        &self,
        ctx: &RuntimeContext,
        req: LLMRequest,
    ) -> Result<LLMResponse, LLMError> {
        let mut last_error = None;
        
        for attempt in 0..=self.config.max_retries {
            match self.inner.complete(ctx, req.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    // Check if error is retryable
                    if !is_retryable_error(&e.message) {
                        // Non-retryable error - return immediately
                        return Err(e);
                    }
                    
                    last_error = Some(e);
                    if attempt < self.config.max_retries {
                        let delay = self.config.delay_for_attempt(attempt + 1);
                        sleep(delay).await;
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| LLMError::new("All retries failed")))
    }
    
    fn complete_stream<'a>(
        &'a self,
        ctx: &'a RuntimeContext,
        req: LLMRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<crate::agent::runtime::capability::StreamChunk, LLMError>> + Send + 'a>> {
        // For retry wrapper, just delegate to inner stream
        self.inner.complete_stream(ctx, req)
    }
}

/// Retry wrapper for Tool capability
pub struct RetryTools {
    inner: Arc<dyn ToolCapability>,
    config: RetryConfig,
}

impl RetryTools {
    pub fn new(inner: Arc<dyn ToolCapability>, config: RetryConfig) -> Self {
        Self { inner, config }
    }
}

impl Capability for RetryTools {
    fn name(&self) -> &'static str {
        "retry-tools"
    }
}

#[async_trait::async_trait]
impl ToolCapability for RetryTools {
    async fn execute(
        &self,
        ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        let mut last_error = None;
        
        for attempt in 0..=self.config.max_retries {
            match self.inner.execute(ctx, call.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.max_retries {
                        let delay = self.config.delay_for_attempt(attempt + 1);
                        sleep(delay).await;
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| ToolError::new("All retries failed")))
    }
}

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,     // Normal operation
    Open,       // Failing, reject requests
    HalfOpen,   // Testing if recovered
}

/// Circuit breaker for fault tolerance
pub struct CircuitBreaker {
    failure_threshold: u32,
    success_threshold: u32,
    timeout_ms: u64,
    state: std::sync::atomic::AtomicU32,
    failures: std::sync::atomic::AtomicU32,
    successes: std::sync::atomic::AtomicU32,
    last_failure_time: std::sync::Mutex<Option<std::time::Instant>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, success_threshold: u32, timeout_ms: u64) -> Self {
        Self {
            failure_threshold,
            success_threshold,
            timeout_ms,
            state: std::sync::atomic::AtomicU32::new(CircuitState::Closed as u32),
            failures: std::sync::atomic::AtomicU32::new(0),
            successes: std::sync::atomic::AtomicU32::new(0),
            last_failure_time: std::sync::Mutex::new(None),
        }
    }
    
    fn get_state(&self) -> CircuitState {
        match self.state.load(std::sync::atomic::Ordering::Relaxed) {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }
    
    fn set_state(&self, state: CircuitState) {
        self.state.store(state as u32, std::sync::atomic::Ordering::Relaxed);
    }
    
    /// Check if request should be allowed
    pub fn allow_request(&self) -> bool {
        match self.get_state() {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has passed
                if let Ok(last_failure) = self.last_failure_time.lock() {
                    if let Some(time) = *last_failure {
                        if std::time::Instant::now().duration_since(time).as_millis() > self.timeout_ms as u128 {
                            self.set_state(CircuitState::HalfOpen);
                            return true;
                        }
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }
    
    /// Record success
    pub fn record_success(&self) {
        self.failures.store(0, std::sync::atomic::Ordering::Relaxed);
        
        if self.get_state() == CircuitState::HalfOpen {
            let successes = self.successes.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if successes >= self.success_threshold {
                self.set_state(CircuitState::Closed);
                self.successes.store(0, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    
    /// Record failure
    pub fn record_failure(&self) {
        self.successes.store(0, std::sync::atomic::Ordering::Relaxed);
        
        if let Ok(mut last_failure) = self.last_failure_time.lock() {
            *last_failure = Some(std::time::Instant::now());
        }
        
        if self.get_state() == CircuitState::HalfOpen {
            self.set_state(CircuitState::Open);
            return;
        }
        
        let failures = self.failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if failures >= self.failure_threshold {
            self.set_state(CircuitState::Open);
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(5, 3, 30000)
    }
}

/// Circuit breaker wrapper for any capability
pub struct CircuitBreakerLLM {
    inner: Arc<dyn LLMCapability>,
    breaker: CircuitBreaker,
}

impl CircuitBreakerLLM {
    pub fn new(inner: Arc<dyn LLMCapability>) -> Self {
        Self {
            inner,
            breaker: CircuitBreaker::default(),
        }
    }
    
    pub fn with_breaker(inner: Arc<dyn LLMCapability>, breaker: CircuitBreaker) -> Self {
        Self { inner, breaker }
    }
}

impl Capability for CircuitBreakerLLM {
    fn name(&self) -> &'static str {
        "circuit-breaker-llm"
    }
}

#[async_trait::async_trait]
impl LLMCapability for CircuitBreakerLLM {
    async fn complete(
        &self,
        ctx: &RuntimeContext,
        req: LLMRequest,
    ) -> Result<LLMResponse, LLMError> {
        if !self.breaker.allow_request() {
            return Err(LLMError::new("Circuit breaker is open"));
        }
        
        match self.inner.complete(ctx, req).await {
            Ok(response) => {
                self.breaker.record_success();
                Ok(response)
            }
            Err(e) => {
                self.breaker.record_failure();
                Err(e)
            }
        }
    }
    
    fn complete_stream<'a>(
        &'a self,
        ctx: &'a RuntimeContext,
        req: LLMRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<crate::agent::runtime::capability::StreamChunk, LLMError>> + Send + 'a>> {
        self.inner.complete_stream(ctx, req)
    }
}

/// Combined retry + circuit breaker wrapper
pub struct ResilientLLM {
    retry: RetryLLM,
    breaker: CircuitBreaker,
}

impl ResilientLLM {
    pub fn new(inner: Arc<dyn LLMCapability>, retry_config: RetryConfig) -> Self {
        Self {
            retry: RetryLLM::new(inner, retry_config),
            breaker: CircuitBreaker::default(),
        }
    }
}

impl Capability for ResilientLLM {
    fn name(&self) -> &'static str {
        "resilient-llm"
    }
}

#[async_trait::async_trait]
impl LLMCapability for ResilientLLM {
    async fn complete(
        &self,
        ctx: &RuntimeContext,
        req: LLMRequest,
    ) -> Result<LLMResponse, LLMError> {
        if !self.breaker.allow_request() {
            return Err(LLMError::new("Circuit breaker is open"));
        }
        
        match self.retry.complete(ctx, req).await {
            Ok(response) => {
                self.breaker.record_success();
                Ok(response)
            }
            Err(e) => {
                self.breaker.record_failure();
                Err(e)
            }
        }
    }
    
    fn complete_stream<'a>(
        &'a self,
        ctx: &'a RuntimeContext,
        req: LLMRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<crate::agent::runtime::capability::StreamChunk, LLMError>> + Send + 'a>> {
        self.retry.complete_stream(ctx, req)
    }
}

/// Check if an error is retryable based on error message patterns
/// Non-retryable: 4xx client errors (400, 401, 403, etc.)
/// Retryable: 429 rate limit, 5xx server errors, network errors
fn is_retryable_error(error_msg: &str) -> bool {
    let msg = error_msg.to_lowercase();
    
    // Non-retryable client errors (4xx)
    if msg.contains("400 bad request") 
        || msg.contains("401 unauthorized")
        || msg.contains("403 forbidden")
        || msg.contains("404 not found")
        || msg.contains("422 unprocessable")
        || msg.contains("invalid request")
        || msg.contains("context length")  // Token limit errors
    {
        return false;
    }
    
    // Retryable errors
    if msg.contains("429")
        || msg.contains("rate limit")
        || msg.contains("500")
        || msg.contains("502")
        || msg.contains("503")
        || msg.contains("504")
        || msg.contains("timeout")
        || msg.contains("connection")
        || msg.contains("network")
        || msg.contains("server error")
    {
        return true;
    }
    
    // Default to retryable for unknown errors
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_retry_config_delay() {
        let config = RetryConfig::default();
        
        assert_eq!(config.delay_for_attempt(0).as_millis(), 0);
        assert_eq!(config.delay_for_attempt(1).as_millis(), 100);
        assert_eq!(config.delay_for_attempt(2).as_millis(), 200);
        assert_eq!(config.delay_for_attempt(3).as_millis(), 400);
    }
    
    #[test]
    fn test_circuit_breaker() {
        let breaker = CircuitBreaker::new(3, 2, 1000);
        
        // Initially closed
        assert!(breaker.allow_request());
        
        // Record failures
        breaker.record_failure();
        breaker.record_failure();
        assert!(breaker.allow_request()); // Still closed
        
        breaker.record_failure();
        assert!(!breaker.allow_request()); // Now open
        
        // Wait for timeout
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(breaker.allow_request()); // Half-open
        
        // Record success
        breaker.record_success();
        breaker.record_success();
        assert!(breaker.allow_request()); // Back to closed
    }
    
    #[tokio::test]
    async fn test_retry_llm() {
        use crate::agent::runtime::graph::StubLLM;
        
        let stub = Arc::new(StubLLM);
        let retry_llm = RetryLLM::new(stub, RetryConfig::default());
        
        let ctx = RuntimeContext::new();
        let context = crate::agent::types::intents::Context::new("test".to_string());
        let req = LLMRequest {
            context,
            max_tokens: None,
            temperature: None,
            model: None,
            response_format: None,
            stream: false,
        };
        
        let result = retry_llm.complete(&ctx, req).await;
        assert!(result.is_ok());
    }
}
