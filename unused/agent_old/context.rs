//! Context management for the legacy Agent
//!
//! This module handles conversation history pruning, condensation,
//! and memory context injection for the legacy Agent.

use crate::llm::chat::{ChatMessage, ChatRequest, MessageRole};
use crate::llm::LlmClient;
use crate::memory::store::{Memory, VectorStore};
use std::error::Error as StdError;
use std::sync::Arc;

/// Prune history to stay within token limits.
///
/// Keeps the system message and as many recent messages as fit within the limit.
/// Ensures the conversation doesn't start with an assistant/tool message after
/// the system prompt (required by some APIs like Gemini).
pub fn prune_history(history: Vec<ChatMessage>, limit: usize) -> Vec<ChatMessage> {
    if history.len() <= 1 {
        return history;
    }

    let mut total_chars = 0;
    for msg in &history {
        total_chars += msg.content.len();
    }

    let approx_tokens = total_chars / 4;
    if approx_tokens <= limit {
        return history;
    }

    let system_msg = history[0].clone();
    let mut pruned = Vec::new();
    pruned.push(system_msg.clone());

    let mut current_tokens = system_msg.content.len() / 4;
    let mut to_keep = Vec::new();

    // Iterate backwards to keep most recent messages
    for msg in history.iter().skip(1).rev() {
        let msg_tokens = msg.content.len() / 4;
        if current_tokens + msg_tokens < limit {
            to_keep.push(msg.clone());
            current_tokens += msg_tokens;
        } else {
            break;
        }
    }

    to_keep.reverse();

    // Gemini/Strict API Requirement: Ensure we don't start with an Assistant/Tool message
    // after the system prompt.
    while !to_keep.is_empty() && to_keep[0].role != MessageRole::User {
        to_keep.remove(0);
    }

    pruned.extend(to_keep);
    pruned
}

/// Condense the conversation history by summarizing older messages.
///
/// This reduces token usage for long conversations while preserving
/// the most recent context.
pub async fn condense_history(
    history: &[ChatMessage],
    llm_client: &LlmClient,
) -> Result<Vec<ChatMessage>, Box<dyn StdError + Send + Sync>> {
    if history.len() <= 5 {
        return Ok(history.to_vec());
    }

    let system_prompt = if !history.is_empty() && history[0].role == MessageRole::System {
        Some(history[0].clone())
    } else {
        None
    };

    let to_summarize = &history[1..history.len() - 3];
    let latest = &history[history.len() - 3..];

    let mut summary_input = String::from(
        "Summarize the following conversation history into a concise summary that preserves all key facts, decisions, and context for an AI assistant to continue the task:\n\n"
    );
    for msg in to_summarize {
        summary_input.push_str(&format!(
            "{}: {}\n",
            match msg.role {
                MessageRole::System => "System",
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
                MessageRole::Tool => "Tool",
            },
            msg.content
        ));
    }

    let summary_request = ChatRequest::new(
        llm_client.model().to_string(),
        vec![
            ChatMessage::system("You are a helpful assistant that summarizes technical conversations."),
            ChatMessage::user(&summary_input),
        ],
    );

    let response = llm_client.chat(&summary_request).await?;
    let summary = response.content();

    let mut new_history = Vec::new();
    if let Some(sys) = system_prompt {
        new_history.push(sys);
    }
    new_history.push(ChatMessage::assistant(format!(
        "[Context Summary]: {}",
        summary
    )));
    new_history.extend_from_slice(latest);

    Ok(new_history)
}

/// Build context string from retrieved memories
pub fn build_context_from_memories(memories: &[Memory]) -> String {
    if memories.is_empty() {
        return String::new();
    }

    let mut context = String::from("## Relevant Past Operations & Knowledge\n");
    for (i, mem) in memories.iter().enumerate() {
        let timestamp = chrono::DateTime::from_timestamp(mem.created_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown time".to_string());

        context.push_str(&format!(
            "{}. [{}] {} ({} {} {})",
            i + 1,
            mem.r#type,
            mem.content,
            "memory_id",
            mem.id,
            timestamp,
        ));
        context.push('\n');
    }
    context.push_str("\nUse this context to inform your actions and avoid repeating mistakes.");
    context
}

/// Inject relevant memories into the conversation history based on the last user message.
pub async fn inject_memory_context(
    history: &mut [ChatMessage],
    memory_store: Option<&Arc<VectorStore>>,
    auto_context: bool,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    if !auto_context {
        return Ok(());
    }

    if let Some(store) = memory_store {
        // Find the last user message to use as a search query
        if let Some(last_user_msg) = history.iter().rev().find(|m| m.role == MessageRole::User) {
            let memories = store
                .search_memory(&last_user_msg.content, 5)
                .await
                .unwrap_or_default();
            if !memories.is_empty() {
                let context = build_context_from_memories(&memories);
                // Append context to the last user message
                if let Some(user_idx) = history.iter().rposition(|m| m.role == MessageRole::User) {
                    history[user_idx].content.push('\n');
                    history[user_idx].content.push('\n');
                    history[user_idx].content.push_str(&context);
                }
            }
        }
    }
    Ok(())
}

/// Calculate approximate token count from character count
/// 
/// Uses a simple heuristic: 1 token ≈ 4 characters
pub fn estimate_token_count(text: &str) -> usize {
    text.len() / 4
}

/// Calculate context usage statistics
pub fn calculate_context_stats(history: &[ChatMessage], limit: usize) -> (usize, usize, f64) {
    let total_chars: usize = history.iter().map(|m| m.content.len()).sum();
    let approx_tokens = estimate_token_count(&" ".repeat(total_chars));
    let usage_pct = if limit > 0 {
        (approx_tokens as f64 / limit as f64) * 100.0
    } else {
        0.0
    };
    
    (approx_tokens, limit.saturating_sub(approx_tokens), usage_pct)
}
