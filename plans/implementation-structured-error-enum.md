# Implementation Guide: Structured Error Enum for Mylm

## Overview

This guide documents how Codex implements structured error handling using typed enums, based on patterns from the Codex codebase. This approach replaces `anyhow::Error` with a custom error enum that provides:

- **Type-safe error handling** - Compile-time checking of error cases
- **Client-friendly error codes** - Machine-readable error categories for UI/telemetry
- **Rich context** - Errors carry structured data for better diagnostics
- **Telemetry integration** - Categorized errors for monitoring and metrics
- **User-friendly messages** - Localized, actionable error descriptions

---

## 1. Error Architecture

### Why Structured Errors?

Codex moved from `anyhow::Error` to a structured error enum for production-grade error handling:

```rust
// Before: anyhow::Error loses type information
pub async fn process_turn() -> anyhow::Result<()> {
    // ... errors are opaque at compile time
}

// After: Structured error enum
pub async fn process_turn() -> Result<(), MylmError> {
    // ... each error variant is explicit and catchable
}
```

**Benefits:**

1. **Client handling** - Frontend can display appropriate UI based on error category
2. **Telemetry** - Errors are automatically categorized for metrics
3. **Retry logic** - Programmatic determination of retryable vs fatal errors
4. **Debugging** - Errors carry structured context (file paths, model names, etc.)
5. **User experience** - Tailored, helpful error messages

### Error Categorization

Codex errors fall into these categories:

| Category | Examples | User Action |
|----------|----------|-------------|
| **Provider/API** | `ContextWindowExceeded`, `ModelCap`, `QuotaExceeded` | Try different model, upgrade plan, wait |
| **Tool/Execution** | `SandboxError`, `Timeout`, `Spawn` | Check permissions, simplify command |
| **Permission/Security** | `Unauthorized`, `RefreshTokenFailed` | Re-authenticate, check API keys |
| **Validation** | `InvalidRequest`, `BadRequest`, `InvalidImageRequest` | Fix input, check file format |
| **System** | `InternalServerError`, `Io`, `Json` | Retry, report bug |
| **Transient** | `Stream`, `ConnectionFailed`, `ResponseStreamFailed` | Automatic retry with backoff |

---

## 2. Codex Error Enum Structure

### Core Error Enum: `CodexErr`

From [`codex-rs/core/src/error.rs`](codex-rs/core/src/error.rs:60-182):

