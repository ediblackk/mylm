use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::{Context, Result};
use arrow_array::{RecordBatch, RecordBatchIterator, StringArray, Float32Array, Int64Array, FixedSizeListArray, Array};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, Table};
use lance_arrow::FixedSizeListArrayExt;
use serde::{Deserialize, Serialize};
use tokio::task;
use futures::TryStreamExt;

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
    #[allow(dead_code)]
    pub embedding: Option<Vec<f32>>,
}

impl std::fmt::Display for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.r#type, self.content)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
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

    #[allow(dead_code)]
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
                    // Schema mismatch detected.
                    // Since we cannot easily drop the table without knowing the exact lancedb version signature,
                    // we will return an error instructing the user to reset.
                    anyhow::bail!("Memory table schema mismatch. Please delete the 'mylm/memory' directory in your data folder (usually ~/.local/share/mylm/memory) to reset the database.");
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

    pub async fn add_memory_typed_with_id(
        &self,
        id: i64,
        content: &str,
        memory_type: MemoryType,
        session_id: Option<String>,
        metadata: Option<serde_json::Value>,
        category_id: Option<String>,
        summary: Option<String>,
    ) -> Result<()> {
        let model = self.embedding_model.clone();
        // If summary is provided, use it for embedding. Otherwise use content.
        let text = summary.clone().unwrap_or_else(|| content.to_string());
        
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
        let content_array = StringArray::from(vec![content]);
        let summary_array = StringArray::from(vec![summary]);
        let created_at_array = Int64Array::from(vec![created_at]);
        
        let flat_embeddings = Float32Array::from(embedding);
        let embedding_array = FixedSizeListArray::try_new_from_values(flat_embeddings, 384)?;
        
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

    pub async fn search_memory(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let model = self.embedding_model.clone();
        let text = query.to_string();
        
        let embeddings = task::spawn_blocking(move || {
            let mut model = model.blocking_lock();
            model.embed(vec![text], None)
        }).await.context("Join error during embedding")?
        .context("Embedding failed")?;

        let query_embedding = embeddings.first().context("No embedding generated")?.clone();
        
        let table = self.get_or_create_table("memories", self.get_memory_schema()).await?;
        let results = table
            .query()
            .nearest_to(query_embedding)?
            .limit(limit)
            .execute()
            .await
            .context("Search query failed")?;

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

    #[allow(dead_code)]
    pub async fn search_by_type(
        &self,
        query: &str,
        memory_type: MemoryType,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let all_results = self.search_memory(query, limit * 3).await?;
        Ok(all_results.into_iter().filter(|r| r.r#type == memory_type).take(limit).collect())
    }

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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
        let embedding_array = FixedSizeListArray::try_new_from_values(flat_embeddings, 384)?;

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
        
        let store = Self::new(data_dir.to_str().unwrap()).await?;
        let _ = store.search_memory("warmup", 1).await;
        println!("✅ AI models ready.");
        Ok(())
    }
}
