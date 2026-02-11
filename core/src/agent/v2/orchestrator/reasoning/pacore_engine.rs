//! PaCoRe (Parallel Consensus Reasoning) Engine
//!
//! This module provides the PaCoRe reasoning engine implementation,
//! which uses multi-round parallel LLM inference to improve response quality.
//!
//! PaCoRe works by:
//! 1. Running multiple LLM calls in parallel for a given prompt
//! 2. Synthesizing the responses
//! 3. Using the synthesized result as input for the next round
//! 4. Repeating for N rounds, with increasing parallelism

use crate::agent::reasoning::{ReasoningEngine, ReasoningTask, ReasoningContext, ReasoningEvent};
use crate::llm::chat::ChatMessage;
use crate::llm::TokenUsage;
use async_trait::async_trait;
use std::pin::Pin;
use futures::{Stream, stream, StreamExt};

/// PaCoRe reasoning engine configuration
#[derive(Debug, Clone)]
pub struct PaCoReConfig {
    /// Number of parallel calls per round (e.g., vec![1, 2, 4] for 3 rounds)
    pub rounds: Vec<usize>,
    /// Maximum concurrent requests
    pub max_concurrent: usize,
    /// Temperature for sampling
    pub temperature: f32,
    /// Random seed for reproducibility (optional)
    pub random_seed: Option<u64>,
}

impl Default for PaCoReConfig {
    fn default() -> Self {
        Self {
            rounds: vec![1, 2, 4], // Default: 3 rounds with 1, 2, 4 calls
            max_concurrent: 10,
            temperature: 0.7,
            random_seed: None,
        }
    }
}

impl PaCoReConfig {
    /// Parse rounds from a comma-separated string (e.g., "1,2,4")
    pub fn from_rounds_str(s: &str) -> Result<Self, String> {
        let rounds: Vec<usize> = s
            .split(',')
            .map(|part| part.trim().parse::<usize>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Invalid rounds string: {}", e))?;
        
        if rounds.is_empty() {
            return Err("Rounds cannot be empty".to_string());
        }
        
        Ok(Self {
            rounds,
            ..Default::default()
        })
    }
    
    /// Get total number of LLM calls
    pub fn total_calls(&self) -> usize {
        self.rounds.iter().sum()
    }
}

/// PaCoRe (Parallel Consensus Reasoning) Engine
///
/// Implements multi-round parallel inference with synthesis between rounds.
pub struct PaCoReEngine {
    llm_client: std::sync::Arc<crate::llm::LlmClient>,
    config: PaCoReConfig,
}

impl PaCoReEngine {
    /// Create a new PaCoRe engine with default configuration
    pub fn new(llm_client: std::sync::Arc<crate::llm::LlmClient>) -> Self {
        Self {
            llm_client,
            config: PaCoReConfig::default(),
        }
    }
    
    /// Create a new PaCoRe engine with custom configuration
    pub fn with_config(llm_client: std::sync::Arc<crate::llm::LlmClient>, config: PaCoReConfig) -> Self {
        Self {
            llm_client,
            config,
        }
    }
    
    /// Parse rounds from string and create engine
    pub fn with_rounds_str(
        llm_client: std::sync::Arc<crate::llm::LlmClient>,
        rounds_str: &str,
    ) -> Result<Self, String> {
        let config = PaCoReConfig::from_rounds_str(rounds_str)?;
        Ok(Self::with_config(llm_client, config))
    }
    
    /// Synthesize responses from multiple LLM calls into a single prompt
    #[allow(dead_code)]
    fn synthesize_prompt(&self, original_query: &str, responses: &[String]) -> String {
        if responses.is_empty() {
            return original_query.to_string();
        }
        
        if responses.len() == 1 {
            return responses[0].clone();
        }
        
        // Build synthesis prompt
        let mut synthesis = format!(
            "Based on the original query and multiple expert responses, provide a comprehensive answer.\n\n\
            Original Query: {}\n\n\
            Expert Responses:\n",
            original_query
        );
        
        for (i, response) in responses.iter().enumerate() {
            synthesis.push_str(&format!("\n--- Response {} ---\n{}\n", i + 1, response));
        }
        
        synthesis.push_str("\n--- Your Synthesized Answer ---\n\
                           Based on these expert responses, provide a comprehensive and accurate answer.");
        
        synthesis
    }
}

#[async_trait]
impl ReasoningEngine for PaCoReEngine {
    fn name(&self) -> &str {
        "pacore"
    }
    