```rust
use thiserror::Error;
use std::time::Duration;

#[derive(Error, Debug)]
pub enum CodexErr {
    /// Turn was aborted (Ctrl-C or interrupt)
    #[error("turn aborted. Something went wrong? Hit `/feedback` to report the issue.")]
    TurnAborted,

    /// Stream disconnected before completion (retryable)
    #[error("stream disconnected before completion: {0}")]
    Stream(String, Option<Duration>),

    /// Model context window exceeded
    #[error(
        "Codex ran out of room in the model's context window. Start a new thread or clear earlier history before retrying."
    )]
    ContextWindowExceeded,

    /// Thread not found
    #[error("no thread with id: {0}")]
    ThreadNotFound(ThreadId),

    /// Agent thread limit reached
    #[error("agent thread limit reached (max {max_threads})")]
    AgentLimitReached { max_threads: usize },

    /// Timeout waiting for child process
    #[error("timeout waiting for child process to exit")]
    Timeout,

    /// Failed to spawn child process
    #[error("spawn failed: child stdout/stderr not captured")]
    Spawn,

    /// User interrupted with Ctrl-C
    #[error("interrupted (Ctrl-C). Something went wrong? Hit `/feedback` to report the issue.")]
    Interrupted,

    /// Unexpected HTTP status
    #[error("{0}")]
    UnexpectedStatus(UnexpectedResponseError),

    /// Invalid request
    #[error("{0}")]
    InvalidRequest(String),

    /// Invalid image (poisoned/unsafe)
    #[error("Image poisoning")]
    InvalidImageRequest(),

    /// Usage limit reached (plan-specific)
    #[error("{0}")]
    UsageLimitReached(UsageLimitReachedError),

    /// Model capacity error
    #[error("{0}")]
    ModelCap(ModelCapError),

    /// Response stream failed
    #[error("{0}")]
    ResponseStreamFailed(ResponseStreamFailed),

    /// Connection failed
    #[error("{0}")]
    ConnectionFailed(ConnectionFailedError),

    /// Quota exceeded (billing)
    #[error("Quota exceeded. Check your plan and billing details.")]
    QuotaExceeded,

    /// Usage not included in plan
    #[error(
        "To use Codex with your ChatGPT plan, upgrade to Plus: https://chatgpt.com/explore/plus."
    )]
    UsageNotIncluded,

    /// Internal server error (5xx)
    #[error("We're currently experiencing high demand, which may cause temporary errors.")]
    InternalServerError,

    /// Retry limit exceeded
    #[error("{0}")]
    RetryLimit(RetryLimitReachedError),

    /// Agent loop died unexpectedly
    #[error("internal error; agent loop died unexpectedly")]
    InternalAgentDied,

    /// Sandbox error
    #[error("sandbox error: {0}")]
    Sandbox(#[from] SandboxErr),

    /// Landlock sandbox executable not provided
    #[error("codex-linux-sandbox was required but not provided")]
    LandlockSandboxExecutableNotProvided,

    /// Unsupported operation
    #[error("unsupported operation: {0}")]
    UnsupportedOperation(String),

    /// Refresh token failed
    #[error("{0}")]
    RefreshTokenFailed(RefreshTokenFailedError),

    /// Fatal error
    #[error("Fatal error: {0}")]
    Fatal(String),

    // Automatic conversions for common external error types
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[cfg(target_os = "linux")]
    #[error(transparent)]
    LandlockRuleset(#[from] landlock::RulesetError),

    #[cfg(target_os = "linux")]
    #[error(transparent)]
    LandlockPathFd(#[from] landlock::PathFdError),

    #[error(transparent)]
    TokioJoin(#[from] tokio::task::JoinError),

    #[error("{0}")]
    EnvVar(EnvVarError),
}
```

### Key Design Patterns

1. **Simple variants** for static messages: `ContextWindowExceeded`, `QuotaExceeded`
2. **Data-carrying variants** for context: `ThreadNotFound(ThreadId)`, `UsageLimitReached(UsageLimitReachedError)`
3. **Transparent wrapping** for external errors: `#[error(transparent)] Io(io::Error)`
4. **Custom error types** for complex scenarios: `UnexpectedResponseError`, `UsageLimitReachedError`

### Nested Error Types

Complex errors with multiple fields:

```rust
/// Unexpected HTTP response error
#[derive(Debug)]
pub struct UnexpectedResponseError {
    pub status: StatusCode,
    pub body: String,
    pub url: Option<String>,
    pub cf_ray: Option<String>,
    pub request_id: Option<String>,
}

impl std::fmt::Display for UnexpectedResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(friendly) = self.friendly_message() {
            write!(f, "{friendly}")
        } else {
            let status = self.status;
            let body = self.display_body();
            let mut message = format!("unexpected status {status}: {body}");
            if let Some(url) = &self.url {
                message.push_str(&format!(", url: {url}"));
            }
            // ... more metadata
            write!(f, "{message}")
        }
    }
}
```

---

## 3. Error Context & Metadata

### Carrying Rich Context

Codex errors carry structured data for programmatic handling:

