use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::{Result, Context};
use serde_json;
use serde::{Deserialize, Serialize};
use crate::llm::LlmClient;
use crate::llm::chat::{ChatMessage, ChatRequest};
use crate::memory::journal::{Journal, InteractionType};
use crate::memory::store::{VectorStore, MemoryType};

#[derive(Debug, Serialize, Deserialize)]
pub struct ConsolidationReport {
    pub entries_processed: usize,
    pub facts_extracted: usize,
    pub categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExtractedFact {
    pub fact: String,
    pub category: String,
    pub importance: f32,
}

pub struct Scribe {
    journal: Arc<Mutex<Journal>>,
    store: Arc<VectorStore>,
    llm: Arc<LlmClient>,
}

impl Scribe {
    pub fn new(journal: Arc<Mutex<Journal>>, store: Arc<VectorStore>, llm: Arc<LlmClient>) -> Self {
        Self {
            journal,
            store,
            llm,
        }
    }

    pub async fn observe(&self, entry_type: InteractionType, content: &str) -> Result<()> {
        let mut journal = self.journal.lock().await;
        journal.log(entry_type.clone(), content)?;

        // Also record to vector store for long-term memory if it's significant
        // For now, we record everything as a UserNote or similar, 
        // but we could map InteractionType to MemoryType.
        let memory_type = match entry_type {
            InteractionType::Thought => MemoryType::Decision,
            InteractionType::Tool => MemoryType::Command,
            InteractionType::Output => MemoryType::Discovery,
            InteractionType::Chat => MemoryType::UserNote,
        };

        self.store.add_memory_typed(content, memory_type, None, None, None).await?;
        
        Ok(())
    }

    pub async fn recall(&self, query: &str, limit: usize) -> Result<String> {
        // 1. Fetch "Hot" memory from Journal (recent entries)
        let hot_context = {
            let journal = self.journal.lock().await;
            let entries = journal.entries();
            let start = entries.len().saturating_sub(10); // Last 10 entries
            let mut context = String::from("### Recent Journal Entries (Hot Context)\n");
            for entry in &entries[start..] {
                context.push_str(&format!("[{}] {}: {}\n", entry.timestamp, entry.entry_type, entry.content));
            }
            context
        };

        // 2. Fetch "Cold" memory from Vector Store
        let cold_memories = self.store.search_memory(query, limit).await?;
        let mut cold_context = String::from("\n### Semantic Memory (Cold Context)\n");
        if cold_memories.is_empty() {
            cold_context.push_str("No relevant long-term memories found.\n");
        } else {
            for mem in cold_memories {
                cold_context.push_str(&format!("- [{}] {}\n", mem.r#type, mem.content));
            }
        }

        Ok(format!("{}\n{}", hot_context, cold_context))
    }

    pub fn journal(&self) -> Arc<Mutex<Journal>> {
        self.journal.clone()
    }

    pub fn store(&self) -> Arc<VectorStore> {
        self.store.clone()
    }

    pub fn llm(&self) -> Arc<LlmClient> {
        self.llm.clone()
    }

    pub async fn sleep(&self) -> Result<ConsolidationReport> {
        // 1. Retrieve all entries from the journal
        let (entries, entries_count) = {
            let journal = self.journal.lock().await;
            let entries = journal.entries().to_vec();
            let count = entries.len();
            (entries, count)
        };

        if entries_count == 0 {
            return Ok(ConsolidationReport {
                entries_processed: 0,
                facts_extracted: 0,
                categories: Vec::new(),
            });
        }

        // 2. Prepare context for LLM
        let mut journal_context = String::new();
        for entry in &entries {
            journal_context.push_str(&format!("[{}] {}: {}\n", entry.timestamp, entry.entry_type, entry.content));
        }

        let prompt = format!(
            "You are a knowledge consolidation engine. Analyze the following journal entries from a coding session and extract discrete, atomic facts, technical decisions, patterns, and solved bugs.\n\n\
            Output MUST be a JSON list of objects with the following structure:\n\
            [\n  {{\n    \"fact\": \"Atomic description of the knowledge\",\n    \"category\": \"One of: decision, discovery, bugfix, command, user_note\",\n    \"importance\": 0.8\n  }}\n]\n\n\
            ## Journal Entries:\n\
            {}\n\n\
            ## Instructions:\n\
            - Only extract high-value information.\n\
            - Ensure the 'fact' is self-contained and descriptive.\n\
            - Map categories strictly to the requested list.\n\
            - Return ONLY the JSON array, no extra text.",
            journal_context
        );

        // 3. Call LLM
        let request = ChatRequest::new(
            self.llm.model().to_string(),
            vec![
                ChatMessage::system("You are a specialized memory organizer that extracts technical knowledge into discrete facts."),
                ChatMessage::user(&prompt),
            ],
        );

        let response = self.llm.chat(&request).await?;
        let content = response.content();
        
        // Handle potential markdown formatting in response
        let cleaned_content = if let Some(json_start) = content.find("```json") {
            let start = json_start + 7;
            if let Some(json_end) = content[start..].find("```") {
                &content[start..start + json_end]
            } else {
                &content[start..]
            }
        } else if let Some(code_start) = content.find("```") {
            let start = code_start + 3;
            if let Some(code_end) = content[start..].find("```") {
                &content[start..start + code_end]
            } else {
                &content[start..]
            }
        } else {
            &content
        };
        let cleaned_content = cleaned_content.trim();

        let facts: Vec<ExtractedFact> = serde_json::from_str(cleaned_content)
            .context("Failed to parse extracted facts from LLM response")?;

        // 4. Store facts in VectorStore
        let mut extracted_categories = Vec::new();
        for fact in &facts {
            let memory_type = match fact.category.to_lowercase().as_str() {
                "decision" => MemoryType::Decision,
                "discovery" => MemoryType::Discovery,
                "bugfix" => MemoryType::Bugfix,
                "command" => MemoryType::Command,
                _ => MemoryType::UserNote,
            };

            if !extracted_categories.contains(&fact.category) {
                extracted_categories.push(fact.category.clone());
            }

            let metadata = serde_json::json!({
                "importance": fact.importance,
                "source": "sleep_consolidation",
            });

            self.store.add_memory_typed(
                &fact.fact,
                memory_type,
                None, // session_id
                Some(metadata),
                None, // category_id
            ).await?;
        }

        // 5. Clear journal
        {
            let mut journal = self.journal.lock().await;
            journal.clear()?;
        }

        Ok(ConsolidationReport {
            entries_processed: entries_count,
            facts_extracted: facts.len(),
            categories: extracted_categories,
        })
    }
}