    fn description(&self) -> &str {
        "Parallel Consensus Reasoning with multi-round parallel inference"
    }
    
    async fn reason(
        &self,
        task: ReasoningTask,
        context: ReasoningContext,
    ) -> Result<Pin<Box<dyn Stream<Item = ReasoningEvent> + Send>>, String> {
        let task_id = task.task_id.clone();
        let task_id_for_stream = task_id.clone();
        let query = task.query;
        let llm_client = self.llm_client.clone();
        let model = context.model;
        let config = self.config.clone();
        let history = context.history;
        
        // Create the stream
        let stream = stream::once(async move {
            ReasoningEvent::Started {
                task_id: task_id_for_stream,
                strategy: "pacore".to_string(),
            }
        })
        .chain(stream::unfold(
            PaCoReState {
                task_id: task_id.clone(),
                query: query.clone(),
                llm_client: llm_client.clone(),
                model: model.clone(),
                config: config.clone(),
                history: history.clone(),
                round_idx: 0,
                last_responses: Vec::new(),
                total_usage: TokenUsage::default(),
                is_complete: false,
            },
            |mut state| async move {
                if state.is_complete {
                    return None;
                }
                
                if state.round_idx >= state.config.rounds.len() {
                    // All rounds complete, emit final result
                    let final_content = state.last_responses.first()
                        .cloned()
                        .unwrap_or_default();
                    
                    state.is_complete = true;
                    
                    return Some((
                        ReasoningEvent::Completed {
                            task_id: state.task_id.clone(),
                            content: final_content,
                            usage: state.total_usage.clone(),
                        },
                        state,
                    ));
                }
                
                let num_calls = state.config.rounds[state.round_idx];
                let current_query = if state.round_idx == 0 {
                    state.query.clone()
                } else {
                    state.synthesize_prompt(&state.query, &state.last_responses)
                };
                
                // Emit progress
                let progress_event = ReasoningEvent::Progress {
                    task_id: state.task_id.clone(),
                    message: format!("Round {}: Running {} parallel calls", state.round_idx + 1, num_calls),
                    step: state.round_idx + 1,
                    total_steps: state.config.rounds.len(),
                };
                
                // Run parallel calls (simplified - sequential for now)
                let mut round_responses = Vec::new();
                for i in 0..num_calls {
                    let messages = build_messages(&state.history, &current_query);
                    let request = crate::llm::chat::ChatRequest {
                        model: state.model.clone(),
                        messages,
                        stream: false,
                        max_tokens: None,
                        stop: None,
                        temperature: Some(state.config.temperature),
                        tools: None,
                    };
                    
                    match state.llm_client.chat(&request).await {
                        Ok(response) => {
                            // TODO: Extract usage from response when available
                            round_responses.push(response.content());
                        }
                        Err(e) => {
                            return Some((
                                ReasoningEvent::Error {
                                    task_id: state.task_id.clone(),
                                    error: format!("Round {} call {} failed: {}", state.round_idx + 1, i + 1, e),
                                },
                                state,
                            ));
                        }
                    }
                }
                
                // Emit intermediate result
                let intermediate_event = if state.round_idx < state.config.rounds.len() - 1 {
                    Some(ReasoningEvent::IntermediateResult {
                        task_id: state.task_id.clone(),
                        content: format!("Round {} complete with {} responses", state.round_idx + 1, round_responses.len()),
                        step: state.round_idx + 1,
                    })
                } else {
                    None
                };
                
                state.last_responses = round_responses;
                state.round_idx += 1;
                
                // Return progress event first, then intermediate if available
                if let Some(_intermediate) = intermediate_event {
                    // We can only return one event per iteration, so we'll emit intermediate next time
                    // For now, just emit progress
                    Some((progress_event, state))
                } else {
                    Some((progress_event, state))
                }
            },
        ));
        
        Ok(Box::pin(stream))
    }
    