```rust
/// Usage limit error with plan-specific messaging
#[derive(Debug)]
pub struct UsageLimitReachedError {
    pub(crate) plan_type: Option<PlanType>,
    pub(crate) resets_at: Option<DateTime<Utc>>,
    pub(crate) rate_limits: Option<RateLimitSnapshot>,
    pub(crate) promo_message: Option<String>,
}

impl std::fmt::Display for UsageLimitReachedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(promo_message) = &self.promo_message {
            return write!(
                f,
                "You've hit your usage limit. {promo_message},{}",
                retry_suffix_after_or(self.resets_at.as_ref())
            );
        }

        let message = match self.plan_type.as_ref() {
            Some(PlanType::Known(KnownPlan::Plus)) => format!(
                "You've hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/codex/settings/usage to purchase more credits{}",
                retry_suffix_after_or(self.resets_at.as_ref())
            ),
            Some(PlanType::Known(KnownPlan::Free)) => format!(
                "You've hit your usage limit. Upgrade to Plus to continue using Codex (https://chatgpt.com/explore/plus),{}",
                retry_suffix_after_or(self.resets_at.as_ref())
            ),
            // ... other plans
            _ => format!("You've hit your usage limit.{}", retry_suffix(self.resets_at.as_ref())),
        };

        write!(f, "{message}")
    }
}
```

**Key insight:** Errors carry *both* machine-readable fields (`plan_type`, `resets_at`) and generate user-friendly messages.

### Error Message Truncation

Protect UI from excessively large error messages:

```rust
/// Limit UI error messages to a reasonable size
const ERROR_MESSAGE_UI_MAX_BYTES: usize = 2 * 1024; // 2 KiB

pub fn get_error_message_ui(e: &CodexErr) -> String {
    let message = match e {
        CodexErr::Sandbox(SandboxErr::Denied { output }) => {
            // Special handling for sandbox errors
            output.aggregated_output.text.clone()
        }
        _ => e.to_string(),
    };

    truncate_text(&message, TruncationPolicy::Bytes(ERROR_MESSAGE_UI_MAX_BYTES))
}
```

---

## 4. Error Conversion & Propagation

### `From` Trait Implementations

Automatic conversion from lower-level errors:

```rust
impl From<RateLimitError> for ApiError {
    fn from(err: RateLimitError) -> Self {
        Self::RateLimit(err.to_string())
    }
}

impl From<CancelErr> for CodexErr {
    fn from(_: CancelErr) -> Self {
        CodexErr::TurnAborted
    }
}

// Via #[error(transparent)] attribute
#[derive(Error, Debug)]
pub enum CodexErr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
```

### Error Mapping at Boundaries

Convert internal errors to client-facing protocol errors:

```rust
impl CodexErr {
    /// Translate core error to client-facing protocol error
    pub fn to_codex_protocol_error(&self) -> CodexErrorInfo {
        match self {
            CodexErr::ContextWindowExceeded => CodexErrorInfo::ContextWindowExceeded,
            CodexErr::UsageLimitReached(_)
            | CodexErr::QuotaExceeded
            | CodexErr::UsageNotIncluded => CodexErrorInfo::UsageLimitExceeded,
            CodexErr::ModelCap(err) => CodexErrorInfo::ModelCap {
                model: err.model.clone(),
                reset_after_seconds: err.reset_after_seconds,
            },
            CodexErr::RetryLimit(_) => CodexErrorInfo::ResponseTooManyFailedAttempts {
                http_status_code: self.http_status_code_value(),
            },
            CodexErr::ConnectionFailed(_) => CodexErrorInfo::HttpConnectionFailed {
                http_status_code: self.http_status_code_value(),
            },
            CodexErr::ResponseStreamFailed(_) => CodexErrorInfo::ResponseStreamConnectionFailed {
                http_status_code: self.http_status_code_value(),
            },
            CodexErr::RefreshTokenFailed(_) => CodexErrorInfo::Unauthorized,
            CodexErr::SessionConfiguredNotFirstEvent
            | CodexErr::InternalServerError
            | CodexErr::InternalAgentDied => CodexErrorInfo::InternalServerError,
            CodexErr::UnsupportedOperation(_)
            | CodexErr::ThreadNotFound(_)
            | CodexErr::AgentLimitReached { .. } => CodexErrorInfo::BadRequest,
            CodexErr::Sandbox(_) => CodexErrorInfo::SandboxError,
            _ => CodexErrorInfo::Other,
        }
    }

    /// Convert to error event for streaming to client
    pub fn to_error_event(&self, message_prefix: Option<String>) -> ErrorEvent {
        let error_message = self.to_string();
        let message: String = match message_prefix {
            Some(prefix) => format!("{prefix}: {error_message}"),
            None => error_message,
        };
        ErrorEvent {
            message,
            codex_error_info: Some(self.to_codex_protocol_error()),
        }
    }
}
```

