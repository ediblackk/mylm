//! Runtime errors

use std::fmt;

/// LLM capability error
#[derive(Debug, Clone)]
pub struct LLMError {
    pub message: String,
}

impl LLMError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for LLMError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LLM error: {}", self.message)
    }
}

impl std::error::Error for LLMError {}

/// Tool capability error
#[derive(Debug, Clone)]
pub struct ToolError {
    pub message: String,
}

impl ToolError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tool error: {}", self.message)
    }
}

impl std::error::Error for ToolError {}

/// Approval capability error
#[derive(Debug, Clone)]
pub struct ApprovalError {
    pub message: String,
}

impl ApprovalError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for ApprovalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Approval error: {}", self.message)
    }
}

impl std::error::Error for ApprovalError {}

/// Worker capability error
#[derive(Debug, Clone)]
pub struct WorkerError {
    pub message: String,
}

impl WorkerError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Worker error: {}", self.message)
    }
}

impl std::error::Error for WorkerError {}

/// Runtime error (aggregates all capability errors)
#[derive(Debug, Clone)]
pub enum RuntimeError {
    LLM(LLMError),
    Tool(ToolError),
    Approval(ApprovalError),
    Worker(WorkerError),
    Cancelled,
    Unknown(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::LLM(e) => write!(f, "{}", e),
            RuntimeError::Tool(e) => write!(f, "{}", e),
            RuntimeError::Approval(e) => write!(f, "{}", e),
            RuntimeError::Worker(e) => write!(f, "{}", e),
            RuntimeError::Cancelled => write!(f, "Operation cancelled"),
            RuntimeError::Unknown(msg) => write!(f, "Unknown error: {}", msg),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<LLMError> for RuntimeError {
    fn from(e: LLMError) -> Self {
        RuntimeError::LLM(e)
    }
}

impl From<ToolError> for RuntimeError {
    fn from(e: ToolError) -> Self {
        RuntimeError::Tool(e)
    }
}

impl From<ApprovalError> for RuntimeError {
    fn from(e: ApprovalError) -> Self {
        RuntimeError::Approval(e)
    }
}

impl From<WorkerError> for RuntimeError {
    fn from(e: WorkerError) -> Self {
        RuntimeError::Worker(e)
    }
}