    fn estimate_cost(&self, _task: &ReasoningTask) -> Option<(usize, usize, f64)> {
        let total_calls = self.config.total_calls();
        // Rough estimate: 1000 input tokens, 500 output tokens per call
        let est_input = 1000 * total_calls;
        let est_output = 500 * total_calls;
        // Rough cost estimate: $0.001 per 1K tokens
        let est_cost = (est_input + est_output) as f64 * 0.000001;
        Some((est_input, est_output, est_cost))
    }
}

/// Internal state for PaCoRe stream
struct PaCoReState {
    task_id: String,
    query: String,
    llm_client: std::sync::Arc<crate::llm::LlmClient>,
    model: String,
    config: PaCoReConfig,
    history: Vec<ChatMessage>,
    round_idx: usize,
    last_responses: Vec<String>,
    total_usage: TokenUsage,
    is_complete: bool,
}

impl PaCoReState {
    fn synthesize_prompt(&self, original_query: &str, responses: &[String]) -> String {
        if responses.is_empty() {
            return original_query.to_string();
        }
        
        if responses.len() == 1 {
            return format!(
                "Based on the previous analysis, provide an improved answer:\n\n\
                Original Query: {}\n\n\
                Previous Analysis: {}\n\n\
                Improved Answer:",
                original_query,
                responses[0]
            );
        }
        
        let mut synthesis = format!(
            "You are an expert synthesizer. Review multiple analyses of the same query and provide a comprehensive, accurate answer.\n\n\
            Original Query: {}\n\n\
            Expert Analyses:\n",
            original_query
        );
        
        for (i, response) in responses.iter().enumerate() {
            synthesis.push_str(&format!("\n--- Analysis {} ---\n{}\n", i + 1, response));
        }
        
        synthesis.push_str("\n--- Your Synthesized Answer ---\n\
                           Synthesize these analyses into a single comprehensive and accurate response:");
        
        synthesis
    }
}

/// Build messages for LLM request
fn build_messages(history: &[ChatMessage], query: &str) -> Vec<ChatMessage> {
    let mut messages = history.to_vec();
    messages.push(ChatMessage::user(query));
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pacore_config_default() {
        let config = PaCoReConfig::default();
        assert_eq!(config.rounds, vec![1, 2, 4]);
        assert_eq!(config.max_concurrent, 10);
        assert_eq!(config.total_calls(), 7);
    }
    
    #[test]
    fn test_pacore_config_from_str() {
        let config = PaCoReConfig::from_rounds_str("1,3,5").unwrap();
        assert_eq!(config.rounds, vec![1, 3, 5]);
        assert_eq!(config.total_calls(), 9);
    }
    
    #[test]
    fn test_pacore_config_from_str_invalid() {
        assert!(PaCoReConfig::from_rounds_str("").is_err());
        assert!(PaCoReConfig::from_rounds_str("a,b,c").is_err());
    }
    
    #[test]
    fn test_synthesize_prompt_single() {
        let engine = PaCoReEngine::new(std::sync::Arc::new(
            crate::llm::LlmClient::new(
                crate::llm::LlmConfig::new(
                    crate::llm::LlmProvider::OpenAiCompatible,
                    "http://test".to_string(),
                    "test-model".to_string(),
                    Some("test-key".to_string()),
                )
            ).unwrap()
        ));
        
        let responses = vec!["Analysis result".to_string()];
        let synthesized = engine.synthesize_prompt("What is 2+2?", &responses);
        
        assert!(synthesized.contains("What is 2+2?"));
        assert!(synthesized.contains("Analysis result"));
    }
}