---

## 5. Telemetry Integration

### Error Classification for Metrics

Codex uses error categories for monitoring:

```rust
impl CodexErr {
    pub fn is_retryable(&self) -> bool {
        match self {
            // Non-retryable errors (fatal, user action required)
            CodexErr::TurnAborted
            | CodexErr::Interrupted
            | CodexErr::EnvVar(_)
            | CodexErr::Fatal(_)
            | CodexErr::UsageNotIncluded
            | CodexErr::QuotaExceeded
            | CodexErr::InvalidImageRequest()
            | CodexErr::InvalidRequest(_)
            | CodexErr::RefreshTokenFailed(_)
            | CodexErr::UnsupportedOperation(_)
            | CodexErr::Sandbox(_)
            | CodexErr::LandlockSandboxExecutableNotProvided
            | CodexErr::RetryLimit(_)
            | CodexErr::ContextWindowExceeded
            | CodexErr::ThreadNotFound(_)
            | CodexErr::AgentLimitReached { .. }
            | CodexErr::Spawn
            | CodexErr::SessionConfiguredNotFirstEvent
            | CodexErr::UsageLimitReached(_)
            | CodexErr::ModelCap(_) => false,

            // Retryable errors (transient, network, system)
            CodexErr::Stream(..)
            | CodexErr::Timeout
            | CodexErr::UnexpectedStatus(_)
            | CodexErr::ResponseStreamFailed(_)
            | CodexErr::ConnectionFailed(_)
            | CodexErr::InternalServerError
            | CodexErr::InternalAgentDied
            | CodexErr::Io(_)
            | CodexErr::Json(_)
            | CodexErr::TokioJoin(_) => true,

            #[cfg(target_os = "linux")]
            CodexErr::LandlockRuleset(_) | CodexErr::LandlockPathFd(_) => false,
        }
    }
}
```

### Telemetry Hooks

From [`codex-rs/codex-api/src/telemetry.rs`](codex-rs/codex-api/src/telemetry.rs:68-98):

```rust
pub(crate) async fn run_with_request_telemetry<T, F, Fut>(
    policy: RetryPolicy,
    telemetry: Option<Arc<dyn RequestTelemetry>>,
    make_request: impl FnMut() -> Request,
    send: F,
) -> Result<T, TransportError>
where
    T: WithStatus,
    F: Clone + Fn(Request) -> Fut,
    Fut: Future<Output = Result<T, TransportError>>,
{
    run_with_retry(policy, make_request, move |req, attempt| {
        let telemetry = telemetry.clone();
        let send = send.clone();
        async move {
            let start = Instant::now();
            let result = send(req).await;
            if let Some(t) = telemetry.as_ref() {
                let (status, err) = match &result {
                    Ok(resp) => (Some(resp.status()), None),
                    Err(err) => (http_status(err), Some(err)),
                };
                t.on_request(attempt, status, err, start.elapsed());
            }
            result
        }
    })
    .await
}
```

---

## 6. Client-Side Error Handling

### Protocol Error Enum: `CodexErrorInfo`

From [`codex-rs/protocol/src/protocol.rs`](codex-rs/protocol/src/protocol.rs:952-984):

