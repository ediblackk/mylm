use crate::llm::{LlmClient, chat::{ChatMessage, ChatRequest}};
use crate::memory::store::MemoryCategory;
use anyhow::{Result, Context};
use std::sync::Arc;
use chrono::Utc;

pub struct MemoryCategorizer {
    llm_client: Arc<LlmClient>,
    store: Arc<crate::memory::store::VectorStore>,
}

impl MemoryCategorizer {
    pub fn new(llm_client: Arc<LlmClient>, store: Arc<crate::memory::store::VectorStore>) -> Self {
        Self { llm_client, store }
    }

    /// Categorize a memory, potentially creating a new category.
    pub async fn categorize_memory(&self, content: &str) -> Result<String> {
        let categories = self.store.get_all_categories().await.unwrap_or_default();
        
        let mut category_list = String::new();
        for cat in &categories {
            category_list.push_str(&format!("- {}: {}\n", cat.name, cat.summary));
        }

        let prompt = format!(
            "Analyze the following memory and assign it to an existing category or suggest a new one.\n\n\
            ## Existing Categories:\n\
            {}\n\n\
            ## Memory Content:\n\
            {}\n\n\
            ## Instructions:\n\
            - If it fits an existing category, respond with only the category name.\n\
            - If it requires a new category, respond with 'NEW: <category_name>'.\n\
            - Keep category names short, lowercase, and use underscores (e.g., rust_development, system_config).\n\
            - Be specific but avoid creating too many redundant categories.",
            if category_list.is_empty() { "None" } else { &category_list },
            content
        );

        let request = ChatRequest::new(
            self.llm_client.model().to_string(),
            vec![
                ChatMessage::system("You are a specialized memory organizer that categorizes technical knowledge."),
                ChatMessage::user(&prompt),
            ],
        );

        let response = self.llm_client.chat(&request).await?;
        let result = response.content().trim().to_string();

        if result.starts_with("NEW: ") {
            let category_name = result.strip_prefix("NEW: ").map(|s| s.trim()).unwrap_or("unknown");
            let category_id = category_name.to_lowercase().replace(" ", "_");
            
            // Initialize new category
            let new_cat = MemoryCategory {
                id: category_id.clone(),
                name: category_name.to_string(),
                summary: "New category pending first summary.".to_string(),
                last_updated: Utc::now().timestamp(),
                embedding: None,
            };
            
            self.store.update_category(new_cat).await?;
            Ok(category_id)
        } else {
            // Check if it's a valid existing category, otherwise fallback to a generic one or create it
            let id = result.to_lowercase().replace(" ", "_");
            if categories.iter().any(|c| c.id == id) {
                Ok(id)
            } else {
                // If LLM returned a name not in list but didn't use NEW:, treat as new anyway
                let new_cat = MemoryCategory {
                    id: id.clone(),
                    name: result.clone(),
                    summary: "New category pending first summary.".to_string(),
                    last_updated: Utc::now().timestamp(),
                    embedding: None,
                };
                self.store.update_category(new_cat).await?;
                Ok(id)
            }
        }
    }

    /// Update the summary of a category based on its memories.
    pub async fn update_category_summary(&self, category_id: &str) -> Result<()> {
        let category = self.store.get_category_by_id(category_id).await?
            .context("Category not found")?;
        
        let memories = self.store.get_memories_by_category(category_id).await?;
        if memories.is_empty() {
            return Ok(());
        }

        let mut memory_text = String::new();
        for mem in memories.iter().take(20) { // Limit to most recent 20 for summary
            memory_text.push_str(&format!("- {}\n", mem.content));
        }

        let prompt = format!(
            "Summarize the following memories belonging to the category '{}'.\n\
            Create a concise, structured summary that represents the core knowledge in this category.\n\n\
            ## Memories:\n\
            {}",
            category.name,
            memory_text
        );

        let request = ChatRequest::new(
            self.llm_client.model().to_string(),
            vec![
                ChatMessage::system("You are a knowledge architect that creates concise summaries of technical information."),
                ChatMessage::user(&prompt),
            ],
        );

        let response = self.llm_client.chat(&request).await?;
        let summary = response.content().trim().to_string();

        let mut updated_cat = category;
        updated_cat.summary = summary;
        updated_cat.last_updated = Utc::now().timestamp();

        self.store.update_category(updated_cat).await?;
        Ok(())
    }
}
