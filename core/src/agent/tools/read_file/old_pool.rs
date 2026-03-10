//! Chunk pool for managing persistent file chunk workers
//!
//! The ChunkPool maintains active chunk workers for large files,
//! allowing follow-up queries without re-reading and re-processing.
//! Workers persist until the session ends.

use super::types::{ChunkSummary, FileChunk, ReadError};
use crate::agent::types::events::WorkerId;
use crate::agent::runtime::core::ToolCapability;
use crate::agent::runtime::orchestrator::commonbox::Commonbox;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use tokio::sync::{mpsc, RwLock};

/// Maximum retry attempts for a failed chunk worker
const MAX_CHUNK_RETRIES: u32 = 3;

/// A query sent to a chunk worker
#[derive(Debug, Clone)]
pub struct ChunkQuery {
    /// The question or query about the chunk content
    pub question: String,
    /// Response channel
    pub response_tx: mpsc::Sender<ChunkQueryResponse>,
}

/// Response from a chunk worker query
#[derive(Debug, Clone)]
pub struct ChunkQueryResponse {
    /// Whether the chunk contains relevant information
    pub is_relevant: bool,
    /// Answer or relevant excerpt from the chunk
    pub answer: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
}

/// An active chunk worker
#[derive(Debug)]
pub struct ActiveChunk {
    /// Chunk identifier
    pub chunk_id: usize,
    /// Worker ID from the runtime
    pub worker_id: WorkerId,
    /// Line range this chunk covers
    pub line_range: (usize, usize),
    /// Summary of chunk content
    pub summary: String,
    /// Key terms extracted from chunk
    pub key_terms: Vec<String>,
    /// Content hash for cache validation
    pub content_hash: String,
    /// Channel to send queries to this worker
    pub query_tx: mpsc::Sender<ChunkQuery>,
}

impl Clone for ActiveChunk {
    fn clone(&self) -> Self {
        Self {
            chunk_id: self.chunk_id,
            worker_id: self.worker_id.clone(),
            line_range: self.line_range,
            summary: self.summary.clone(),
            key_terms: self.key_terms.clone(),
            content_hash: self.content_hash.clone(),
            query_tx: self.query_tx.clone(),
        }
    }
}

/// Pool of active chunk workers
/// 
/// The ChunkPool manages workers for large files that have been read
/// using the chunked strategy. Workers remain active until the session ends.
pub struct ChunkPool {
    /// Session identifier for this pool
    session_id: String,
    /// Active chunks organized by file path
    chunks: Arc<RwLock<HashMap<PathBuf, Vec<ActiveChunk>>>>,
    /// Maximum number of persistent workers allowed
    max_workers: usize,
    /// Current worker count
    worker_count: Arc<RwLock<usize>>,
    /// Commonbox for job tracking and coordination
    commonbox: Arc<Commonbox>,
    /// Factory for creating worker sessions (RwLock for late initialization)
    factory: StdRwLock<Option<crate::agent::AgentSessionFactory>>,
    /// Output sender for worker events
    output_tx: StdRwLock<Option<crate::agent::runtime::orchestrator::OutputSender>>,
    /// Worker LLM context window size (for chunk sizing)
    worker_context_window: usize,
}

