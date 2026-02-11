//! Reasoning Engine System - Pluggable reasoning strategies for agents
//!
//! This module provides a trait-based system for different reasoning approaches:
//! - PaCoRe (Parallel Consensus Reasoning): Multi-round parallel inference
//! - Future: TreeOfThought, SelfConsistency, ChainOfThought, etc.
//!
//! # Architecture
//! ```
//! AgentOrchestrator -> ReasoningEngine -> Stream<ReasoningEvent>
//! ```
//!
//! The reasoning engine is responsible for:
//! - Taking a task and context
//! - Running the reasoning process (possibly with multiple LLM calls)
//! - Emitting progress events
//! - Returning the final result

use crate::llm::chat::ChatMessage;
use crate::llm::TokenUsage;
use async_trait::async_trait;
use std::pin::Pin;
use futures::{Stream, StreamExt};

/// Context for reasoning
#[derive(Debug, Clone)]
pub struct ReasoningContext {
    /// Chat history for context
    pub history: Vec<ChatMessage>,
    /// Session ID for tracking
    pub session_id: String,
    /// Model to use
    pub model: String,
    /// System prompt prefix
    pub system_prompt: String,
}

impl ReasoningContext {
    /// Create a new reasoning context
    pub fn new(
        history: Vec<ChatMessage>,
        session_id: impl Into<String>,
        model: impl Into<String>,
        system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            history,
            session_id: session_id.into(),
            model: model.into(),
            system_prompt: system_prompt.into(),
        }
    }
}

/// Task for reasoning
#[derive(Debug, Clone)]
pub struct ReasoningTask {
    /// The user query/prompt
    pub query: String,
    /// Task ID for tracking
    pub task_id: String,
}

impl ReasoningTask {
    /// Create a new reasoning task
    pub fn new(query: impl Into<String>, task_id: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            task_id: task_id.into(),
        }
    }
}

/// Events emitted during reasoning
#[derive(Debug, Clone)]
pub enum ReasoningEvent {
    /// Reasoning has started
    Started {
        task_id: String,
        strategy: String,
    },
    /// Progress update
    Progress {
        task_id: String,
        message: String,
        step: usize,
        total_steps: usize,
    },
    /// An intermediate result (for multi-step reasoning)
    IntermediateResult {
        task_id: String,
        content: String,
        step: usize,
    },
    /// The final result
    Completed {
        task_id: String,
        content: String,
        usage: TokenUsage,
    },
    /// An error occurred
    Error {
        task_id: String,
        error: String,
    },
}

/// Trait for reasoning engines
///
/// Implement this trait to create new reasoning strategies.
#[async_trait]
pub trait ReasoningEngine: Send + Sync {
    /// Get the name of the reasoning strategy
    fn name(&self) -> &str;
    
    /// Get a description of the reasoning strategy
    fn description(&self) -> &str;
    
    /// Run the reasoning process
    ///
    /// # Arguments
    /// * `task` - The task to reason about
    /// * `context` - The reasoning context (history, model, etc.)
    ///
    /// # Returns
    /// A stream of reasoning events (progress, intermediate results, final result)
    async fn reason(
        &self,
        task: ReasoningTask,
        context: ReasoningContext,
    ) -> Result<Pin<Box<dyn Stream<Item = ReasoningEvent> + Send>>, String>;
    
    /// Check if this engine supports streaming
    fn supports_streaming(&self) -> bool {
        true
    }
    
    /// Get estimated cost/token usage before running
    ///
    /// Returns an estimate of (input_tokens, output_tokens, cost_usd)
    fn estimate_cost(&self, _task: &ReasoningTask) -> Option<(usize, usize, f64)> {
        None
    }
}

/// Type alias for reasoning engine trait object
pub type ReasoningEngineRef = std::sync::Arc<dyn ReasoningEngine>;

/// Configuration for reasoning engines
#[derive(Debug, Clone)]
pub struct ReasoningConfig {
    /// The reasoning strategy to use
    pub strategy: ReasoningStrategy,
    /// Model override (optional)
    pub model: Option<String>,
    /// Max tokens per request
    pub max_tokens: Option<usize>,
    /// Temperature for sampling
    pub temperature: Option<f32>,
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            strategy: ReasoningStrategy::Standard,
            model: None,
            max_tokens: None,
            temperature: None,
        }
    }
}

