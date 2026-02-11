//! Error types and retry utilities for PaCoRe.
//!
//! Defines the error enum for API failures, timeouts, and parsing errors,
//! along with exponential backoff retry logic.
//!
//! # Main Types
//! - `Error`: Error enum for PaCoRe operations
//! - `retry`: Async retry function with exponential backoff

use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {status} - {message}")]
    Api { status: u16, message: String },
    #[error("Timeout after {0:?}")]
    Timeout(Duration),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Template error: {0}")]
    Template(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Internal error: {0}")]
    Internal(String),
}

pub async fn retry<F, Fut, T>(
    f: F,
    max_retries: usize,
    base_delay_ms: u64,
) -> Result<T, Error>
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<T, Error>> + Send,
    T: Send,
{
    for attempt in 0..max_retries {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if attempt == max_retries - 1 {
                    return Err(e);
                }
                // Only retry on transient errors (can be expanded)
                let jitter = rand::random::<u64>() % 200;
                let delay = Duration::from_millis(base_delay_ms * 2u64.pow(attempt as u32) + jitter);
                tokio::time::sleep(delay).await;
            }
        }
    }
    unreachable!()
}