impl ChunkPool {
    /// Create a new chunk pool for a session
    /// 
    /// # Arguments
    /// * `session_id` - Unique identifier for this session
    /// * `max_workers` - Maximum number of concurrent chunk workers (1-50)
    /// * `worker_context_window` - Context window size of the worker LLM (for optimal chunk sizing)
    pub fn new(
        session_id: impl Into<String>,
        max_workers: usize,
        worker_context_window: usize,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            chunks: Arc::new(RwLock::new(HashMap::new())),
            max_workers: max_workers.max(1).min(50),
            worker_count: Arc::new(RwLock::new(0)),
            commonbox: Arc::new(Commonbox::new()),
            factory: StdRwLock::new(None),
            output_tx: StdRwLock::new(None),
            worker_context_window: worker_context_window.max(4096), // Minimum 4K context
        }
    }
    
    /// Create a new chunk pool with commonbox for coordination
    pub fn with_commonbox(
        session_id: impl Into<String>,
        max_workers: usize,
        commonbox: Arc<Commonbox>,
        worker_context_window: usize,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            chunks: Arc::new(RwLock::new(HashMap::new())),
            max_workers: max_workers.max(1).min(50),
            worker_count: Arc::new(RwLock::new(0)),
            commonbox,
            factory: StdRwLock::new(None),
            output_tx: StdRwLock::new(None),
            worker_context_window: worker_context_window.max(4096),
        }
    }
    
    /// Get the worker context window size
    pub fn worker_context_window(&self) -> usize {
        self.worker_context_window
    }
    
    /// Set the worker context window size (for late configuration)
    pub fn set_worker_context_window(&mut self, context_window: usize) {
        self.worker_context_window = context_window.max(4096);
    }
    
    /// Set the session factory for spawning workers (for builder pattern)
    pub fn with_factory(self, factory: crate::agent::AgentSessionFactory) -> Self {
        *self.factory.write().unwrap() = Some(factory);
        self
    }
    
    /// Set the factory after creation (for late initialization)
    pub fn set_factory(&self, factory: crate::agent::AgentSessionFactory) {
        *self.factory.write().unwrap() = Some(factory);
    }
    
    /// Set the output sender for worker events (for builder pattern)
    pub fn with_output_sender(self, output_tx: crate::agent::runtime::orchestrator::OutputSender) -> Self {
        *self.output_tx.write().unwrap() = Some(output_tx);
        self
    }
    
    /// Set the output sender after creation (for late initialization)
    pub fn set_output_sender(&self, output_tx: crate::agent::runtime::orchestrator::OutputSender) {
        *self.output_tx.write().unwrap() = Some(output_tx);
    }
    
    /// Set the output sender optionally (for builder pattern)
    pub fn with_output_sender_optional(self, output_tx: Option<crate::agent::runtime::orchestrator::OutputSender>) -> Self {
        *self.output_tx.write().unwrap() = output_tx;
        self
    }
    
    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    
    /// Get max workers limit
    pub fn max_workers(&self) -> usize {
        self.max_workers
    }
    
    /// Check if we can spawn more workers
    pub async fn can_spawn(&self) -> bool {
        let count = *self.worker_count.read().await;
        count < self.max_workers
    }
    
    /// Get current worker count
    pub async fn worker_count(&self) -> usize {
        *self.worker_count.read().await
    }
    
    /// Register a chunk for a file
    /// 
    /// This is called when a chunk worker is successfully spawned
    pub async fn register_chunk(
        &self,
        file_path: PathBuf,
        chunk: ActiveChunk,
    ) -> Result<(), ReadError> {
        let mut chunks = self.chunks.write().await;
        let mut count = self.worker_count.write().await;
        
        if *count >= self.max_workers {
            return Err(ReadError::InvalidArgument(
                format!("Maximum persistent workers ({}) reached", self.max_workers)
            ));
        }
        
        chunks.entry(file_path).or_default().push(chunk);
        *count += 1;
        
        Ok(())
    }
    
    /// Get all active chunks for a file
    pub async fn get_file_chunks(&self, path: &PathBuf) -> Vec<ActiveChunk> {
        let chunks = self.chunks.read().await;
        chunks.get(path).map(|v| v.clone()).unwrap_or_default()
    }
    
    /// Update a chunk's summary and key terms
    /// 
    /// Called when a worker completes its initial analysis
    pub async fn update_chunk_summary(
        &self,
        file_path: &PathBuf,
        chunk_id: usize,
        summary: String,
        key_terms: Vec<String>,
    ) -> Result<(), ReadError> {
        let mut chunks = self.chunks.write().await;
        
        if let Some(file_chunks) = chunks.get_mut(file_path) {
            if let Some(chunk) = file_chunks.iter_mut().find(|c| c.chunk_id == chunk_id) {
                chunk.summary = summary;
                chunk.key_terms = key_terms;
                Ok(())
            } else {
                Err(ReadError::InvalidArgument(
                    format!("Chunk {} not found for file {:?}", chunk_id, file_path)
                ))
            }
        } else {
            Err(ReadError::InvalidArgument(
                format!("File {:?} not found in chunk pool", file_path)
            ))
        }
    }
    
    /// Parse worker analysis output to extract summary and key terms
    /// 
    /// Workers should respond with JSON in the format:
    /// {"summary": "...", "key_terms": ["term1", "term2"]}
    fn parse_worker_analysis(output: &str) -> Result<(String, Vec<String>), String> {
        // Try to find JSON in the output
        let json_start = output.find('{');
        let json_end = output.rfind('}');
        
        if let (Some(start), Some(end)) = (json_start, json_end) {
            if end > start {
                let json_str = &output[start..=end];
                match serde_json::from_str::<serde_json::Value>(json_str) {
                    Ok(json) => {
                        let summary = json.get("summary")
                            .and_then(|s| s.as_str())
                            .unwrap_or("Analysis completed")
                            .to_string();
                        let key_terms = json.get("key_terms")
                            .and_then(|k| k.as_array())
                            .map(|arr| arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect())
                            .unwrap_or_default();
                        return Ok((summary, key_terms));
                    }
                    Err(e) => {
                        crate::warn_log!("[ChunkPool] Failed to parse worker JSON: {}", e);
                    }
                }
            }
        }
        
        // Fallback: use first line as summary
        let summary = output.lines().next()
            .unwrap_or("Analysis completed")
            .to_string();
        Ok((summary, vec![]))
    }
    
    /// Static version of update_chunk_summary for use in async contexts
    async fn update_chunk_summary_static(
        chunks_arc: &Arc<RwLock<HashMap<PathBuf, Vec<ActiveChunk>>>>,
        file_path: &PathBuf,
        chunk_id: usize,
        summary: String,
        key_terms: Vec<String>,
    ) -> Result<(), ReadError> {
        let mut chunks = chunks_arc.write().await;
        
        if let Some(file_chunks) = chunks.get_mut(file_path) {
            if let Some(chunk) = file_chunks.iter_mut().find(|c| c.chunk_id == chunk_id) {
                chunk.summary = summary;
                chunk.key_terms = key_terms;
                crate::info_log!("[ChunkPool] Updated chunk {} summary: {}", chunk_id, chunk.summary.chars().take(50).collect::<String>());
                Ok(())
            } else {
                Err(ReadError::InvalidArgument(
                    format!("Chunk {} not found for file {:?}", chunk_id, file_path)
                ))
            }
        } else {
            Err(ReadError::InvalidArgument(
                format!("File {:?} not found in chunk pool", file_path)
            ))
        }
    }
    
    /// Find chunks that might contain information about a query
    /// 
    /// Uses key terms matching to find relevant chunks
    pub async fn find_relevant_chunks(
        &self,
        path: &PathBuf,
        query: &str,
    ) -> Vec<usize> {
        let chunks = self.chunks.read().await;
        let file_chunks = match chunks.get(path) {
            Some(c) => c,
            None => return vec![],
        };
        
        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
        
        file_chunks
            .iter()
            .filter_map(|chunk| {
                // Check if any key term matches
                let matches = chunk.key_terms.iter().any(|term| {
                    let term_lower = term.to_lowercase();
                    query_terms.iter().any(|q| term_lower.contains(q) || q.contains(&term_lower))
                });
                
                // Also check summary
                let summary_matches = query_terms.iter().any(|q| {
                    chunk.summary.to_lowercase().contains(q)
                });
                
                if matches || summary_matches {
                    Some(chunk.chunk_id)
                } else {
                    None
                }
            })
            .collect()
    }
    
    /// Query a specific chunk via commonbox routing
    /// 
    /// Routes the query to the chunk worker through commonbox.
    /// The worker must be in Idle state to receive queries.
    pub async fn query_chunk(
        &self,
        path: &PathBuf,
        chunk_id: usize,
        question: String,
    ) -> Option<ChunkQueryResponse> {
        // Get the job ID for this chunk worker
        // Chunk workers are registered with IDs like "chunk_filename_0"
        let worker_id = format!("chunk_{}_{}", 
            path.file_name()?.to_string_lossy(), 
            chunk_id
        );
        
        // Find the job by looking up the worker's agent ID
        let agent_id = crate::agent::identity::AgentId::worker(worker_id.clone());
        let job = self.commonbox.get_agent_job(&agent_id).await?;
        
        // Check if worker is available (Idle or Running)
        if !matches!(job.status, 
            crate::agent::runtime::orchestrator::commonbox::JobStatus::Idle |
            crate::agent::runtime::orchestrator::commonbox::JobStatus::Running
        ) {
            crate::warn_log!("[ChunkPool] Worker {} is not available (status: {:?})", worker_id, job.status);
            return None;
        }
        
        // Route query to worker via commonbox
        let query_id = match self.commonbox.route_query(
            &job.id,
            question,
            Some(format!("Chunk {} of file {:?}", chunk_id, path))
        ).await {
            Ok(id) => id,
            Err(e) => {
                crate::error_log!("[ChunkPool] Failed to route query to worker {}: {:?}", worker_id, e);
                return None;
            }
        };
        
        // Subscribe to commonbox events to wait for response
        let mut subscriber = self.commonbox.subscribe();
        let timeout = std::time::Duration::from_secs(30);
        let deadline = std::time::Instant::now() + timeout;
        
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(
                std::time::Duration::from_millis(100),
                subscriber.recv()
            ).await {
                Ok(Ok(crate::agent::runtime::orchestrator::commonbox::CommonboxEvent::QueryResult { 
                    job_id, 
                    query_id: result_query_id, 
                    result 
                })) => {
                    if job_id == job.id && result_query_id == query_id {
                        // Extract response from result
                        let response_text = result.0
                            .get("response")
                            .and_then(|v| v.as_str())
                            .unwrap_or("No response");
                        
                        let is_relevant = !response_text.contains("NOT_RELEVANT");
                        let confidence = if is_relevant { 0.8 } else { 0.0 };
                        
                        return Some(ChunkQueryResponse {
                            is_relevant,
                            answer: response_text.to_string(),
                            confidence,
                        });
                    }
                }
                Ok(Ok(_)) => {
                    // Other event, continue waiting
                    continue;
                }
                Ok(Err(_)) => {
                    // Channel closed
                    break;
                }
                Err(_) => {
                    // Timeout on this iteration, continue if not past deadline
                    continue;
                }
            }
        }
        
        crate::warn_log!("[ChunkPool] Timeout waiting for response from worker {}", worker_id);
        None
    }
    
    /// List all files with active chunks
    pub async fn list_active_files(&self) -> Vec<PathBuf> {
        let chunks = self.chunks.read().await;
        chunks.keys().cloned().collect()
    }
    
    /// Get chunk IDs for a file
    pub async fn list_chunks_for_file(&self, path: &PathBuf) -> Vec<usize> {
        let chunks = self.chunks.read().await;
        chunks
            .get(path)
            .map(|c| c.iter().map(|chunk| chunk.chunk_id).collect())
            .unwrap_or_default()
    }
    
    /// Remove all chunks for a file
    pub async fn remove_file(&self, path: &PathBuf) {
        let mut chunks = self.chunks.write().await;
        let mut count = self.worker_count.write().await;
        
        if let Some(file_chunks) = chunks.remove(path) {
            *count = count.saturating_sub(file_chunks.len());
            
            // Workers will be dropped and should terminate
            // In a full implementation, we'd send a shutdown signal
        }
    }
    
    /// Clear all chunks (called on session end)
    pub async fn clear(&self) {
        let mut chunks = self.chunks.write().await;
        let mut count = self.worker_count.write().await;
        
        chunks.clear();
        *count = 0;
    }
    
    /// Spawn chunk workers for a file using DelegateTool
    /// 
    /// Each chunk gets its own worker that analyzes the content and provides
    /// a summary and key terms. Workers remain idle after initial analysis
    /// to answer follow-up queries.
    pub async fn spawn_chunks(
        &self,
        file_path: &PathBuf,
        chunks: Vec<FileChunk>,
        content: &str,
    ) -> Result<Vec<ChunkSummary>, ReadError> {
        crate::info_log!("[ChunkPool] spawn_chunks called for {} with {} chunks, content_len={}",
            file_path.display(), chunks.len(), content.len());
        
        // Check if we have factory configured
        let factory = match self.factory.read().unwrap().clone() {
            Some(f) => f,
            None => {
                crate::error_log!("[ChunkPool] CRITICAL: Factory not configured! Cannot spawn workers.");
                return Err(ReadError::ReadError(
                    "ChunkPool factory not configured. Workers cannot be spawned.".to_string()
                ));
            }
        };
        crate::info_log!("[ChunkPool] Factory is configured, proceeding...");
        
        // Create DelegateTool for spawning workers
        let delegate_tool = crate::agent::tools::DelegateTool::new(
            self.commonbox.clone(),
            factory,
        );
        
        // Check if file is PDF - PDFs need special handling since they don't support line-based reading
        let is_pdf = file_path.extension()
            .map(|e| e.to_ascii_lowercase() == "pdf")
            .unwrap_or(false);
        
        // For PDFs, use the pre-extracted content passed from read_chunked
        // This avoids double-extraction since read_chunked already extracted and cached the PDF
        let pdf_text = if is_pdf && !content.is_empty() {
            crate::info_log!("[ChunkPool] Using pre-extracted PDF content ({} chars)", content.len());
            Some(content.to_string())
        } else if is_pdf {
            crate::info_log!("[ChunkPool] Pre-extracting PDF text for chunk workers: {}", file_path.display());
            match super::pdf::extract_text(file_path).await {
                Ok(text) => {
                    crate::info_log!("[ChunkPool] PDF extracted: {} characters", text.len());
                    Some(text)
                }
                Err(e) => {
                    crate::error_log!("[ChunkPool] Failed to extract PDF: {}", e);
                    None
                }
            }
        } else {
            None
        };
        
        // Build worker configurations for each chunk
        let mut worker_configs = Vec::new();
        for chunk in &chunks {
            let objective = if let Some(ref text) = pdf_text {
                // For PDFs: pass the actual chunk content in the objective
                let lines: Vec<&str> = text.lines().collect();
                let start_idx = (chunk.line_start - 1).min(lines.len());
                let end_idx = chunk.line_end.min(lines.len());
                let chunk_content = lines[start_idx..end_idx].join("\n");
                
                format!(
                    r#"You are analyzing a chunk of a PDF document.

File: {}
Chunk: {} (lines {}-{})

CHUNK CONTENT:
```
{}
```

Your task:
1. Analyze the content above and provide a concise summary (2-3 sentences)
2. Extract 5-10 key terms or identifiers from this chunk

Respond with a JSON object in this exact format:
{{
  "summary": "brief summary of the chunk content",
  "key_terms": ["term1", "term2", "term3", "term4", "term5"]
}}

After providing your analysis, you will enter an idle state waiting for follow-up questions about this chunk."#,
                    file_path.display(),
                    chunk.id,
                    chunk.line_start,
                    chunk.line_end,
                    chunk_content
                )
            } else {
                // For regular text files: instruct to read specific lines
                format!(
                    r#"You are analyzing a chunk of a large file.

File: {}
Chunk: {} (lines {}-{})

Your task:
1. Read lines {}-{} from the file using read_file tool with line_offset and n_lines parameters
2. Analyze the content and provide a concise summary (2-3 sentences)
3. Extract 5-10 key terms or identifiers from this chunk

Respond with a JSON object in this exact format:
{{
  "summary": "brief summary of the chunk content",
  "key_terms": ["term1", "term2", "term3", "term4", "term5"]
}}

After providing your analysis, you will enter an idle state waiting for follow-up questions about this chunk."#,
                    file_path.display(),
                    chunk.id,
                    chunk.line_start,
                    chunk.line_end,
                    chunk.line_start,
                    chunk.line_end
                )
            };
            
            let config = crate::agent::tools::delegate::WorkerConfig {
                id: format!("chunk_{}_{}", file_path.file_name().unwrap_or_default().to_string_lossy(), chunk.id),
                objective,
                instructions: Some("You are a document chunk analyzer. Be concise and accurate. Always respond with valid JSON when providing your analysis.".to_string()),
                // PDF workers don't need read_file since content is in objective
                tools: if is_pdf { None } else { Some(vec!["read_file".to_string()]) },
                allowed_commands: None,
                forbidden_commands: Some(vec!["*".to_string()]), // No shell commands for chunk workers
                tags: vec!["chunk_worker".to_string(), format!("chunk_{}", chunk.id)],
                depends_on: vec![],
                context: None,
                max_iterations: Some(3), // Reduced iterations since content is provided
                timeout_secs: Some(60),   // 1 minute timeout per chunk
            };
            
            worker_configs.push(config);
        }
        
        // Build delegate arguments
        let delegate_args = crate::agent::tools::delegate::DelegateArgs {
            shared_context: Some(format!(
                "Analyzing file: {}. Total chunks: {}. Each worker analyzes one chunk.",
                file_path.display(),
                chunks.len()
            )),
            workers: worker_configs,
        };
        
        // Create tool call for delegate
        let tool_call = crate::agent::types::intents::ToolCall::new(
            "delegate",
            serde_json::to_value(delegate_args).map_err(|e| ReadError::ReadError(format!("Failed to serialize delegate args: {}", e)))?
        );
        
        // Spawn workers via DelegateTool
        let runtime_context = crate::agent::runtime::core::RuntimeContext::new();
        
        crate::info_log!("[ChunkPool] About to call delegate_tool.execute with {} workers...", chunks.len());
        
        match delegate_tool.execute(&runtime_context, tool_call).await {
            Ok(tool_result) => {
                match tool_result {
                    crate::agent::types::events::ToolResult::Success { structured, .. } => {
                        // Extract job IDs from result and wait for workers to complete initial analysis
                        if let Some(result_value) = structured {
                            crate::info_log!("[ChunkPool] Workers spawned successfully: {:?}", result_value);
                            
                            // Workers are now spawned and will analyze their chunks.
                            // Placeholder summaries are returned immediately.
                            // In Phase 3, we'll collect actual summaries from worker results
                            // via commonboard or by querying workers directly.
                            let summaries: Vec<ChunkSummary> = chunks
                                .iter()
                                .map(|chunk| ChunkSummary {
                                    chunk_id: chunk.id,
                                    line_range: (chunk.line_start, chunk.line_end),
                                    summary: format!("Lines {}-{} (analyzing...)", chunk.line_start, chunk.line_end),
                                    key_terms: vec![],
                                    content_hash: {
                                        let mut hasher = DefaultHasher::new();
                                        format!("{}:{}-{}", file_path.display(), chunk.line_start, chunk.line_end).hash(&mut hasher);
                                        format!("{:016x}", hasher.finish())
                                    },
                                })
                                .collect();
                            
                            // Register chunks as "active" - workers are now analyzing
                            for (i, chunk) in chunks.iter().enumerate() {
                                let (query_tx, _query_rx) = mpsc::channel(10);
                                let active_chunk = ActiveChunk {
                                    chunk_id: chunk.id,
                                    worker_id: WorkerId((1000 + chunk.id) as u64),
                                    line_range: (chunk.line_start, chunk.line_end),
                                    summary: summaries[i].summary.clone(),
                                    key_terms: summaries[i].key_terms.clone(),
                                    content_hash: summaries[i].content_hash.clone(),
                                    query_tx,
                                };
                                
                                let mut pool_chunks = self.chunks.write().await;
                                let mut count = self.worker_count.write().await;
                                pool_chunks.entry(file_path.clone()).or_default().push(active_chunk);
                                *count += 1;
                            }
                            
                            // Spawn background task to collect worker analysis results
                            // This updates chunk summaries once workers complete initial analysis
                            let file_path_clone = file_path.clone();
                            let _chunks_clone = chunks.clone();
                            let _commonbox_clone = Arc::clone(&self.commonbox);
                            let chunks_arc = Arc::clone(&self.chunks);
                            
                            // Subscribe to commonbox events to track worker completion
                            let mut commonbox_subscriber = self.commonbox.subscribe();
                            
                            // Build map of job_id -> chunk_id for tracking
                            let mut job_to_chunk: HashMap<String, usize> = HashMap::new();
                            if let Some(workers) = result_value.get("workers").and_then(|w| w.as_array()) {
                                for (i, worker) in workers.iter().enumerate() {
                                    if let Some(job_id) = worker.get("job_id").and_then(|j| j.as_str()) {
                                        if i < chunks.len() {
                                            job_to_chunk.insert(job_id.to_string(), chunks[i].id);
                                        }
                                    }
                                }
                            }
                            crate::info_log!("[ChunkPool] Tracking {} workers by job ID", job_to_chunk.len());
                            
                            // Wait for workers to complete initial analysis (with timeout)
                            crate::info_log!("[ChunkPool] === WORKER TRACKING START ===");
                            crate::info_log!("[ChunkPool] Waiting for {} workers to complete...", chunks.len());
                            let start_time = std::time::Instant::now();
                            
                            let wait_result = tokio::time::timeout(
                                std::time::Duration::from_secs(60), // 1 minute timeout (faster for quick models)
                                async {
                                    let mut completed = 0;
                                    let mut last_reported = 0;
                                    let total = chunks.len();
                                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(55);
                                    
                                    while completed < total && std::time::Instant::now() < deadline {
                                        // Check for commonbox events (job completion)
                                        match tokio::time::timeout(
                                            std::time::Duration::from_millis(100),
                                            commonbox_subscriber.recv()
                                        ).await {
                                            Ok(Ok(event)) => {
                                                use crate::agent::runtime::orchestrator::commonbox::CommonboxEvent;
                                                match event {
                                                    CommonboxEvent::JobCompleted { job_id, result } => {
                                                        let job_id_str = job_id.0.to_string();
                                                        if let Some(&chunk_id) = job_to_chunk.get(&job_id_str) {
                                                            crate::info_log!("[ChunkPool] Worker job {} completed for chunk {}", job_id_str, chunk_id);
                                                            // Try to parse result for summary
                                                            let output = result.as_str().unwrap_or("");
                                                            if let Ok((summary, key_terms)) = Self::parse_worker_analysis(output) {
                                                                let _ = Self::update_chunk_summary_static(
                                                                    &chunks_arc, &file_path_clone, chunk_id, summary, key_terms
                                                                ).await;
                                                                completed += 1;
                                                            } else {
                                                                completed += 1; // Count as done even if parse fails
                                                            }
                                                        }
                                                    }
                                                    CommonboxEvent::JobFailed { job_id, error } => {
                                                        let job_id_str = job_id.0.to_string();
                                                        if let Some(&chunk_id) = job_to_chunk.get(&job_id_str) {
                                                            crate::warn_log!("[ChunkPool] Worker job {} failed for chunk {}: {}", job_id_str, chunk_id, error);
                                                            completed += 1; // Count as completed even if failed
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            }
                                            _ => {} // Timeout or error, continue polling
                                        }
                                        
                                        // Also poll chunk summaries for external updates
                                        let chunks_guard = chunks_arc.read().await;
                                        if let Some(file_chunks) = chunks_guard.get(&file_path_clone) {
                                            let analyzed_count = file_chunks.iter()
                                                .filter(|c| !c.summary.contains("analyzing..."))
                                                .count();
                                            if analyzed_count > completed {
                                                completed = analyzed_count;
                                            }
                                        }
                                        drop(chunks_guard);
                                        
                                        // Progress report every 5 workers or every 10 seconds
                                        if completed > last_reported && (completed % 5 == 0 || completed == total) {
                                            let elapsed = start_time.elapsed().as_secs_f32();
                                            crate::info_log!(
                                                "[ChunkPool] Progress: {}/{} chunks analyzed ({:.0}%) - {:.1}s elapsed",
                                                completed, total, 
                                                (completed as f32 / total as f32) * 100.0,
                                                elapsed
                                            );
                                            last_reported = completed;
                                        }
                                        
                                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                    }
                                    
                                    completed
                                }
                            ).await;
                            
                            let total_elapsed = start_time.elapsed().as_secs_f32();
                            match wait_result {
                                Ok(completed) => {
                                    crate::info_log!(
                                        "[ChunkPool] === WORKER TRACKING COMPLETE ==="
                                    );
                                    crate::info_log!(
                                        "[ChunkPool] Workers completed: {}/{} in {:.1}s",
                                        completed, chunks.len(), total_elapsed
                                    );
                                    if completed < chunks.len() {
                                        crate::warn_log!(
                                            "[ChunkPool] {} workers did not complete in time",
                                            chunks.len() - completed
                                        );
                                    }
                                }
                                Err(_) => {
                                    crate::warn_log!(
                                        "[ChunkPool] TIMEOUT after {:.1}s - {}/{} workers completed",
                                        total_elapsed, 
                                        wait_result.unwrap_or(0), 
                                        chunks.len()
                                    );
                                }
                            }
                            
                            // Return updated summaries from active chunks
                            let final_chunks = self.chunks.read().await;
                            let final_summaries: Vec<ChunkSummary> = if let Some(file_chunks) = final_chunks.get(file_path) {
                                file_chunks.iter().map(|c| ChunkSummary {
                                    chunk_id: c.chunk_id,
                                    line_range: c.line_range,
                                    summary: c.summary.clone(),
                                    key_terms: c.key_terms.clone(),
                                    content_hash: c.content_hash.clone(),
                                }).collect()
                            } else {
                                summaries // Fallback to original placeholders
                            };
                            
                            Ok(final_summaries)
                        } else {
                            Err(ReadError::ChunkWorkerFailed {
                                chunk_id: 0,
                                error: "No structured result from delegate tool".to_string(),
                            })
                        }
                    }
                    crate::agent::types::events::ToolResult::Error { message, code, .. } => {
                        crate::error_log!("[ChunkPool] Delegate returned error: {} (code: {:?})", message, code);
                        Err(ReadError::ChunkWorkerFailed {
                            chunk_id: 0,
                            error: format!("Delegate error: {} (code: {:?})", message, code),
                        })
                    }
                    crate::agent::types::events::ToolResult::Cancelled => {
                        crate::error_log!("[ChunkPool] Delegate execution was cancelled");
                        Err(ReadError::ChunkWorkerFailed {
                            chunk_id: 0,
                            error: "Delegate tool execution was cancelled".to_string(),
                        })
                    }
                }
            }
            Err(e) => {
                crate::error_log!("[ChunkPool] Failed to execute delegate tool: {}", e);
                Err(ReadError::ChunkWorkerFailed {
                    chunk_id: 0,
                    error: format!("Failed to execute delegate tool: {}", e),
                })
            }
        }
    }
    
    /// Retry a failed chunk worker
    /// 
    /// Returns true if retry was successful
    pub async fn retry_chunk(
        &self,
        _file_path: &PathBuf,
        _chunk: &FileChunk,
        _attempt: u32,
    ) -> Result<ChunkSummary, ReadError> {
        if _attempt >= MAX_CHUNK_RETRIES {
            return Err(ReadError::ChunkWorkerFailed {
                chunk_id: _chunk.id,
                error: format!("Failed after {} retries", MAX_CHUNK_RETRIES),
            });
        }
        
        // TODO: Implement actual retry logic with DelegateTool
        // For now, simulate success after a delay
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        
        Ok(ChunkSummary {
            chunk_id: _chunk.id,
            line_range: (_chunk.line_start, _chunk.line_end),
            summary: format!("Lines {}-{} (retry {})", _chunk.line_start, _chunk.line_end, _attempt),
            key_terms: vec![],
            content_hash: String::new(),
        })
    }
}

impl Clone for ChunkPool {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            chunks: Arc::clone(&self.chunks),
            max_workers: self.max_workers,
            worker_count: Arc::clone(&self.worker_count),
            commonbox: Arc::clone(&self.commonbox),
            factory: StdRwLock::new(self.factory.read().unwrap().clone()),
            output_tx: StdRwLock::new(self.output_tx.read().unwrap().clone()),
            worker_context_window: self.worker_context_window,
        }
    }
}