```rust
/// Codex errors that we expose to clients.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum CodexErrorInfo {
    ContextWindowExceeded,
    UsageLimitExceeded,
    ModelCap {
        model: String,
        reset_after_seconds: Option<u64>,
    },
    HttpConnectionFailed {
        http_status_code: Option<u16>,
    },
    /// Failed to connect to the response SSE stream
    ResponseStreamConnectionFailed {
        http_status_code: Option<u16>,
    },
    InternalServerError,
    Unauthorized,
    BadRequest,
    SandboxError,
    /// The response SSE stream disconnected before completion
    ResponseStreamDisconnected {
        http_status_code: Option<u16>,
    },
    /// Reached the retry limit for responses
    ResponseTooManyFailedAttempts {
        http_status_code: Option<u16>,
    },
    ThreadRollbackFailed,
    Other,
}
```

**Key points:**

- `#[ts(export_to = "v2/")]` generates TypeScript types for frontend
- `#[serde(rename_all = "snake_case")]` ensures JSON compatibility
- Variants carry minimal structured data for client consumption
- `Other` is a catch-all for unknown errors

### Error Event Structure

```rust
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, TS)]
pub struct ErrorEvent {
    pub message: String,
    #[serde(default)]
    pub codex_error_info: Option<CodexErrorInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, TS)]
pub struct StreamErrorEvent {
    pub message: String,
    #[serde(default)]
    pub codex_error_info: Option<CodexErrorInfo>,
    /// Optional details about the underlying stream failure
    #[serde(default)]
    pub additional_details: Option<String>,
}
```

### Frontend Error Handling (TypeScript)

Generated TypeScript types enable type-safe error handling:

```typescript
// Generated from CodexErrorInfo enum
type CodexErrorInfo = 
  | { type: 'context_window_exceeded' }
  | { type: 'usage_limit_exceeded' }
  | { type: 'model_cap'; model: string; reset_after_seconds?: number | null }
  | { type: 'http_connection_failed'; http_status_code?: number | null }
  | { type: 'response_stream_connection_failed'; http_status_code?: number | null }
  | { type: 'internal_server_error' }
  | { type: 'unauthorized' }
  | { type: 'bad_request' }
  | { type: 'sandbox_error' }
  | { type: 'response_stream_disconnected'; http_status_code?: number | null }
  | { type: 'response_too_many_failed_attempts'; http_status_code?: number | null }
  | { type: 'thread_rollback_failed' }
  | { type: 'other' };

// Type-safe error handling
function handleError(event: ErrorEvent) {
  switch (event.codex_error_info?.type) {
    case 'context_window_exceeded':
      // Suggest starting new thread
      showError('Context full. Start a new conversation.');
      break;
    case 'usage_limit_exceeded':
      // Link to billing
      showError('Usage limit reached. Upgrade your plan.');
      break;
    case 'model_cap':
      // Suggest alternative model
      showError(`Model ${event.codex_error_info.model} is at capacity.`);
      break;
    default:
      showError(event.message);
  }
}
```

---

## 7. Best Practices & Lessons Learned

### What Makes a Good Error Enum

1. **Clear variant names** - Use descriptive, consistent naming (`ContextWindowExceeded`, not `CtxFull`)
2. **Carry context** - Include relevant data in variants (file paths, IDs, limits)
3. **User-friendly messages** - `#[error]` attributes should be actionable
4. **Categorize for telemetry** - Provide `is_retryable()` or similar for monitoring
5. **Separate internal vs external** - Internal errors can be detailed; external errors should avoid leaking internals

### Error Variant Design

```rust
// ✅ GOOD: Clear, carries context
#[error("failed to read config file: {path}")]
ConfigFileReadError { path: PathBuf },

// ❌ AVOID: Too generic, no context
#[error("config error")]
ConfigError,

// ✅ GOOD: Plan-specific messaging with structured data
#[error("usage limit reached for plan: {plan_type}")]
UsageLimitReached { plan_type: String },

// ❌ AVOID: Opaque error with no programmatic handling
#[error("operation failed")]
OperationFailed,
```

