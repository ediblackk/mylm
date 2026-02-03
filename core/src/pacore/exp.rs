use std::sync::Arc;
use tokio::sync::Semaphore;
use rand::SeedableRng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use crate::pacore::client::ChatClient;
use crate::pacore::error::Error;
use crate::pacore::model::{ChatRequest, ChatResponse, Message};
use crate::pacore::template::TemplateEngine;
use futures_util::Stream;
use std::pin::Pin;

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
        }
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

            let responses = self.run_parallel_calls(current_messages, num_calls).await?;
            all_rounds.push(RoundResult {
                round_idx,
                responses,
            });
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

            // If it's the last round, we stream it
            if round_idx == num_rounds - 1 && num_calls == 1 {
                 let request = ChatRequest {
                    model: self.model.clone(),
                    messages: current_messages,
                    stream: Some(true),
                    max_tokens: None,
                    temperature: Some(0.7),
                    top_p: None,
                };
                return self.client.stream_chat(request).await;
            }

            let responses = self.run_parallel_calls(current_messages, num_calls).await?;
            all_rounds.push(RoundResult {
                round_idx,
                responses,
            });
        }

        Err(Error::Internal("Unexpected end of process_single_stream".to_string()))
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
    ) -> Result<Vec<ChatResponse>, Error> {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let mut tasks = Vec::new();

        for _ in 0..num_calls {
            let permit = semaphore.clone().acquire_owned().await.map_err(|e| Error::Internal(e.to_string()))?;
            let messages = messages.clone();
            let client = self.client.clone();
            let model = self.model.clone();

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
                client.chat_completion(request).await
            });
            tasks.push(task);
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
