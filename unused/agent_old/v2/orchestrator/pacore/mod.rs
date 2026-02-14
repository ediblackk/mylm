//! PaCoRe (Parallel Consensus Reasoning) - Multi-round LLM inference engine.
//!
//! Implements an experimental approach to improve LLM responses through
//! parallel sampling and synthesis across multiple rounds.
//!
//! # Main Components
//! - `client`: HTTP client for LLM API communication
//! - `error`: Error types and retry logic
//! - `exp`: Core experiment orchestration (Exp)
//! - `model`: Request/response data models
//! - `template`: Prompt templating engine
//! - `utils`: JSONL file utilities

pub mod client;
pub mod error;
pub mod exp;
pub mod model;
pub mod template;
pub mod utils;

pub use client::ChatClient;
pub use error::Error;
pub use exp::{Exp, ProcessedResult, RoundResult, PaCoReProgressEvent};
pub use model::{ChatRequest, ChatResponse, Message};
pub use template::TemplateEngine;
pub use utils::{load_jsonl, save_jsonl};