/// Available reasoning strategies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningStrategy {
    /// Standard single-pass reasoning
    Standard,
    /// PaCoRe (Parallel Consensus Reasoning)
    PaCoRe,
    /// Tree of Thought (planned)
    TreeOfThought,
    /// Self-consistency (planned)
    SelfConsistency,
}

impl ReasoningStrategy {
    /// Get the name of the strategy
    pub fn name(&self) -> &'static str {
        match self {
            ReasoningStrategy::Standard => "standard",
            ReasoningStrategy::PaCoRe => "pacore",
            ReasoningStrategy::TreeOfThought => "tree_of_thought",
            ReasoningStrategy::SelfConsistency => "self_consistency",
        }
    }
    
    /// Get a description of the strategy
    pub fn description(&self) -> &'static str {
        match self {
            ReasoningStrategy::Standard => "Single-pass LLM inference",
            ReasoningStrategy::PaCoRe => "Parallel Consensus Reasoning with multiple rounds",
            ReasoningStrategy::TreeOfThought => "Tree-structured reasoning exploration",
            ReasoningStrategy::SelfConsistency => "Self-consistency through multiple samples",
        }
    }
}

impl std::str::FromStr for ReasoningStrategy {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "standard" => Ok(ReasoningStrategy::Standard),
            "pacore" => Ok(ReasoningStrategy::PaCoRe),
            "tree_of_thought" | "tot" => Ok(ReasoningStrategy::TreeOfThought),
            "self_consistency" | "sc" => Ok(ReasoningStrategy::SelfConsistency),
            _ => Err(format!("Unknown reasoning strategy: {}", s)),
        }
    }
}

impl std::fmt::Display for ReasoningStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Factory for creating reasoning engines
pub struct ReasoningEngineFactory;

impl ReasoningEngineFactory {
    /// Create a reasoning engine from configuration
    pub fn create(
        strategy: ReasoningStrategy,
        llm_client: std::sync::Arc<crate::llm::LlmClient>,
    ) -> Option<ReasoningEngineRef> {
        match strategy {
            ReasoningStrategy::Standard => {
                Some(std::sync::Arc::new(StandardReasoningEngine::new(llm_client)))
            }
            ReasoningStrategy::PaCoRe => {
                Some(std::sync::Arc::new(PaCoReEngine::new(llm_client)))
            }
            _ => None, // Not yet implemented
        }
    }
}

// Re-export engines
pub mod pacore_engine;
pub use pacore_engine::PaCoReEngine;

/// Standard single-pass reasoning engine
pub struct StandardReasoningEngine {
    llm_client: std::sync::Arc<crate::llm::LlmClient>,
}

impl StandardReasoningEngine {
    /// Create a new standard reasoning engine
    pub fn new(llm_client: std::sync::Arc<crate::llm::LlmClient>) -> Self {
        Self { llm_client }
    }
}

#[async_trait]
impl ReasoningEngine for StandardReasoningEngine {
    fn name(&self) -> &str {
        "standard"
    }
    
    fn description(&self) -> &str {
        "Standard single-pass LLM inference"
    }
    
    async fn reason(
        &self,
        task: ReasoningTask,
        context: ReasoningContext,
    ) -> Result<Pin<Box<dyn Stream<Item = ReasoningEvent> + Send>>, String> {
        use futures::stream;
        
        let task_id = task.task_id.clone();
        let query = task.query;
        let llm_client = self.llm_client.clone();
        let model = context.model;
        let _system_prompt = context.system_prompt;
        
        // Build messages
        let mut messages = context.history;
        messages.push(ChatMessage::user(&query));
        
        let task_id_clone = task_id.clone();
        
        // Create a stream that emits events
        let events = stream::once(async move {
            // Emit started event
            ReasoningEvent::Started {
                task_id: task_id_clone.clone(),
                strategy: "standard".to_string(),
            }
        })
        .chain(stream::once(async move {
            // Call LLM
            let request = crate::llm::chat::ChatRequest {
                model: model.clone(),
                messages: messages.clone(),
                stream: false,
                max_tokens: None,
                stop: None,
                temperature: Some(0.7),
                tools: None,
            };
            
            match llm_client.chat(&request).await {
                Ok(response) => {
                    let content = response.content();
                    let usage = TokenUsage::default(); // TODO: extract from response
                    ReasoningEvent::Completed {
                        task_id: task_id.clone(),
                        content,
                        usage,
                    }
                }
                Err(e) => ReasoningEvent::Error {
                    task_id: task_id.clone(),
                    error: e.to_string(),
                },
            }
        }));
        
        Ok(Box::pin(events))
    }
}
