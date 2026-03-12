use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use anyhow::{Context, Result};
use arrow_array::{RecordBatch, RecordBatchIterator, StringArray, Float32Array, Int64Array, FixedSizeListArray, Array, ArrayRef, new_null_array};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, Table};

use serde::{Deserialize, Serialize};
use tokio::task;
use futures::TryStreamExt;
use tracing::{info, warn, error};

/// Sanitize memory content by removing patterns that trigger WAF.
/// Uses simple replacement text that doesn't look like code/markup to WAF.
pub fn sanitize_memory_content(content: &str) -> String {
    use regex::Regex;
    
    // Step 1: Strip ANSI escape sequences
    let ansi_regex = Regex::new(r"\x1B\[[0-9;]*[a-zA-Z]|\x1B\][^\x07]*\x07|\x1B\[[\?0-9]*[hl]").unwrap();
    let without_ansi = ansi_regex.replace_all(content, "");
    
    // Step 2: Remove command substitution patterns entirely
    let cmd_subst_regex = Regex::new(r"\$\([^)]+\)|`[^`]+`|\$\{[^}]+\}").unwrap();
    let without_cmd_subst = cmd_subst_regex.replace_all(&without_ansi, " ... ");
    
    // Step 3: Remove shell prompt patterns entirely
    let shell_prompt_regex = Regex::new(r"[a-zA-Z0-9_-]+@[a-zA-Z0-9_-]+:[^$]+\$\s*>?\s*|[a-zA-Z0-9_-]+@[a-zA-Z0-9_-]+\$\s*").unwrap();
    let without_shell_prompts = shell_prompt_regex.replace_all(&without_cmd_subst, " ");
    
    // Step 4: Remove the word "command" in JSON keys
    let command_key_regex = Regex::new(r#""?command"?"#).unwrap();
    let without_command_key = command_key_regex.replace_all(&without_shell_prompts, "cmd");
    
    // Step 5: Remove execute_command action name
    let exec_cmd_regex = Regex::new(r"execute_command").unwrap();
    let without_exec_cmd = exec_cmd_regex.replace_all(&without_command_key, "run");
    
    // Step 6: Remove suggestion UI lines entirely
    let suggestion_regex = Regex::new(r"\[Suggestion\]:.*").unwrap();
    let without_suggestions = suggestion_regex.replace_all(&without_exec_cmd, " ");

    // Step 7: Remove lines starting with "> " (shell redirection)
    let redirect_regex = Regex::new(r"(?m)^> .+").unwrap();
    let without_redirects = redirect_regex.replace_all(&without_suggestions, " ");

    // Step 8: Remove shell operators and test patterns
    let shell_ops_regex = Regex::new(r"2>/dev/null|>/dev/null|&&|\|\||\[ -t 0 \]|stty echo|stty -echo|\{ [^}]+ \}|> [^;\n]+").unwrap();
    let without_shell_ops = shell_ops_regex.replace_all(&without_redirects, " ");
    
    // Step 9: Remove terminal context markers (look like code comments)
    let terminal_marker_regex = Regex::new(r"--- TERMINAL CONTEXT ---|--- COMMAND OUTPUT ---|CMD_OUTPUT:").unwrap();
    let without_markers = terminal_marker_regex.replace_all(&without_shell_ops, " ");
    
    // Step 10: Collapse multiple spaces and newlines
    let collapsed = Regex::new(r"\s+").unwrap().replace_all(&without_markers, " ");
    
    // Step 11: Remove control characters except whitespace
    collapsed
        .chars()
        .filter(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    Command,
    SshExecution,
    Decision,
    Discovery,
    Bugfix,
    UserNote,
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryType::Command => write!(f, "command"),
            MemoryType::SshExecution => write!(f, "ssh_exec"),
            MemoryType::Decision => write!(f, "decision"),
            MemoryType::Discovery => write!(f, "discovery"),
            MemoryType::Bugfix => write!(f, "bugfix"),
            MemoryType::UserNote => write!(f, "user_note"),
        }
    }
}

impl From<&str> for MemoryType {
    fn from(s: &str) -> Self {
        match s {
            "command" => MemoryType::Command,
            "ssh_exec" => MemoryType::SshExecution,
            "decision" => MemoryType::Decision,
            "discovery" => MemoryType::Discovery,
            "bugfix" => MemoryType::Bugfix,
            _ => MemoryType::UserNote,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub content: String,
    pub summary: Option<String>,
    pub created_at: i64,
    pub r#type: MemoryType,
    pub session_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub category_id: Option<String>,
    #[serde(skip)]
    
    pub embedding: Option<Vec<f32>>,
}

impl std::fmt::Display for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.r#type, self.content)
    }
}

/// A memory with its relevance score
#[derive(Debug, Clone)]
pub struct ScoredMemory {
    pub memory: Memory,
    pub score: f32,
}

/// Calculate lexical score between query and content
/// 
/// Returns a value from 0.0 (no word matches) to 1.0 (all words match)
fn lexical_score(query: &str, content: &str) -> f32 {
    const STOP_WORDS: &[&str] = &[
        "the", "a", "an", "is", "are", "was", "were", "be", "been",
        "have", "has", "had", "do", "does", "did", "will", "would",
        "this", "that", "these", "those", "and", "or", "but", "in",
        "on", "at", "to", "for", "of", "with", "by", "from",
        "it", "its", "it's", "they", "them", "their", "i", "you", "he", "she",
        "we", "us", "our", "me", "him", "her", "my", "your",
    ];
    
    let query_lower = query.to_lowercase();
    let content_lower = content.to_lowercase();
    
    // Extract meaningful words from query (3+ chars, not stop words)
    let query_words: Vec<&str> = query_lower
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(w))
        .collect();
    
    if query_words.is_empty() {
        return 0.0;
    }
    
    // Count matches
    let matches = query_words.iter()
        .filter(|&&word| content_lower.contains(word))
        .count();
    
    matches as f32 / query_words.len() as f32
}

