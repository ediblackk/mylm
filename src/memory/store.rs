use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::{Context, Result};
use arrow_array::{RecordBatch, RecordBatchIterator, StringArray, Float32Array, Int64Array, FixedSizeListArray};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, Table};
use lance_arrow::FixedSizeListArrayExt;
use serde::{Deserialize, Serialize};
use tokio::task;
use futures::TryStreamExt;

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Memory {
    pub id: i64,
    pub content: String,
    pub created_at: i64,
    pub embedding: Vec<f32>,
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
        // We use BGE-Small-EN-v1.5 by default
        let embedding_model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15)
                .with_cache_dir(cache_dir)
        ).context("Failed to initialize FastEmbed model")?;

        Ok(Self {
            conn,
            embedding_model: Arc::new(Mutex::new(embedding_model)),
        })
    }

    fn get_schema(&self) -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("created_at", DataType::Int64, false),
            Field::new("embedding", DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), 384), false),
        ]))
    }

    async fn get_or_create_table(&self) -> Result<Table> {
        let table_name = "memories";
        match self.conn.open_table(table_name).execute().await {
            Ok(table) => Ok(table),
            Err(_) => {
                // Create empty table with schema
                let schema = self.get_schema();
                let batches = RecordBatchIterator::new(vec![], schema.clone());
                self.conn
                    .create_table(table_name, Box::new(batches))
                    .execute()
                    .await
                    .context("Failed to create memories table")
            }
        }
    }

    pub async fn add_memory(&self, content: &str) -> Result<()> {
        let model = self.embedding_model.clone();
        let text = content.to_string();
        
        // Run embedding in blocking thread
        let embeddings = task::spawn_blocking(move || {
            let mut model = model.blocking_lock();
            model.embed(vec![text], None)
        }).await.context("Join error during embedding")?
        .context("Embedding failed")?;

        let embedding = embeddings.first().context("No embedding generated")?.clone();
        let id = Utc::now().timestamp_nanos_opt().unwrap_or_else(|| Utc::now().timestamp());
        let created_at = Utc::now().timestamp();

        let schema = self.get_schema();
        
        let id_array = Int64Array::from(vec![id]);
        let content_array = StringArray::from(vec![content]);
        let created_at_array = Int64Array::from(vec![created_at]);
        
        let flat_embeddings = Float32Array::from(embedding);
        let embedding_array = FixedSizeListArray::try_new_from_values(flat_embeddings, 384)?;

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(id_array),
                Arc::new(content_array),
                Arc::new(created_at_array),
                Arc::new(embedding_array),
            ],
        )?;

        let table = self.get_or_create_table().await?;
        table.add(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
            .execute()
            .await
            .context("Failed to add memory to LanceDB")?;

        Ok(())
    }

    pub async fn search_memory(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        let model = self.embedding_model.clone();
        let text = query.to_string();
        
        let embeddings = task::spawn_blocking(move || {
            let mut model = model.blocking_lock();
            model.embed(vec![text], None)
        }).await.context("Join error during embedding")?
        .context("Embedding failed")?;

        let query_embedding = embeddings.first().context("No embedding generated")?.clone();
        
        let table = self.get_or_create_table().await?;
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
            let content_col = batch
                .column_by_name("content")
                .context("Content column missing")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Failed to downcast content column")?;

            for i in 0..batch.num_rows() {
                memories.push(content_col.value(i).to_string());
            }
        }

        Ok(memories)
    }

    /// Warmup the embedding model (triggers download if needed)
    pub async fn warmup() -> Result<()> {
        let data_dir = dirs::data_dir()
            .context("Could not find data directory")?
            .join("mylm")
            .join("memory");
        std::fs::create_dir_all(&data_dir)?;
        
        let cache_dir = dirs::cache_dir()
            .context("Could not find cache directory")?
            .join("mylm")
            .join("models");
        
        let model_exists = cache_dir.join("models--fastembed--bge-small-en-v1.5").exists();
        
        if !model_exists {
            println!("⏳ Downloading AI models (this may take a minute)...");
        } else {
            println!("⏳ Warming up AI models...");
        }

        let store = Self::new(data_dir.to_str().unwrap()).await?;
        let _ = store.search_memory("warmup", 1).await;
        println!("✅ AI models ready.");
        Ok(())
    }
}
