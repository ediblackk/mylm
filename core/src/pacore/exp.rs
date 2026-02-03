use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Semaphore;
use rand::SeedableRng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use crate::pacore::client::ChatClient;
use crate::pacore::error::Error;
use crate::pacore::model::{ChatRequest, ChatResponse, Message, Choice};
use crate::pacore::template::TemplateEngine;
use futures_util::Stream;
use std::pin::Pin;


#[derive(Debug, Clone)]
pub enum PaCoReProgressEvent {
    RoundStarted { round: usize, total_rounds: usize, calls_in_round: usize },
    BatchStarted { round: usize, batch: usize, total_batches: usize },
    CallCompleted { round: usize, call_index: usize, total_calls: usize },
    RoundCompleted { round: usize, responses_received: usize },
    SynthesisStarted { round: usize },
    StreamingStarted,
    Error { round: usize, error: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RoundResult {
    pub round_idx: usize,
    pub responses: Vec<ChatResponse>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProcessedResult {
    pub request_id: String,
    pub final_response: Option<String>,
    pub rounds: Vec<RoundResult>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BatchItem {
    pub messages: Vec<Message>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BatchResult {
    pub request_id: String,
    pub result: Result<ProcessedResult, String>,
}

#[derive(Clone)]
pub struct Exp {
    pub model: String,
    pub num_responses_per_round: Vec<usize>,
    pub max_concurrent: usize,
    pub client: ChatClient,
    pub template: TemplateEngine,
    pub random_seed: Option<u64>,
    // Progress callback for UI feedback
    pub progress_callback: Option<Arc<dyn Fn(PaCoReProgressEvent) + Send + Sync>>,
}

impl Exp {
    pub fn new(
        model: String,
        num_responses_per_round: Vec<usize>,
        max_concurrent: usize,
        client: ChatClient,
    ) -> Self {
        Self {
            model,
            num_responses_per_round,
            max_concurrent,
            client,
            template: TemplateEngine::new(),
            random_seed: None,
            progress_callback: None,
        }
    }

    pub fn with_progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(PaCoReProgressEvent) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    pub async fn process_single(
        &self,
        messages: Vec<Message>,
        request_id: &str,
    ) -> Result<ProcessedResult, Error> {
        let mut all_rounds: Vec<RoundResult> = Vec::new();
        let original_prompt = self.extract_user_content(&messages)?;

        for (round_idx, &num_calls) in self.num_responses_per_round.iter().enumerate() {
            let current_messages = if round_idx == 0 {
                messages.clone()
            } else {
                let prev_answers: Vec<String> = all_rounds.last()
                    .unwrap()
                    .responses
                    .iter()
                    .flat_map(|resp| resp.choices.iter())
                    .filter_map(|choice| choice.message.as_ref())
                    .map(|msg| msg.content.clone())
                    .collect();

                let synthesized_prompt = self.template.render(
                    "synthesis_prompt",
                    &original_prompt,
                    &prev_answers,
                )?;

                let mut new_messages = messages.clone();
                self.update_last_user_message(&mut new_messages, synthesized_prompt)?;
                new_messages
            };

            // Emit round started event
            if let Some(cb) = &self.progress_callback {
                cb(PaCoReProgressEvent::RoundStarted { 
                    round: round_idx, 
                    total_rounds: self.num_responses_per_round.len(), 
                    calls_in_round: num_calls 
                });
            }

            let responses = self.run_parallel_calls(current_messages, num_calls, round_idx).await?;
            all_rounds.push(RoundResult {
                round_idx,
                responses: responses.clone(),
            });

            // Emit round completed event
            if let Some(cb) = &self.progress_callback {
                cb(PaCoReProgressEvent::RoundCompleted { 
                    round: round_idx, 
                    responses_received: responses.len() 
                });
            }
        }

        let final_response = all_rounds.last()
            .and_then(|r| r.responses.first())
            .and_then(|resp| resp.choices.first())
            .and_then(|choice| choice.message.as_ref())
            .map(|msg| msg.content.clone());

        Ok(ProcessedResult {
            request_id: request_id.to_string(),
            final_response,
            rounds: all_rounds,
        })
    }

    pub async fn process_single_stream(
        &self,
        messages: Vec<Message>,
        _request_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatResponse, Error>> + Send>>, Error> {
        let mut all_rounds: Vec<RoundResult> = Vec::new();
        let original_prompt = self.extract_user_content(&messages)?;
        let num_rounds = self.num_responses_per_round.len();

        for (round_idx, &num_calls) in self.num_responses_per_round.iter().enumerate() {
            // Emit round started event
            if let Some(cb) = &self.progress_callback {
                cb(PaCoReProgressEvent::RoundStarted { 
                    round: round_idx, 
                    total_rounds: num_rounds, 
                    calls_in_round: num_calls 
                });
            }

            let current_messages = if round_idx == 0 {
                messages.clone()
            } else {
                // Emit synthesis started event
                if let Some(cb) = &self.progress_callback {
                    cb(PaCoReProgressEvent::SynthesisStarted { round: round_idx });
                }

                let prev_answers: Vec<String> = all_rounds.last()
                    .unwrap()
                    .responses
                    .iter()
                    .flat_map(|resp| resp.choices.iter())
                    .filter_map(|choice| choice.message.as_ref())
                    .map(|msg| msg.content.clone())
                    .collect();

                let synthesized_prompt = self.template.render(
                    "synthesis_prompt",
                    &original_prompt,
                    &prev_answers,
                )?;

                let mut new_messages = messages.clone();
                self.update_last_user_message(&mut new_messages, synthesized_prompt)?;
                new_messages
            };

            // If it's the last round, we stream it
            if round_idx == num_rounds - 1 {
                // Emit streaming started event
                if let Some(cb) = &self.progress_callback {
                    cb(PaCoReProgressEvent::StreamingStarted);
                }

                if num_calls == 1 {
                    // Single call - stream directly from API
                    let request = ChatRequest {
                        model: self.model.clone(),
                        messages: current_messages,
                        stream: Some(true),
                        max_tokens: None,
                        temperature: Some(0.7),
                        top_p: None,
                    };
                    return self.client.stream_chat(request).await;
                } else {
                    // Multiple calls in final round - run them and synthesize
                    let responses = self.run_parallel_calls(current_messages, num_calls, round_idx).await?;
                    all_rounds.push(RoundResult {
                        round_idx,
                        responses: responses.clone(),
                    });

                    // Emit round completed event
                    if let Some(cb) = &self.progress_callback {
                        cb(PaCoReProgressEvent::RoundCompleted { 
                            round: round_idx, 
                            responses_received: responses.len() 
                        });
                    }

                    // Synthesize final answer from all responses
                    let final_content = self.synthesize_final_answer(&responses)?;

                    // Return as synthetic stream
                    return Ok(Box::pin(tokio_stream::iter(vec![
                        Ok(ChatResponse {
                            id: "pacore_synthesized".to_string(),
                            object: "chat.completion".to_string(),
                            created: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            model: self.model.clone(),
                            choices: vec![Choice {
                                index: 0,
                                message: Some(Message {
                                    role: "assistant".to_string(),
                                    content: final_content,
                                    name: None,
                                    tool_calls: None,
                                }),
                                delta: None,
                                finish_reason: Some("stop".to_string()),
                            }],
                            usage: None,
                        })
                    ])));
                }
            }

            let responses = self.run_parallel_calls(current_messages, num_calls, round_idx).await?;
            all_rounds.push(RoundResult {
                round_idx,
                responses: responses.clone(),
            });

            // Emit round completed event
            if let Some(cb) = &self.progress_callback {
                cb(PaCoReProgressEvent::RoundCompleted { 
                    round: round_idx, 
                    responses_received: responses.len() 
                });
            }
        }

        // This should not be reached, but return an error just in case
        Err(Error::Internal("Unexpected end of process_single_stream".to_string()))
    }

    /// Synthesize a final answer from multiple responses
    fn synthesize_final_answer(&self, responses: &[ChatResponse]) -> Result<String, Error> {
        // Extract all response contents
        let answers: Vec<String> = responses
            .iter()
            .filter_map(|resp| {
                resp.choices.first()
                    .and_then(|choice| choice.message.as_ref())
                    .map(|msg| msg.content.clone())
            })
            .collect();

        if answers.is_empty() {
            return Err(Error::Internal("No valid responses to synthesize".to_string()));
        }

        if answers.len() == 1 {
            return Ok(answers[0].clone());
        }

        // Simple synthesis: take the most common answer (voting)
        // For now, return the first answer as a placeholder for more sophisticated synthesis
        // TODO: Implement proper synthesis using LLM or consensus algorithm
        Ok(answers[0].clone())
    }

    pub async fn run_batch(
        &self,
        dataset: Vec<BatchItem>,
        max_concurrent_requests: usize,
    ) -> Vec<BatchResult> {
        let semaphore = Arc::new(Semaphore::new(max_concurrent_requests));
        let mut tasks = Vec::new();

        for (idx, item) in dataset.into_iter().enumerate() {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let exp = self.clone();
            let request_id = format!("req_{}", idx);

            let task = tokio::spawn(async move {
                let _permit = permit;
                let result = exp.process_single(item.messages, &request_id).await;
                BatchResult {
                    request_id,
                    result: result.map_err(|e| e.to_string()),
                }
            });
            tasks.push(task);
        }

        let mut results = Vec::new();
        for task in tasks {
            if let Ok(res) = task.await {
                results.push(res);
            }
        }
        results
    }

    async fn run_parallel_calls(
        &self,
        messages: Vec<Message>,
        num_calls: usize,
        round_idx: usize,
    ) -> Result<Vec<ChatResponse>, Error> {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let mut tasks = Vec::new();
        let completed = Arc::new(AtomicUsize::new(0));
        
        let num_batches = (num_calls + self.max_concurrent - 1) / self.max_concurrent;

        for batch_idx in 0..num_batches {
            // Emit batch started event
            if let Some(cb) = &self.progress_callback {
                cb(PaCoReProgressEvent::BatchStarted { 
                    round: round_idx, 
                    batch: batch_idx + 1, 
                    total_batches: num_batches 
                });
            }

            let start = batch_idx * self.max_concurrent;
            let end = ((batch_idx + 1) * self.max_concurrent).min(num_calls);

            for i in start..end {
                let permit = semaphore.clone().acquire_owned().await.map_err(|e| Error::Internal(e.to_string()))?;
                let messages = messages.clone();
                let client = self.client.clone();
                let model = self.model.clone();
                let callback = self.progress_callback.clone();
                let completed = completed.clone();
                let call_index = i;

                let task = tokio::spawn(async move {
                    let _permit = permit;
                    let request = ChatRequest {
                        model,
                        messages,
                        stream: Some(false),
                        max_tokens: None,
                        temperature: Some(0.7),
                        top_p: None,
                    };
                    let result = client.chat_completion(request).await;
                    
                    // Increment and emit progress
                    let _count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(cb) = callback {
                        cb(PaCoReProgressEvent::CallCompleted { 
                            round: round_idx, 
                            call_index: call_index, 
                            total_calls: num_calls 
                        });
                    }
                    
                    result
                });
                tasks.push(task);
            }
        }

        let mut results = Vec::new();
        for task in tasks {
            results.push(task.await.map_err(|e| Error::Internal(e.to_string()))??);
        }

        // Shuffle to avoid ordering bias
        match self.random_seed {
            Some(seed) => {
                let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
                results.shuffle(&mut rng);
            }
            None => {
                let mut rng = rand::thread_rng();
                results.shuffle(&mut rng);
            }
        };

        Ok(results)
    }

    fn extract_user_content(&self, messages: &[Message]) -> Result<String, Error> {
        messages.iter()
            .filter(|m| m.role == "user")
            .last()
            .map(|m| m.content.clone())
            .ok_or_else(|| Error::Internal("No user message found".to_string()))
    }

    fn update_last_user_message(&self, messages: &mut Vec<Message>, new_content: String) -> Result<(), Error> {
        if let Some(m) = messages.iter_mut().filter(|m| m.role == "user").last() {
            m.content = new_content;
            Ok(())
        } else {
            Err(Error::Internal("No user message to update".to_string()))
        }
    }
}