### Migration Strategy from `anyhow::Error`

**Phase 1: Define error enum**

```rust
#[derive(Error, Debug)]
pub enum MylmError {
    #[error("network error: {0}")]
    Network(NetworkError),
    #[error("validation error: {0}")]
    Validation(ValidationError),
    #[error("internal error: {0}")]
    Internal(InternalError),
    #[error(transparent)]
    Other(#[from] anyhow::Error), // Temporary catch-all
}
```

**Phase 2: Implement `From` for external crates**

```rust
impl From<reqwest::Error> for MylmError {
    fn from(err: reqwest::Error) -> Self {
        MylmError::Network(NetworkError::from(err))
    }
}
```

**Phase 3: Replace `anyhow::Result` gradually**

```rust
// Before
pub async fn fetch_data() -> anyhow::Result<Data> { ... }

// After
pub async fn fetch_data() -> Result<Data, MylmError> { ... }
```

**Phase 4: Remove `anyhow` dependency entirely**

### Common Pitfalls

1. **Error explosion** - Too many variants makes maintenance hard
   - **Solution:** Group related errors into sub-enums or structs

2. **Loss of context** - Wrapping errors without preserving source
   - **Solution:** Use `#[source]` attribute or `thiserror::Error` with `#[error(transparent)]`

3. **Sensitive data leakage** - Error messages exposing secrets
   - **Solution:** Filter sensitive fields in `Display` impl, use `redact` crate

4. **Breaking changes** - Modifying error enum variants
   - **Solution:** Keep old variants as `#[deprecated]`, add new ones; use `#[non_exhaustive]`

5. **Performance** - Large error structs copied frequently
   - **Solution:** Use `Arc<ErrorData>` for heavy context, or `Box<Error>`

### Sensitive Data Filtering

```rust
impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidToken { token, .. } => {
                // Never log the actual token
                write!(f, "invalid authentication token")
            }
            AuthError::PermissionDenied { user, resource } => {
                // User might be sensitive depending on context
                write!(f, "permission denied for user {user} on {resource}")
            }
        }
    }
}
```

---

## 8. Complete Example: MylmError Enum

Based on Codex patterns, here's a production-ready error enum for Mylm:

```rust
use thiserror::Error;
use std::time::Duration;
use chrono::{DateTime, Utc};

/// Primary error type for Mylm operations
#[derive(Error, Debug)]
pub enum MylmError {
    /// Authentication/authorization errors
    #[error("unauthorized: {message}")]
    Unauthorized { message: String },

    /// Invalid API key or token
    #[error("invalid credentials: {reason}")]
    InvalidCredentials { reason: String },

    /// Token expired
    #[error("authentication token expired")]
    TokenExpired,

    /// Rate limit exceeded
    #[error("rate limit exceeded: {limit_type}")]
    RateLimitExceeded { limit_type: String },

    /// Quota/billing limit reached
    #[error("quota exceeded. Plan: {plan_type}, resets: {resets_at}")]
    QuotaExceeded {
        plan_type: String,
        resets_at: Option<DateTime<Utc>>,
    },

    /// Model context window full
    #[error("context window exceeded. Max: {max_tokens}, Used: {used_tokens}")]
    ContextWindowExceeded {
        max_tokens: usize,
        used_tokens: usize,
    },

    /// Model is at capacity/unavailable
    #[error("model {model} is at capacity. Try again in {retry_after}")]
    ModelCapacity {
        model: String,
        retry_after: Option<Duration>,
    },

    /// Invalid user input
    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    /// File/path error
    #[error("file error: {path} - {message}")]
    FileError { path: String, message: String },

    /// Network/connection error
    #[error("connection failed: {message}")]
    ConnectionFailed { message: String },

    /// Timeout
    #[error("operation timed out after {duration:?}")]
    Timeout { duration: Duration },

    /// Internal system error
    #[error("internal error: {message}")]
    Internal { message: String },

    /// Service unavailable (maintenance, 503)
    #[error("service temporarily unavailable. Retry after {retry_after:?}")]
    ServiceUnavailable { retry_after: Option<Duration> },

    /// Tool execution error
    #[error("tool execution failed: {tool_name} - {error}")]
    ToolExecutionFailed { tool_name: String, error: String },

    /// Sandbox/permission denied
    #[error("sandbox denied: {reason}")]
    SandboxDenied { reason: String },

    /// Stream disconnected (retryable)
    #[error("stream disconnected: {reason}")]
    StreamDisconnected { reason: String },

    // Automatic conversions
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Timezone(#[from] chrono::ParseError),
}

impl MylmError {
    /// Check if error is retryable (transient)
    pub fn is_retryable(&self) -> bool {
        match self {
            MylmError::ConnectionFailed { .. } => true,
            MylmError::Timeout { .. } => true,
            MylmError::StreamDisconnected { .. } => true,
            MylmError::ServiceUnavailable { .. } => true,
            MylmError::ModelCapacity { .. } => true,
            MylmError::RateLimitExceeded { .. } => true,
            MylmError::Internal { .. } => false,
            MylmError::Unauthorized { .. } => false,
            MylmError::InvalidCredentials { .. } => false,
            MylmError::TokenExpired => false,
            MylmError::QuotaExceeded { .. } => false,
            MylmError::ContextWindowExceeded { .. } => false,
            MylmError::InvalidInput { .. } => false,
            MylmError::FileError { .. } => false,
            MylmError::ToolExecutionFailed { .. } => false,
            MylmError::SandboxDenied { .. } => false,
            MylmError::Io(_) => true,
            MylmError::Json(_) => false,
            MylmError::Reqwest(_) => true,
            MylmError::Timezone(_) => false,
        }
    }

    /// Convert to client-facing error code
    pub fn to_client_error(&self) -> MylmClientError {
        match self {
            MylmError::Unauthorized { .. } => MylmClientError::Unauthorized,
            MylmError::InvalidCredentials { .. } => MylmClientError::InvalidCredentials,
            MylmError::TokenExpired => MylmClientError::TokenExpired,
            MylmError::RateLimitExceeded { .. } => MylmClientError::RateLimitExceeded,
            MylmError::QuotaExceeded { .. } => MylmClientError::QuotaExceeded,
            MylmError::ContextWindowExceeded { .. } => MylmClientError::ContextWindowExceeded,
            MylmError::ModelCapacity { .. } => MylmClientError::ModelUnavailable,
            MylmError::InvalidInput { .. } => MylmClientError::InvalidInput,
            MylmError::FileError { .. } => MylmClientError::FileError,
            MylmError::ConnectionFailed { .. } => MylmClientError::ConnectionFailed,
            MylmError::Timeout { .. } => MylmClientError::Timeout,
            MylmError::Internal { .. } => MylmClientError::InternalServerError,
            MylmError::ServiceUnavailable { .. } => MylmClientError::ServiceUnavailable,
            MylmError::ToolExecutionFailed { .. } => MylmClientError::ToolExecutionFailed,
            MylmError::SandboxDenied { .. } => MylmClientError::SandboxDenied,
            MylmError::StreamDisconnected { .. } => MylmClientError::StreamDisconnected,
            MylmError::Io(_) => MylmClientError::InternalServerError,
            MylmError::Json(_) => MylmClientError::InvalidInput,
            MylmError::Reqwest(_) => MylmClientError::ConnectionFailed,
            MylmError::Timezone(_) => MylmClientError::InvalidInput,
        }
    }
}

/// Client-facing error codes (simplified for frontend)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum MylmClientError {
    Unauthorized,
    InvalidCredentials,
    TokenExpired,
    RateLimitExceeded,
    QuotaExceeded,
    ContextWindowExceeded,
    ModelUnavailable,
    InvalidInput,
    FileError,
    ConnectionFailed,
    Timeout,
    InternalServerError,
    ServiceUnavailable,
    ToolExecutionFailed,
    SandboxDenied,
    StreamDisconnected,
    Unknown,
}
```