/// Background task to collect worker analysis results
/// 
/// Waits for workers to complete their initial analysis and updates
/// chunk summaries with actual content from workers.
async fn _collect_worker_summaries(
    file_path: PathBuf,
    chunks: Vec<FileChunk>,
    commonbox: Arc<Commonbox>,
    chunks_arc: Arc<RwLock<HashMap<PathBuf, Vec<ActiveChunk>>>>,
) {
    crate::info_log!("[ChunkPool] Starting background collection for {} chunks in {}", 
        chunks.len(), file_path.display());
    
    // Subscribe to commonbox events to detect when workers complete
    let mut subscriber = commonbox.subscribe();
    let timeout = std::time::Duration::from_secs(120); // 2 minute timeout
    let deadline = std::time::Instant::now() + timeout;
    
    let mut completed_chunks = std::collections::HashSet::new();
    
    while std::time::Instant::now() < deadline && completed_chunks.len() < chunks.len() {
        match tokio::time::timeout(
            std::time::Duration::from_millis(500),
            subscriber.recv()
        ).await {
            Ok(Ok(crate::agent::runtime::orchestrator::commonbox::CommonboxEvent::JobIdle { job_id })) |
            Ok(Ok(crate::agent::runtime::orchestrator::commonbox::CommonboxEvent::JobCompleted { job_id, .. })) => {
                // Check if this job corresponds to one of our chunk workers
                if let Some(job) = commonbox.get_job(&job_id).await {
                    let worker_id = job.agent_id.short_name();
                    
                    // Extract chunk ID from worker name (format: "chunk_filename_N")
                    if let Some(chunk_id_str) = worker_id.strip_prefix("chunk_") {
                        if let Some(chunk_id) = chunk_id_str.rsplit('_').next()
                            .and_then(|s| s.parse::<usize>().ok()) {
                            
                            if !completed_chunks.contains(&chunk_id) {
                                completed_chunks.insert(chunk_id);
                                
                                // Query the worker for its summary
                                // Workers should have stored their analysis in the commonboard
                                // For now, we query them directly
                                let summary_query = "Provide a brief summary of your chunk content (2-3 sentences) and list 5-10 key terms. Format as JSON: {\"summary\": \"...\", \"key_terms\": [\"...\"]}".to_string();
                                
                                // Route query to get summary from worker
                                if let Ok(_query_id) = commonbox.route_query(
                                    &job_id,
                                    summary_query,
                                    None
                                ).await {
                                    crate::info_log!("[ChunkPool] Requested summary from chunk {} worker", chunk_id);
                                }
                            }
                        }
                    }
                }
            }
            Ok(Ok(crate::agent::runtime::orchestrator::commonbox::CommonboxEvent::QueryResult { job_id, result, .. })) => {
                // Check if this is a summary response from one of our workers
                if let Some(job) = commonbox.get_job(&job_id).await {
                    let worker_id = job.agent_id.short_name();
                    
                    if let Some(chunk_id_str) = worker_id.strip_prefix("chunk_") {
                        if let Some(chunk_id) = chunk_id_str.rsplit('_').next()
                            .and_then(|s| s.parse::<usize>().ok()) {
                            
                            // Try to parse JSON response
                            let response_text = result.0.get("response")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(response_text) {
                                let summary = json.get("summary")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Analysis complete")
                                    .to_string();
                                
                                let key_terms: Vec<String> = json.get("key_terms")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| arr.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect())
                                    .unwrap_or_default();
                                
                                // Update chunk summary
                                let mut chunks_guard = chunks_arc.write().await;
                                if let Some(file_chunks) = chunks_guard.get_mut(&file_path) {
                                    if let Some(chunk) = file_chunks.iter_mut()
                                        .find(|c| c.chunk_id == chunk_id) {
                                        chunk.summary = summary.clone();
                                        chunk.key_terms = key_terms.clone();
                                        crate::info_log!("[ChunkPool] Updated summary for chunk {}: {}", 
                                            chunk_id, &summary[..summary.len().min(50)]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(Ok(_)) => {
                // Other event, continue
                continue;
            }
            Ok(Err(_)) => {
                // Channel closed
                break;
            }
            Err(_) => {
                // Timeout on this iteration
                continue;
            }
        }
    }
    
    crate::info_log!("[ChunkPool] Background collection complete. {}/{} chunks updated.",
        completed_chunks.len(), chunks.len());
}

/// Worker handle for managing a chunk worker's lifecycle
#[derive(Debug)]
#[cfg(test)]
pub struct ChunkWorkerHandle {
    /// Worker ID
    pub worker_id: WorkerId,
    /// Chunk being processed
    pub chunk: FileChunk,
    /// Shutdown signal
    shutdown_tx: mpsc::Sender<()>,
}


#[cfg(test)]
impl ChunkWorkerHandle {
    /// Create a new worker handle
    pub fn new(worker_id: WorkerId, chunk: FileChunk) -> (Self, mpsc::Receiver<()>) {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        
        let handle = Self {
            worker_id,
            chunk,
            shutdown_tx,
        };
        
        (handle, shutdown_rx)
    }
    
    /// Signal the worker to shut down
    pub async fn shutdown(&self) -> Result<(), mpsc::error::SendError<()>> {
        self.shutdown_tx.send(()).await
    }
}

/// Builder for creating chunk workers
#[cfg(test)]
pub struct ChunkWorkerBuilder {
    file_path: PathBuf,
    chunk: FileChunk,
    content: String,
}


#[cfg(test)]
impl ChunkWorkerBuilder {
    /// Create a new worker builder
    pub fn new(file_path: PathBuf, chunk: FileChunk, content: String) -> Self {
        Self {
            file_path,
            chunk,
            content,
        }
    }
    
    /// Build the worker objective prompt
    pub fn build_objective(&self) -> String {
        format!(
            r#"You are analyzing a chunk of a large file.

File: {}
Chunk: {} (lines {}-{})

Your content:
```
{}
```

Your tasks:
1. Analyze this chunk and provide a concise summary (2-3 sentences)
2. Extract 5-10 key terms or identifiers from this chunk
3. Be ready to answer specific questions about this content

Respond in this JSON format:
{{
  "summary": "brief summary of content",
  "key_terms": ["term1", "term2", "term3"]
}}"#,
            self.file_path.display(),
            self.chunk.id,
            self.chunk.line_start,
            self.chunk.line_end,
            self.content
        )
    }
    
    /// Get the chunk
    pub fn chunk(&self) -> &FileChunk {
        &self.chunk
    }
    
    /// Get the file path
    pub fn file_path(&self) -> &PathBuf {
        &self.file_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_chunk_pool_creation() {
        let pool = ChunkPool::new("test-session", 5, 8192);
        assert_eq!(pool.session_id(), "test-session");
        assert!(pool.can_spawn().await);
        assert_eq!(pool.worker_count().await, 0);
    }
    
    #[tokio::test]
    async fn test_chunk_registration() {
        let pool = ChunkPool::new("test-session", 2, 8192);
        let path = PathBuf::from("/test/file.txt");
        
        let (tx, _rx) = mpsc::channel(10);
        let chunk = ActiveChunk {
            chunk_id: 0,
            worker_id: WorkerId(1),
            line_range: (1, 100),
            summary: "Test summary".to_string(),
            key_terms: vec!["test".to_string()],
            content_hash: "abc123".to_string(),
            query_tx: tx,
        };
        
        pool.register_chunk(path.clone(), chunk).await.unwrap();
        assert_eq!(pool.worker_count().await, 1);
        
        // Test max workers limit
        let (tx2, _rx2) = mpsc::channel(10);
        let chunk2 = ActiveChunk {
            chunk_id: 1,
            worker_id: WorkerId(2),
            line_range: (101, 200),
            summary: "Test summary 2".to_string(),
            key_terms: vec!["test2".to_string()],
            content_hash: "def456".to_string(),
            query_tx: tx2,
        };
        
        pool.register_chunk(path.clone(), chunk2).await.unwrap();
        assert_eq!(pool.worker_count().await, 2);
        
        // Should fail - max workers reached
        let (tx3, _rx3) = mpsc::channel(10);
        let chunk3 = ActiveChunk {
            chunk_id: 2,
            worker_id: WorkerId(3),
            line_range: (201, 300),
            summary: "Test summary 3".to_string(),
            key_terms: vec!["test3".to_string()],
            content_hash: "ghi789".to_string(),
            query_tx: tx3,
        };
        
        assert!(pool.register_chunk(path.clone(), chunk3).await.is_err());
    }
    
    #[tokio::test]
    async fn test_find_relevant_chunks() {
        let pool = ChunkPool::new("test-session", 5, 8192);
        let path = PathBuf::from("/test/file.txt");
        
        let (tx, _rx) = mpsc::channel(10);
        let chunk = ActiveChunk {
            chunk_id: 0,
            worker_id: WorkerId(1),
            line_range: (1, 100),
            summary: "Functions for error handling".to_string(),
            key_terms: vec!["error".to_string(), "Result".to_string(), "panic".to_string()],
            content_hash: "abc123".to_string(),
            query_tx: tx,
        };
        
        pool.register_chunk(path.clone(), chunk).await.unwrap();
        
        // Find by key term
        let relevant = pool.find_relevant_chunks(&path, "error handling").await;
        assert!(relevant.contains(&0));
        
        // Find by summary content
        let relevant = pool.find_relevant_chunks(&path, "functions").await;
        assert!(relevant.contains(&0));
        
        // No match
        let relevant = pool.find_relevant_chunks(&path, "database").await;
        assert!(!relevant.contains(&0));
    }
    
    #[tokio::test]
    async fn test_worker_count_limits() {
        let pool = ChunkPool::new("test-session", 5, 8192);
        assert!(pool.can_spawn().await);
        
        let pool_full = ChunkPool::new("test-session", 0, 8192);
        // Should be clamped to minimum of 1
        assert!(pool_full.can_spawn().await);
        
        let pool_large = ChunkPool::new("test-session", 100, 8192);
        // Should be clamped to maximum of 50
        // We can't directly test this, but we can verify it accepts workers
        assert!(pool_large.can_spawn().await);
    }
    
    #[test]
    fn test_chunk_worker_builder() {
        let chunk = FileChunk::new(0, 1, 100, 4000);
        let builder = ChunkWorkerBuilder::new(
            PathBuf::from("/test.rs"),
            chunk,
            "fn main() {}".to_string(),
        );
        
        let objective = builder.build_objective();
        assert!(objective.contains("/test.rs"));
        assert!(objective.contains("lines 1-100"));
        assert!(objective.contains("fn main()"));
        assert!(objective.contains("JSON format"));
    }
    
    #[tokio::test]
    async fn test_chunk_pool_clear() {
        let pool = ChunkPool::new("test-session", 5, 8192);
        let path = PathBuf::from("/test/file.txt");
        
        let (tx, _rx) = mpsc::channel(10);
        let chunk = ActiveChunk {
            chunk_id: 0,
            worker_id: WorkerId(1),
            line_range: (1, 100),
            summary: "Test".to_string(),
            key_terms: vec![],
            content_hash: "abc".to_string(),
            query_tx: tx,
        };
        
        pool.register_chunk(path.clone(), chunk).await.unwrap();
        assert_eq!(pool.worker_count().await, 1);
        
        pool.clear().await;
        assert_eq!(pool.worker_count().await, 0);
        assert!(pool.list_active_files().await.is_empty());
    }
}
