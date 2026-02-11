//! Recovery mechanism for agent V2 when LLM responses fail to parse.
//!
//! Provides `RecoveryWorker` which analyzes failed content and error messages
//! to generate corrected Short-Key JSON responses using a recovery LLM prompt.

use crate::llm::{LlmClient, chat::{ChatMessage, ChatRequest}};
use crate::agent::v2::protocol::{ShortKeyAction, parse_short_key_actions_from_content};
use std::error::Error;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryContext {
    pub task: String,
    pub available_tools: String,
    pub failed_content: String,
    pub error_message: String,
}

pub struct RecoveryWorker {
    pub llm_client: Arc<LlmClient>,
}

impl RecoveryWorker {
    pub fn new(llm_client: Arc<LlmClient>) -> Self {
        Self { llm_client }
    }

    pub async fn recover(&self, context: RecoveryContext, model_override: Option<String>) -> Result<Vec<ShortKeyAction>, Box<dyn Error + Send + Sync>> {
        let model = model_override.unwrap_or_else(|| self.llm_client.model().to_string());
        
        let system_prompt = format!(
            "You are a recovery specialist for an AI agent. \
            The agent failed to produce a valid Short-Key JSON response. \
            Your task is to analyze the failed content and the error message, and produce the CORRECTED Short-Key JSON response that the agent intended.\n\n\
            # Available Tools\n\
            {}\n\n\
            # Original Task\n\
            {}\n\n\
            # Short-Key JSON Protocol Schema\n\
            - `t`: Thought. Your internal reasoning and next steps.\n\
            - `a`: Action. The name of the tool to execute (optional if providing final answer).\n\
            - `i`: Input. The arguments for the tool in strict JSON format (optional).\n\
            - `f`: Final Answer. Your final response to the user (optional).\n\n\
            Output ONLY the valid JSON (either a single object or an array of objects) wrapped in a code block or as raw text.",
            context.available_tools,
            context.task
        );

        let user_prompt = format!(
            "FAILED CONTENT:\n{}\n\nERROR MESSAGE:\n{}",
            context.failed_content,
            context.error_message
        );

        let request = ChatRequest::new(
            model,
            vec![
                ChatMessage::system(system_prompt),
                ChatMessage::user(user_prompt),
            ],
        );

        let response = self.llm_client.chat(&request).await?;
        let content = response.content();

        match parse_short_key_actions_from_content(&content) {
            Ok(actions) => Ok(actions),
            Err(e) => Err(format!("Recovery failed to produce valid JSON: {}", e.message).into()),
        }
    }
}