---

## 9. Testing Error Handling

### Unit Tests for Error Formatting

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_exceeded_error_message() {
        let err = MylmError::QuotaExceeded {
            plan_type: "pro".to_string(),
            resets_at: Some(Utc::now() + chrono::Duration::hours(1)),
        };
        assert!(err.to_string().contains("quota exceeded"));
        assert!(err.to_string().contains("pro"));
    }

    #[test]
    fn test_retryable_errors() {
        assert!(MylmError::ConnectionFailed { message: "timeout".to_string() }.is_retryable());
        assert!(MylmError::Timeout { duration: Duration::from_secs(30) }.is_retryable());
        assert!(!MylmError::Unauthorized { message: "bad token".to_string() }.is_retryable());
    }

    #[test]
    fn test_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let mylm_err: MylmError = io_err.into();
        assert!(matches!(mylm_err, MylmError::Io(_)));
    }
}
```

### Integration Testing with Error Events

```rust
#[tokio::test]
async fn test_error_propagation() {
    let result = process_turn().await;
    match result {
        Ok(_) => panic!("expected error"),
        Err(MylmError::ContextWindowExceeded { max_tokens, used_tokens }) => {
            assert!(used_tokens > max_tokens);
        }
        Err(_) => panic!("unexpected error variant"),
    }
}
```

---

## 10. Summary & Checklist

### Implementation Checklist

- [ ] Define error enum with `#[derive(Error)]` from `thiserror`
- [ ] Use `#[error("...")]` for user-friendly messages
- [ ] Include structured data in variants where needed
- [ ] Implement `From<T>` for external error types
- [ ] Add `is_retryable()` method for transient error detection
- [ ] Create client-facing error codes (separate enum or mapping)
- [ ] Implement error-to-event conversion for streaming
- [ ] Add telemetry hooks (error categorization)
- [ ] Write unit tests for error formatting and classification
- [ ] Document error variants and when they occur
- [ ] Review for sensitive data leakage in error messages
- [ ] Consider `#[non_exhaustive]` for public error enums

### Key Takeaways from Codex

1. **Structured > opaque** - Typed errors enable better tooling and UX
2. **Context matters** - Carry relevant data (IDs, paths, limits) in errors
3. **Separate concerns** - Internal error enum vs client-facing error codes
4. **Telemetry by design** - Build error classification into the enum
5. **User-friendly messages** - `Display` impls should be actionable
6. **Test error paths** - Errors are critical paths; test them thoroughly

---

## References

- Codex Core Error: [`codex-rs/core/src/error.rs`](codex-rs/core/src/error.rs)
- API Error: [`codex-rs/codex-api/src/error.rs`](codex-rs/codex-api/src/error.rs)
- Protocol Errors: [`codex-rs/protocol/src/protocol.rs`](codex-rs/protocol/src/protocol.rs:952-984)
- Telemetry: [`codex-rs/codex-api/src/telemetry.rs`](codex-rs/codex-api/src/telemetry.rs)
- ExecPolicy Errors: [`codex-rs/execpolicy/src/error.rs`](codex-rs/execpolicy/src/error.rs)
- Metrics Errors: [`codex-rs/otel/src/metrics/error.rs`](codex-rs/otel/src/metrics/error.rs)

---

*This documentation is based on Codex's production implementation. Adapt patterns to your specific needs while maintaining the core principles of structured, type-safe error handling.*