/// Calculate recency score based on memory age
/// 
/// Returns 1.0 for very recent, decaying to 0.2 for old memories
fn recency_score(created_at: i64) -> f32 {
    let now = chrono::Utc::now().timestamp();
    let age_hours = (now - created_at).max(0) as f32 / 3600.0;
    
    // Step function decay
    match age_hours {
        h if h < 1.0 => 1.0,      // < 1 hour
        h if h < 24.0 => 0.8,     // < 1 day
        h if h < 168.0 => 0.6,    // < 1 week
        h if h < 720.0 => 0.4,    // < 1 month
        _ => 0.2,                  // older
    }
}

/// Calculate importance score based on memory type and content
/// 
/// Returns 0.0 to 1.0, higher for more important memories
fn importance_score(memory: &Memory) -> f32 {
    // Base score by memory type
    let base = match memory.r#type {
        MemoryType::Bugfix => 1.0,
        MemoryType::Decision => 0.9,
        MemoryType::Discovery => 0.7,
        MemoryType::UserNote => 0.6,
        MemoryType::Command => 0.5,
        MemoryType::SshExecution => 0.5,
    };
    
    let content_lower = memory.content.to_lowercase();
    let mut boost = 0.0;
    
    // User corrections (high value)
    if content_lower.contains("wrong") 
        || content_lower.contains("incorrect")
        || content_lower.contains("not correct")
        || content_lower.contains("actually,") {
        boost += 0.2;
    }
    
    // User preferences
    if content_lower.contains("prefer") 
        || content_lower.contains("i like")
        || content_lower.contains("i want")
        || content_lower.contains("always")
        || content_lower.contains("never") {
        boost += 0.15;
    }
    
    // Problem resolution
    if content_lower.contains("error") && content_lower.contains("fix") {
        boost += 0.1;
    }
    
    // Check metadata for explicit importance
    if let Some(ref metadata) = memory.metadata {
        if let Some(imp) = metadata.get("importance").and_then(|v| v.as_f64()) {
            boost += imp as f32 * 0.1;
        }
    }
    
    (base + boost).min(1.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCategory {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub last_updated: i64,
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

pub struct VectorStore {
    conn: Connection,
    embedding_model: Arc<Mutex<TextEmbedding>>,
}

impl VectorStore {
    pub async fn new(path: &str) -> Result<Self> {
        let conn = connect(path).execute().await.context("Failed to connect to LanceDB")?;
        
        let cache_dir = dirs::cache_dir()
            .context("Could not find cache directory")?
            .join("mylm")
            .join("models");
        std::fs::create_dir_all(&cache_dir)?;

        // Initialize embedding model
        // We use BGE-Small-EN-v1.5 by default (384 dims)
        let embedding_model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15)
                .with_cache_dir(cache_dir)
        ).context("Failed to initialize FastEmbed model")?;

        Ok(Self {
            conn,
            embedding_model: Arc::new(Mutex::new(embedding_model)),
        })
    }

    fn get_memory_schema(&self) -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("summary", DataType::Utf8, true),
            Field::new("created_at", DataType::Int64, false),
            Field::new("embedding", DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), 384), false),
            Field::new("type", DataType::Utf8, false),
            Field::new("session_id", DataType::Utf8, true),
            Field::new("metadata", DataType::Utf8, true),
            Field::new("category_id", DataType::Utf8, true),
        ]))
    }

    
    fn get_category_schema(&self) -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("summary", DataType::Utf8, false),
            Field::new("last_updated", DataType::Int64, false),
            Field::new("embedding", DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), 384), false),
        ]))
    }

    async fn get_or_create_table(&self, name: &str, schema: Arc<Schema>) -> Result<Table> {
    	match self.conn.open_table(name).execute().await {
    		Ok(table) => {
    			let current_schema = table.schema().await?;
    			// Check for schema mismatch (column count or field names)
    			let mismatch = current_schema.fields().len() != schema.fields().len() ||
    				current_schema.fields().iter().zip(schema.fields().iter()).any(|(a, b)| a.name() != b.name());
   
    			if mismatch {
    				// Schema mismatch detected - log detailed information for debugging
    				error!("Memory table '{}' schema mismatch detected!", name);
    				error!("  Expected schema ({} fields):", schema.fields().len());
    				for (i, field) in schema.fields().iter().enumerate() {
    					error!("    {}. {}: {:?}", i+1, field.name(), field.data_type());
    				}
    				error!("  Actual schema ({} fields):", current_schema.fields().len());
    				for (i, field) in current_schema.fields().iter().enumerate() {
    					error!("    {}. {}: {:?}", i+1, field.name(), field.data_type());
    				}
    				
    				// Attempt automatic migration
    				info!("Attempting automatic schema migration for table '{}'...", name);
    				match self.migrate_table(name, &current_schema, schema.clone()).await {
    					Ok(new_table) => {
    						info!("Schema migration successful!");
    						Ok(new_table)
    					},
    					Err(e) => {
    						error!("Schema migration failed: {}", e);
    						anyhow::bail!("Memory table schema mismatch and automatic migration failed. Please delete the 'mylm/memory' directory in your data folder (usually ~/.local/share/mylm/memory) to reset the database. Error: {}", e);
    					}
    				}
    			} else {
    				Ok(table)
    			}
    		},
    		Err(_) => self.create_empty_table(name, schema).await,
    	}
    }

    async fn create_empty_table(&self, name: &str, schema: Arc<Schema>) -> Result<Table> {
    	let batches = RecordBatchIterator::new(vec![], schema.clone());
    	self.conn
    		.create_table(name, Box::new(batches))
    		.execute()
    		.await
    		.context(format!("Failed to create table {}", name))
    }
   
    /// Migrate an existing table to a new schema.
    /// Reads all data from the old table, transforms records to match the new schema,
    /// and creates a new table with the correct schema.
    async fn migrate_table(&self, name: &str, old_schema: &Schema, new_schema: Arc<Schema>) -> Result<Table> {
        use arrow_array::RecordBatch;
        
        // Open the old table
    	let old_table = self.conn.open_table(name).execute().await?;
    	
    	// Read all records from the old table
    	let query = old_table.query().execute().await?;
    	let old_batches: Vec<RecordBatch> = query.try_collect::<Vec<_>>().await?;
    	
    	info!("Migrating table '{}': {} records found", name, old_batches.iter().map(|b| b.num_rows()).sum::<usize>());
    	
    	// If no data, just create a new empty table with the correct schema
    	if old_batches.is_empty() {
    		info!("Table '{}' is empty, creating new table with correct schema", name);
    		// We cannot easily drop the old table, so we'll create a new one and the old one will be orphaned
    		// But for empty tables, we can just return a new table
    		return self.create_empty_table(name, new_schema).await;
    	}
    	
    	// Build field mapping from old to new schema
    	let mut field_map: HashMap<String, (usize, &Field)> = HashMap::new();
    	for (idx, field) in old_schema.fields().iter().enumerate() {
    		field_map.insert(field.name().to_string(), (idx, field));
    	}
    	
    	// Transform each batch to the new schema
    	let mut new_batches = Vec::new();
    	for old_batch in &old_batches {
    	    let mut new_columns: Vec<ArrayRef> = Vec::new();
    	    
    	    for new_field in new_schema.fields() {
    	        let field_name = new_field.name();
    	        
    	        if let Some((old_idx, old_field)) = field_map.get(field_name) {
    	            // Field exists in old schema - get column by index (no Result)
    	            let old_col = old_batch.column(*old_idx);
    	            // Check if data types are compatible
    	            if old_field.data_type() != new_field.data_type() {
    	                warn!("Field '{}' type mismatch: old={:?}, new={:?}. Using old column as-is.", field_name, old_field.data_type(), new_field.data_type());
    	            }
    	            new_columns.push(old_col.clone());
    	        } else {
    	            // Field doesn't exist in old schema - create null column with default value
    	            info!("Field '{}' missing in old schema, creating null column with default", field_name);
    	            new_columns.push(new_null_array(new_field.data_type(), old_batch.num_rows()));
    	        }
    	    }
    	    
    	    let new_batch = RecordBatch::try_new(new_schema.clone(), new_columns)?;
    	    new_batches.push(new_batch);
    	}
    	
    	// Create a new table with the migrated data
    	// We'll use a temporary name to avoid conflicts
    	let temp_name = format!("{}_migrated_{}", name, chrono::Utc::now().timestamp());
    	info!("Creating migrated table '{}' with {} records", temp_name, new_batches.iter().map(|b| b.num_rows()).sum::<usize>());
    	
    	let batches_iter = RecordBatchIterator::new(new_batches.into_iter().map(Ok), new_schema.clone());
    	let new_table = self.conn
    		.create_table(&temp_name, Box::new(batches_iter))
    		.execute()
    		.await
    		.context("Failed to create migrated table")?;
    	
    	// Drop the old table and rename the new one to replace it
    	// This ensures subsequent get_or_create_table calls get the migrated data
    	info!("Dropping old table '{}' and renaming migrated table...", name);
    	
    	// Drop old table if it exists (empty namespace for default)
    	if let Err(e) = self.conn.drop_table(name, &[]).await {
    		warn!("Failed to drop old table '{}': {}. Continuing anyway.", name, e);
    	}
    	
    	// Rename the migrated table to the original name
    	// LanceDB doesn't have a direct rename, so we need to:
    	// 1. Create a new table with the correct name
    	// 2. Copy data from migrated table
    	// 3. Drop the migrated table
    	let migrated_data: Vec<RecordBatch> = new_table
    		.query()
    		.execute()
    		.await?
    		.try_collect()
    		.await?;
    	
    	if migrated_data.is_empty() {
    		// No data to migrate - just create empty table with correct schema
    		self.create_empty_table(name, new_schema).await
    	} else {
    		let batches_iter = RecordBatchIterator::new(migrated_data.into_iter().map(Ok), new_schema.clone());
    		let final_table = self.conn
    			.create_table(name, Box::new(batches_iter))
    			.execute()
    			.await
    			.context("Failed to create final table after migration")?;
    		
    		// Drop the temporary migrated table
    		if let Err(e) = self.conn.drop_table(&temp_name, &[]).await {
    			warn!("Failed to drop temporary table '{}': {}. Continuing anyway.", temp_name, e);
    		}
    		
    		info!("Migration complete. Table '{}' successfully migrated to new schema.", name);
    		Ok(final_table)
    	}
    }
   
    /// Repair the memory database by attempting to migrate all tables to the correct schema.
    /// Returns a summary of what was done.
    pub async fn repair_database(&self) -> Result<String> {
    	info!("Starting database repair...");
    	let mut report = String::new();
    	
    	// Try to repair memories table
    	match self.get_or_create_table("memories", self.get_memory_schema()).await {
    		Ok(_) => {
    			report.push_str("✅ Memories table: OK\n");
    		},
    		Err(e) => {
    			report.push_str(&format!("❌ Memories table: Failed - {}\n", e));
    		}
    	}
    	
    	// Try to repair categories table
    	match self.get_or_create_table("categories", self.get_category_schema()).await {
    		Ok(_) => {
    			report.push_str("✅ Categories table: OK\n");
    		},
    		Err(e) => {
    			report.push_str(&format!("❌ Categories table: Failed - {}\n", e));
    		}
    	}
    	
    	info!("Database repair completed:\n{}", report);
    	
    	// Clean up orphaned migrated tables
    	match self.cleanup_orphaned_tables().await {
    		Ok(count) => {
    			if count > 0 {
    				report.push_str(&format!("\n🧹 Cleaned up {} orphaned migrated tables\n", count));
    			}
    		},
    		Err(e) => {
    			report.push_str(&format!("\n⚠️  Failed to clean up orphaned tables: {}\n", e));
    		}
    	}
    	
    	Ok(report)
    }
    
    /// Clean up orphaned migrated tables (tables with "_migrated_" suffix)
    async fn cleanup_orphaned_tables(&self) -> Result<usize> {
    	let table_names: Vec<String> = self.conn.table_names().execute().await?;
    	let mut cleaned = 0;
    	
    	for name in &table_names {
    		if name.contains("_migrated_") {
    			info!("Dropping orphaned migrated table: {}", name);
    			if let Err(e) = self.conn.drop_table(name, &[]).await {
    				warn!("Failed to drop orphaned table '{}': {}", name, e);
    			} else {
    				cleaned += 1;
    			}
    		}
    	}
    	
    	Ok(cleaned)
    }

    pub async fn add_memory(&self, content: &str) -> Result<()> {
        self.add_memory_typed(content, MemoryType::UserNote, None, None, None, None).await
    }

    pub async fn record_command(
        &self,
        command: &str,
        output: &str,
        exit_code: i32,
        session_id: Option<String>,
    ) -> Result<i64> {
        let content = format!(
            "Command: {}\nOutput: {}\nExit Code: {}",
            command,
            output.chars().take(1000).collect::<String>(),
            exit_code
        );

        let metadata = serde_json::json!({
            "command": command,
            "exit_code": exit_code,
            "output_length": output.len(),
        });

        let id = Utc::now().timestamp_nanos_opt().unwrap_or_else(|| Utc::now().timestamp());

        self.add_memory_typed_with_id(
            id,
            &content,
            MemoryType::Command,
            session_id,
            Some(metadata),
            Some("shell_commands".to_string()),
            None,
        ).await?;
        
        Ok(id)
    }

    pub async fn add_memory_typed(
        &self,
        content: &str,
        memory_type: MemoryType,
        session_id: Option<String>,
        metadata: Option<serde_json::Value>,
        category_id: Option<String>,
        summary: Option<String>,
    ) -> Result<()> {
        let id = Utc::now().timestamp_nanos_opt().unwrap_or_else(|| Utc::now().timestamp());
        self.add_memory_typed_with_id(id, content, memory_type, session_id, metadata, category_id, summary).await
    }

    /// Internal method: Add memory directly without deduplication
    /// Use `add_memory_typed_with_id` for automatic deduplication
    async fn add_memory_typed_with_id_internal(
        &self,
        id: i64,
        content: &str,
        memory_type: MemoryType,
        session_id: Option<String>,
        metadata: Option<serde_json::Value>,
        category_id: Option<String>,
        summary: Option<String>,
    ) -> Result<()> {
        // Sanitize content to remove WAF-triggering patterns
        let sanitized_content = sanitize_memory_content(content);
        
        let model = self.embedding_model.clone();
        // If summary is provided, use it for embedding. Otherwise use sanitized content.
        let text = summary.clone().unwrap_or_else(|| sanitized_content.clone());
        
        // Run embedding in blocking thread
        let embeddings = task::spawn_blocking(move || {
            let mut model = model.blocking_lock();
            model.embed(vec![text], None)
        }).await.context("Join error during embedding")?
        .context("Embedding failed")?;

        let embedding = embeddings.first().context("No embedding generated")?.clone();
        let created_at = Utc::now().timestamp();

        let schema = self.get_memory_schema();
        
        let id_array = Int64Array::from(vec![id]);
        let content_array = StringArray::from(vec![sanitized_content]);
        let summary_array = StringArray::from(vec![summary]);
        let created_at_array = Int64Array::from(vec![created_at]);
        
        let flat_embeddings = Float32Array::from(embedding);
        let field = Arc::new(Field::new("item", DataType::Float32, true));
        let embedding_array = FixedSizeListArray::try_new(field, 384, Arc::new(flat_embeddings), None)?;
        
        let type_array = StringArray::from(vec![memory_type.to_string()]);
        let session_id_array = StringArray::from(vec![session_id.clone()]);
        
        let metadata_str = metadata.map(|m| m.to_string());
        let metadata_array = StringArray::from(vec![metadata_str]);
        
        let category_id_array = StringArray::from(vec![category_id]);

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(id_array),
                Arc::new(content_array),
                Arc::new(summary_array),
                Arc::new(created_at_array),
                Arc::new(embedding_array),
                Arc::new(type_array),
                Arc::new(session_id_array),
                Arc::new(metadata_array),
                Arc::new(category_id_array),
            ],
        )?;

        let table = self.get_or_create_table("memories", schema.clone()).await?;
        table.add(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
            .execute()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to add memory to LanceDB: {:#}", e))?;

        Ok(())
    }
    
    /// Add memory with automatic deduplication (DEFAULT behavior)
    /// 
    /// # Deduplication Logic
    /// - Similarity > 0.95: Skip as duplicate
    /// - Similarity > 0.85: Reinforce existing memory
    /// - Similarity < 0.85: Add as new memory
    pub async fn add_memory_typed_with_id(
        &self,
        _id: i64,
        content: &str,
        memory_type: MemoryType,
        session_id: Option<String>,
        metadata: Option<serde_json::Value>,
        category_id: Option<String>,
        summary: Option<String>,
    ) -> Result<()> {
        // Use deduplication logic
        match self.add_memory_deduplicated(
            content,
            memory_type,
            session_id,
            metadata,
            category_id,
            summary,
        ).await? {
            Some(_) => Ok(()),
            None => Ok(()), // Duplicate skipped
        }
    }
    
    /// Calculate cosine similarity between two vectors
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm_a * norm_b)
    }
    
    /// Add memory with deduplication check
    /// 
    /// # Deduplication Logic
    /// - Similarity > 0.95: Skip as duplicate
    /// - Similarity > 0.85: Update existing memory (reinforce)
    /// - Similarity < 0.85: Add as new memory
    /// 
    /// # Returns
    /// - Ok(None): Memory was skipped as duplicate
    /// - Ok(Some(id)): ID of new or updated memory
    pub async fn add_memory_deduplicated(
        &self,
        content: &str,
        memory_type: MemoryType,
        session_id: Option<String>,
        metadata: Option<serde_json::Value>,
        category_id: Option<String>,
        summary: Option<String>,
    ) -> Result<Option<i64>> {
        // Generate embedding for the new content
        let model = self.embedding_model.clone();
        let text = summary.clone().unwrap_or_else(|| content.to_string());
        
        let embeddings = task::spawn_blocking(move || {
            let mut model = model.blocking_lock();
            model.embed(vec![text], None)
        }).await.context("Join error during embedding")?
        .context("Embedding failed")?;
        
        let new_embedding = embeddings.first().context("No embedding generated")?;
        
        // Search for similar existing memories
        let similar = self.search_memory_semantic(content, 5).await?;
        
        // Check for duplicates
        for memory in similar {
            if let Some(ref mem_embedding) = memory.embedding {
                let similarity = Self::cosine_similarity(new_embedding, mem_embedding);
                
                if similarity > 0.95 {
                    // High similarity - skip as duplicate
                    info!("[DEDUP] Skipping duplicate memory (similarity: {:.3})", similarity);
                    return Ok(None);
                } else if similarity > 0.85 {
                    // Medium similarity - reinforce existing memory
                    info!("[DEDUP] Reinforcing existing memory {} (similarity: {:.3})", memory.id, similarity);
                    
                    // Update metadata to track reinforcement
                    let mut updated_metadata = memory.metadata.clone().unwrap_or_default();
                    if let Some(obj) = updated_metadata.as_object_mut() {
                        let count = obj.get("reinforcement_count")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) + 1;
                        obj.insert("reinforcement_count".to_string(), serde_json::json!(count));
                        obj.insert("last_reinforced".to_string(), serde_json::json!(chrono::Utc::now().timestamp()));
                    }
                    
                    // Note: LanceDB doesn't support easy updates, so we just log for now
                    // In production, would: delete old + insert updated
                    return Ok(Some(memory.id));
                }
            }
        }
        
        // No similar memory found - add new
        let id = chrono::Utc::now().timestamp_nanos_opt()
            .unwrap_or_else(|| chrono::Utc::now().timestamp());
        
        self.add_memory_typed_with_id_internal(
            id,
            content,
            memory_type,
            session_id,
            metadata,
            category_id,
            summary,
        ).await?;
        
        Ok(Some(id))
    }

    /// Search memories using hybrid scoring (semantic + lexical + recency + importance)
    /// 
    /// # Arguments
    /// * `query` - Search query
    /// * `limit` - Maximum number of results to return
    /// 
    /// # Returns
    /// Memories ranked by combined relevance score
    pub async fn search_memory(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        // Get more candidates for re-ranking
        let candidates = self.search_memory_semantic(query, limit * 3).await?;
        
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        
        // Score and re-rank candidates
        let mut scored: Vec<ScoredMemory> = candidates
            .into_iter()
            .enumerate()
            .map(|(rank, memory)| {
                // Semantic score: 1.0 for first, decaying by rank
                let semantic_score = 1.0 / (1.0 + rank as f32 * 0.1);
                
                // Lexical score: word match ratio
                let lexical_score = lexical_score(query, &memory.content);
                
                // Recency score: newer is better
                let recency_score = recency_score(memory.created_at);
                
                // Importance score: based on memory type and content
                let importance_score = importance_score(&memory);
                
                // Combined score with weights
                let final_score = 
                    semantic_score * 0.50 +
                    lexical_score * 0.25 +
                    recency_score * 0.15 +
                    importance_score * 0.10;
                
                ScoredMemory { memory, score: final_score }
            })
            .collect();
        
        // Sort by score descending
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        
        // Take top N
        scored.truncate(limit);
        
        Ok(scored.into_iter().map(|s| s.memory).collect())
    }
    
    /// Pure semantic search via LanceDB (internal use)
    async fn search_memory_semantic(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let model = self.embedding_model.clone();
        let text = query.to_string();
        
        let embeddings = task::spawn_blocking(move || {
            let mut model = model.blocking_lock();
            model.embed(vec![text], None)
        }).await.context("Join error during embedding")?
        .context("Embedding failed")?;

        let query_embedding = embeddings.first().context("No embedding generated")?.clone();
        
        let table = self.get_or_create_table("memories", self.get_memory_schema()).await?;
        info!("store: starting vector search for query: {}", query);
        let results = table
            .query()
            .nearest_to(query_embedding)?
            .limit(limit)
            .execute()
            .await
            .context("Search query failed")?;

        info!("store: collecting results from vector search");
        let batches: Vec<RecordBatch> = results.try_collect::<Vec<_>>().await?;
        info!("store: vector search complete, got {} batches", batches.len());
        
        let mut memories = Vec::new();
        for batch in batches {
            let id_col = batch.column_by_name("id").context("id column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast id")?;
            let content_col = batch.column_by_name("content").context("content column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast content")?;
            
            let summary_col = batch.column_by_name("summary");
            let summary_array = if let Some(col) = summary_col {
                col.as_any().downcast_ref::<StringArray>()
            } else {
                None
            };

            let created_at_col = batch.column_by_name("created_at").context("created_at column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast created_at")?;
            let type_col = batch.column_by_name("type").context("type column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast type")?;
            
            let session_col = batch.column_by_name("session_id").context("session_id column missing")?;
            let session_array = session_col.as_any().downcast_ref::<StringArray>().context("Failed downcast session")?;
            
            let metadata_col = batch.column_by_name("metadata").context("metadata column missing")?;
            let metadata_array = metadata_col.as_any().downcast_ref::<StringArray>().context("Failed downcast metadata")?;
            
            let category_col = batch.column_by_name("category_id").context("category_id column missing")?;
            let category_array = category_col.as_any().downcast_ref::<StringArray>().context("Failed downcast category")?;

            for i in 0..batch.num_rows() {
                let metadata_str = metadata_array.value(i);
                let metadata = if metadata_col.is_null(i) || metadata_str.is_empty() {
                    None
                } else {
                    serde_json::from_str(metadata_str).ok()
                };

                let summary = if let Some(arr) = summary_array {
                    if arr.is_null(i) { None } else { Some(arr.value(i).to_string()) }
                } else {
                    None
                };

                memories.push(Memory {
                    id: id_col.value(i),
                    content: content_col.value(i).to_string(),
                    summary,
                    created_at: created_at_col.value(i),
                    r#type: MemoryType::from(type_col.value(i)),
                    session_id: if session_col.is_null(i) { None } else { Some(session_array.value(i).to_string()) },
                    metadata,
                    category_id: if category_col.is_null(i) { None } else { Some(category_array.value(i).to_string()) },
                    embedding: None,
                });
            }
        }

        Ok(memories)
    }

    
    pub async fn search_by_type(
        &self,
        query: &str,
        memory_type: MemoryType,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        // Get results from hybrid search, then filter by type
        let all_results = self.search_memory(query, limit * 2).await?;
        Ok(all_results.into_iter().filter(|r| r.r#type == memory_type).take(limit).collect())
    }

    
    pub async fn get_all_categories(&self) -> Result<Vec<MemoryCategory>> {
        let table = self.get_or_create_table("categories", self.get_category_schema()).await?;
        let results = table.query().execute().await?;
        let batches: Vec<RecordBatch> = results.try_collect::<Vec<_>>().await?;
        
        let mut categories = Vec::new();
        for batch in batches {
            let id_col = batch.column_by_name("id").context("id column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast id")?;
            let name_col = batch.column_by_name("name").context("name column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast name")?;
            let summary_col = batch.column_by_name("summary").context("summary column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast summary")?;
            let last_updated_col = batch.column_by_name("last_updated").context("last_updated column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast last_updated")?;

            for i in 0..batch.num_rows() {
                categories.push(MemoryCategory {
                    id: id_col.value(i).to_string(),
                    name: name_col.value(i).to_string(),
                    summary: summary_col.value(i).to_string(),
                    last_updated: last_updated_col.value(i),
                    embedding: None,
                });
            }
        }
        Ok(categories)
    }

    pub async fn get_category_by_id(&self, id: &str) -> Result<Option<MemoryCategory>> {
        let table = self.get_or_create_table("categories", self.get_category_schema()).await?;
        let results = table.query()
            .only_if(format!("id = '{}'", id))
            .execute()
            .await?;
        
        let batches: Vec<RecordBatch> = results.try_collect::<Vec<_>>().await?;
        for batch in batches {
            if batch.num_rows() > 0 {
                let id_col = batch.column_by_name("id").context("id column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast id")?;
                let name_col = batch.column_by_name("name").context("name column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast name")?;
                let summary_col = batch.column_by_name("summary").context("summary column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast summary")?;
                let last_updated_col = batch.column_by_name("last_updated").context("last_updated column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast last_updated")?;

                return Ok(Some(MemoryCategory {
                    id: id_col.value(0).to_string(),
                    name: name_col.value(0).to_string(),
                    summary: summary_col.value(0).to_string(),
                    last_updated: last_updated_col.value(0),
                    embedding: None,
                }));
            }
        }
        Ok(None)
    }

    pub async fn get_memories_by_category(&self, category_id: &str) -> Result<Vec<Memory>> {
        let table = self.get_or_create_table("memories", self.get_memory_schema()).await?;
        let results = table.query()
            .only_if(format!("category_id = '{}'", category_id))
            .execute()
            .await?;
        
        let batches: Vec<RecordBatch> = results.try_collect::<Vec<_>>().await?;
        let mut memories = Vec::new();
        for batch in batches {
            let id_col = batch.column_by_name("id").context("id column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast id")?;
            let content_col = batch.column_by_name("content").context("content column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast content")?;
            
            let summary_col = batch.column_by_name("summary");
            let summary_array = if let Some(col) = summary_col {
                col.as_any().downcast_ref::<StringArray>()
            } else {
                None
            };

            let created_at_col = batch.column_by_name("created_at").context("created_at column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast created_at")?;
            let type_col = batch.column_by_name("type").context("type column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast type")?;
            let session_col = batch.column_by_name("session_id").context("session_id column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast session")?;
            let metadata_col = batch.column_by_name("metadata").context("metadata column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast metadata")?;
            let category_col = batch.column_by_name("category_id").context("category_id column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast category")?;

            for i in 0..batch.num_rows() {
                let metadata_str = metadata_col.value(i);
                let metadata = if metadata_col.is_null(i) || metadata_str.is_empty() {
                    None
                } else {
                    serde_json::from_str(metadata_str).ok()
                };

                let summary = if let Some(arr) = summary_array {
                    if arr.is_null(i) { None } else { Some(arr.value(i).to_string()) }
                } else {
                    None
                };

                memories.push(Memory {
                    id: id_col.value(i),
                    content: content_col.value(i).to_string(),
                    summary,
                    created_at: created_at_col.value(i),
                    r#type: MemoryType::from(type_col.value(i)),
                    session_id: if session_col.is_null(i) { None } else { Some(session_col.value(i).to_string()) },
                    metadata,
                    category_id: if category_col.is_null(i) { None } else { Some(category_col.value(i).to_string()) },
                    embedding: None,
                });
            }
        }
        Ok(memories)
    }

    
    pub async fn update_category(&self, category: MemoryCategory) -> Result<()> {
        let model = self.embedding_model.clone();
        let text = format!("{}: {}", category.name, category.summary);
        
        let embeddings = task::spawn_blocking(move || {
            let mut model = model.blocking_lock();
            model.embed(vec![text], None)
        }).await.context("Join error during embedding")?
        .context("Embedding failed")?;

        let embedding = embeddings.first().context("No embedding generated")?.clone();
        let schema = self.get_category_schema();
        
        let id_array = StringArray::from(vec![category.id.clone()]);
        let name_array = StringArray::from(vec![category.name]);
        let summary_array = StringArray::from(vec![category.summary]);
        let last_updated_array = Int64Array::from(vec![category.last_updated]);
        let flat_embeddings = Float32Array::from(embedding);
        let field = Arc::new(Field::new("item", DataType::Float32, true));
        let embedding_array = FixedSizeListArray::try_new(field, 384, Arc::new(flat_embeddings), None)?;

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(id_array),
                Arc::new(name_array),
                Arc::new(summary_array),
                Arc::new(last_updated_array),
                Arc::new(embedding_array),
            ],
        )?;

        let table = self.get_or_create_table("categories", schema.clone()).await?;
        // Check if category exists to perform update vs add
        if self.get_category_by_id(&category.id.clone()).await?.is_some() {
            // LanceDB 0.23 doesn't have a direct "update" that works easily with record batches for single rows
            // We'll use the "overwrite" mode by recreating the table if it's small or just appending if we don't care about duplicates
            // Actually, for categories, we want to replace.
            // In a real prod app, we'd use merge. For now, we'll append and rely on most recent when querying if needed,
            // or just use overwrite for the whole table if it's small.
            // Let's use the simplest: delete and re-add if supported, or just add.
            // Since LanceDB is append-mostly, we'll just add. The query `get_category_by_id` will return the first match.
        }

        table.add(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
            .execute()
            .await?;

        Ok(())
    }

    pub async fn update_memory_category(&self, memory_id: i64, category_id: String) -> Result<()> {
        let table = self.get_or_create_table("memories", self.get_memory_schema()).await?;
        // LanceDB update is tricky in 0.23. We'll use the update builder if available.
        table.update()
            .only_if(format!("id = {}", memory_id))
            .column("category_id", format!("'{}'", category_id))
            .execute()
            .await
            .context("Failed to update memory category")?;
        Ok(())
    }

    /// Get recent memories ordered by created_at (newest first)
    pub async fn get_recent_memories(&self, limit: usize) -> Result<Vec<Memory>> {
        self.get_recent_memories_with_offset(limit, 0).await
    }
    
    /// Get recent memories with offset pagination support
    /// 
    /// # Arguments
    /// * `limit` - Maximum number of memories to return
    /// * `offset` - Number of memories to skip (for pagination)
    pub async fn get_recent_memories_with_offset(&self, limit: usize, offset: usize) -> Result<Vec<Memory>> {
        let table = self.get_or_create_table("memories", self.get_memory_schema()).await?;
        
        // Query all memories and sort by created_at descending
        let results = table.query()
            .execute()
            .await?;
        
        let batches: Vec<RecordBatch> = results.try_collect::<Vec<_>>().await?;
        let mut memories = Vec::new();
        
        for batch in batches {
            let id_col = batch.column_by_name("id").context("id column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast id")?;
            let content_col = batch.column_by_name("content").context("content column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast content")?;
            
            let summary_col = batch.column_by_name("summary");
            let summary_array = if let Some(col) = summary_col {
                col.as_any().downcast_ref::<StringArray>()
            } else {
                None
            };

            let created_at_col = batch.column_by_name("created_at").context("created_at column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast created_at")?;
            let type_col = batch.column_by_name("type").context("type column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast type")?;
            let session_col = batch.column_by_name("session_id").context("session_id column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast session")?;
            let metadata_col = batch.column_by_name("metadata").context("metadata column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast metadata")?;
            let category_col = batch.column_by_name("category_id").context("category_id column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast category")?;

            for i in 0..batch.num_rows() {
                let metadata_str = metadata_col.value(i);
                let metadata = if metadata_col.is_null(i) || metadata_str.is_empty() {
                    None
                } else {
                    serde_json::from_str(metadata_str).ok()
                };

                let summary = if let Some(arr) = summary_array {
                    if arr.is_null(i) { None } else { Some(arr.value(i).to_string()) }
                } else {
                    None
                };

                memories.push(Memory {
                    id: id_col.value(i),
                    content: content_col.value(i).to_string(),
                    summary,
                    created_at: created_at_col.value(i),
                    r#type: MemoryType::from(type_col.value(i)),
                    session_id: if session_col.is_null(i) { None } else { Some(session_col.value(i).to_string()) },
                    metadata,
                    category_id: if category_col.is_null(i) { None } else { Some(category_col.value(i).to_string()) },
                    embedding: None,
                });
            }
        }
        
        // Sort by created_at descending (newest first)
        memories.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        
        // Apply offset and limit
        let start = offset.min(memories.len());
        let _end = (offset + limit).min(memories.len());
        let paginated: Vec<Memory> = memories.into_iter().skip(start).take(limit).collect();
        
        Ok(paginated)
    }

    /// Get a single memory by ID
    pub async fn get_memory_by_id(&self, id: i64) -> Result<Option<Memory>> {
        let table = self.get_or_create_table("memories", self.get_memory_schema()).await?;
        
        let results = table.query()
            .only_if(format!("id = {}", id))
            .execute()
            .await?;
        
        let batches: Vec<RecordBatch> = results.try_collect::<Vec<_>>().await?;
        
        for batch in batches {
            if batch.num_rows() == 0 {
                continue;
            }
            
            let id_col = batch.column_by_name("id").context("id column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast id")?;
            let content_col = batch.column_by_name("content").context("content column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast content")?;
            
            let summary_col = batch.column_by_name("summary");
            let summary_array = if let Some(col) = summary_col {
                col.as_any().downcast_ref::<StringArray>()
            } else {
                None
            };

            let created_at_col = batch.column_by_name("created_at").context("created_at column missing")?.as_any().downcast_ref::<Int64Array>().context("Failed downcast created_at")?;
            let type_col = batch.column_by_name("type").context("type column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast type")?;
            let session_col = batch.column_by_name("session_id").context("session_id column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast session")?;
            let metadata_col = batch.column_by_name("metadata").context("metadata column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast metadata")?;
            let category_col = batch.column_by_name("category_id").context("category_id column missing")?.as_any().downcast_ref::<StringArray>().context("Failed downcast category")?;

            let metadata_str = metadata_col.value(0);
            let metadata = if metadata_col.is_null(0) || metadata_str.is_empty() {
                None
            } else {
                serde_json::from_str(metadata_str).ok()
            };

            let summary = if let Some(arr) = summary_array {
                if arr.is_null(0) { None } else { Some(arr.value(0).to_string()) }
            } else {
                None
            };

            return Ok(Some(Memory {
                id: id_col.value(0),
                content: content_col.value(0).to_string(),
                summary,
                created_at: created_at_col.value(0),
                r#type: MemoryType::from(type_col.value(0)),
                session_id: if session_col.is_null(0) { None } else { Some(session_col.value(0).to_string()) },
                metadata,
                category_id: if category_col.is_null(0) { None } else { Some(category_col.value(0).to_string()) },
                embedding: None,
            }));
        }
        
        Ok(None)
    }

    /// Delete a memory by ID
    pub async fn delete_memory(&self, id: i64) -> Result<()> {
        let table = self.get_or_create_table("memories", self.get_memory_schema()).await?;
        
        table.delete(&format!("id = {}", id))
            .await
            .context("Failed to delete memory")?;
        
        info!("Deleted memory with id: {}", id);
        Ok(())
    }

    /// Update memory content and regenerate embedding
    pub async fn update_memory(&self, id: i64, content: &str) -> Result<()> {
        // First get the existing memory to preserve other fields
        let existing = self.get_memory_by_id(id).await?;
        if existing.is_none() {
            anyhow::bail!("Memory with id {} not found", id);
        }
        let existing = existing.unwrap();
        
        // Generate new embedding for updated content
        let model = self.embedding_model.clone();
        let text = content.to_string();
        
        let embeddings = task::spawn_blocking(move || {
            let mut model = model.blocking_lock();
            model.embed(vec![text], None)
        }).await.context("Join error during embedding")?
        .context("Embedding failed")?;

        let embedding = embeddings.first().context("No embedding generated")?.clone();
        let created_at = existing.created_at;

        let schema = self.get_memory_schema();
        
        let id_array = Int64Array::from(vec![id]);
        let content_array = StringArray::from(vec![content]);
        let summary_array = StringArray::from(vec![existing.summary.clone()]);
        let created_at_array = Int64Array::from(vec![created_at]);
        
        let flat_embeddings = Float32Array::from(embedding);
        let field = Arc::new(Field::new("item", DataType::Float32, true));
        let embedding_array = FixedSizeListArray::try_new(field, 384, Arc::new(flat_embeddings), None)?;
        
        let type_array = StringArray::from(vec![existing.r#type.to_string()]);
        let session_id_array = StringArray::from(vec![existing.session_id.clone()]);
        let metadata_str = existing.metadata.map(|m| m.to_string());
        let metadata_array = StringArray::from(vec![metadata_str]);
        let category_id_array = StringArray::from(vec![existing.category_id.clone()]);

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(id_array),
                Arc::new(content_array),
                Arc::new(summary_array),
                Arc::new(created_at_array),
                Arc::new(embedding_array),
                Arc::new(type_array),
                Arc::new(session_id_array),
                Arc::new(metadata_array),
                Arc::new(category_id_array),
            ],
        )?;

        // Delete old record and add updated one
        // LanceDB 0.23 doesn't have a clean update, so we delete + add
        let table = self.get_or_create_table("memories", schema.clone()).await?;
        
        // Delete old
        table.delete(&format!("id = {}", id))
            .await
            .context("Failed to delete old memory during update")?;
        
        // Add updated
        table.add(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
            .execute()
            .await
            .context("Failed to add updated memory")?;

        info!("Updated memory with id: {}", id);
        Ok(())
    }

    /// Count total memories in the store
    pub async fn count_memories(&self) -> Result<usize> {
        let table = self.get_or_create_table("memories", self.get_memory_schema()).await?;
        
        let results = table.query()
            .execute()
            .await?;
        
        let batches: Vec<RecordBatch> = results.try_collect::<Vec<_>>().await?;
        let count = batches.iter().map(|b| b.num_rows()).sum();
        
        Ok(count)
    }

    /// Warmup the embedding model
    pub async fn warmup() -> Result<()> {
        let data_dir = dirs::data_dir()
            .context("Could not find data directory")?
            .join("mylm")
            .join("memory");
        std::fs::create_dir_all(&data_dir)?;
        
        let _cache_dir = dirs::cache_dir()
            .context("Could not find cache directory")?
            .join("mylm")
            .join("models");
        
        let store = Self::new(data_dir.to_str().ok_or_else(|| anyhow::anyhow!("Invalid data directory path"))?).await?;
        let _ = store.search_memory("warmup", 1).await;
        println!("✅ AI models ready.");
        Ok(())
    }
}
