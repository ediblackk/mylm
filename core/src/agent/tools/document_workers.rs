//! Document Worker Tools
//!
//! Three-tool architecture for document processing:
//! 1. `query_file` - Spawns sandboxed LLM workers for each chunk
//! 2. `query_chunk` - Sends queries to specific chunk workers
//! 3. `close_file` - Cleans up chunk workers for a file

use crate::agent::runtime::core::{Capability, RuntimeContext, ToolCapability, ToolError};
use crate::agent::types::events::ToolResult;
use crate::agent::types::intents::ToolCall;
use crate::agent::tools::expand_tilde;
use crate::agent::tools::read_file::{TokenChunkConfig, compute_chunks_with_tokens};
use crate::provider::LlmClient;
use crate::provider::chat::{ChatMessage, ChatRequest};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::task::JoinHandle;

/// Message sent to a chunk worker
#[derive(Debug)]
pub struct ChunkWorkerMessage {
    /// The query/prompt from the main agent
    pub query: String,
    /// Channel to send the response back
    pub response_tx: oneshot::Sender<ChunkWorkerResponse>,
}

/// Response from a chunk worker
#[derive(Debug, Clone)]
pub struct ChunkWorkerResponse {
    /// The worker's response
    pub answer: String,
    /// Whether the query was relevant to the chunk
    pub is_relevant: bool,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
}

/// Registry for chunk worker channels
/// Stored in Commonbox or as a global resource
#[derive(Debug, Clone)]
pub struct ChunkWorkerRegistry {
    /// Map of chunk_id -> sender channel
    /// chunk_id format: "{filename}_chunk_{index}"
    workers: Arc<RwLock<HashMap<String, mpsc::Sender<ChunkWorkerMessage>>>>,
    /// Track which chunks belong to which file for cleanup
    file_chunks: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl ChunkWorkerRegistry {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
            file_chunks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a chunk worker channel
    pub async fn register(&self, chunk_id: String, sender: mpsc::Sender<ChunkWorkerMessage>) {
        let mut workers = self.workers.write().await;
        workers.insert(chunk_id, sender);
    }

    /// Get a worker's sender channel
    pub async fn get(&self, chunk_id: &str) -> Option<mpsc::Sender<ChunkWorkerMessage>> {
        let workers = self.workers.read().await;
        workers.get(chunk_id).cloned()
    }

    /// Register file -> chunks mapping for cleanup
    pub async fn register_file_chunks(&self, file_name: String, chunk_ids: Vec<String>) {
        let mut file_chunks = self.file_chunks.write().await;
        file_chunks.insert(file_name, chunk_ids);
    }

    /// Get all chunk IDs for a file
    pub async fn get_file_chunks(&self, file_name: &str) -> Option<Vec<String>> {
        let file_chunks = self.file_chunks.read().await;
        file_chunks.get(file_name).cloned()
    }

    /// Remove a chunk worker (returns true if existed)
    pub async fn remove(&self, chunk_id: &str) -> bool {
        let mut workers = self.workers.write().await;
        workers.remove(chunk_id).is_some()
    }

    /// Remove all chunks for a file and return their IDs
    pub async fn remove_file(&self, file_name: &str) -> Vec<String> {
        let chunk_ids = {
            let mut file_chunks = self.file_chunks.write().await;
            file_chunks.remove(file_name).unwrap_or_default()
        };
        
        let mut workers = self.workers.write().await;
        for chunk_id in &chunk_ids {
            workers.remove(chunk_id);
        }
        
        chunk_ids
    }

    /// List all registered chunk IDs
    pub async fn list_chunks(&self) -> Vec<String> {
        let workers = self.workers.read().await;
        workers.keys().cloned().collect()
    }
}

impl Default for ChunkWorkerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tool 1: query_file - Spawn sandboxed LLM workers for document chunks
// ============================================================================

/// Arguments for query_file tool
#[derive(Debug, Clone, serde::Deserialize)]
pub struct QueryFileArgs {
    #[serde(alias = "path")]
    pub file_path: String,
    /// Initial prompt to broadcast to all chunks (for map-reduce summary)
    pub prompt: String,
}

/// Tool that spawns sandboxed LLM workers for each document chunk
pub struct QueryFileTool {
    /// Registry for chunk worker channels
    registry: Arc<ChunkWorkerRegistry>,
    /// LLM client for spawning workers
    llm_client: Arc<LlmClient>,
    /// Worker context window size
    worker_context_window: usize,
    /// Maximum chunk utilization (e.g., 0.5 for 50%)
    max_utilization: f32,
    /// Output sender to emit progress events to UI
    output_tx: Option<crate::agent::runtime::orchestrator::OutputSender>,
}

impl QueryFileTool {
    pub fn new(
        registry: Arc<ChunkWorkerRegistry>,
        llm_client: Arc<LlmClient>,
        worker_context_window: usize,
        output_tx: crate::agent::runtime::orchestrator::OutputSender,
    ) -> Self {
        Self {
            registry,
            llm_client,
            worker_context_window,
            max_utilization: 0.5,
            output_tx: Some(output_tx),
        }
    }

    /// Spawn a sandboxed worker task for a single chunk
    /// 
    /// The worker runs a loop listening for queries on the mpsc channel.
    /// It has NO access to tools, memory, or registry - only raw LLM client.
    fn spawn_chunk_worker(
        &self,
        chunk_id: String,
        chunk_content: String,
        line_start: usize,
        line_end: usize,
        mut rx: mpsc::Receiver<ChunkWorkerMessage>,
    ) -> JoinHandle<()> {
        let llm_client = Arc::clone(&self.llm_client);
        
        tokio::spawn(async move {
            // Worker maintains local conversation history
            let mut conversation: Vec<ChatMessage> = vec![
                ChatMessage::system(format!(
                    "You are a background document analysis worker. Your ONLY job is to extract, analyze, and format information for the Main Orchestrator Agent. \
                    You have access to a specific chunk of a document (lines {}-{}). \
                    Your chunk content is provided in the first user message. \
                    Answer questions based ONLY on your chunk content. \
                    NEVER address the user directly (do not say 'Here is the summary', 'You can find', etc.). \
                    Output raw facts, summaries, or extracted quotes that the Main Agent can use. \
                    If a question is not relevant to your chunk, respond with EXACTLY 'NOT_RELEVANT'.",
                    line_start, line_end
                )),
                ChatMessage::user(format!(
                    "CHUNK CONTENT (lines {}-{}):\n```\n{}\n```",
                    line_start, line_end, chunk_content
                )),
            ];

            crate::info_log!("[Worker:{}] Spawned and ready", chunk_id);

            // Worker loop - completely sandboxed
            while let Some(msg) = rx.recv().await {
                crate::info_log!("[Worker:{}] Received query: '{}'", chunk_id, 
                    msg.query.chars().take(50).collect::<String>());

                // Add the query to conversation history
                conversation.push(ChatMessage::user(msg.query.clone()));

                // Create LLM request
                let request = ChatRequest::new(
                    llm_client.model().to_string(),
                    conversation.clone()
                )
                .with_temperature(0.3)
                .with_max_tokens(1000);

                // Query LLM (no tools, no memory, no registry access)
                let response_text = match llm_client.chat(&request).await {
                    Ok(response) => {
                        let text = response.content();
                        crate::info_log!("[Worker:{}] LLM responded: {} chars", chunk_id, text.len());
                        text
                    }
                    Err(e) => {
                        crate::error_log!("[Worker:{}] LLM error: {}", chunk_id, e);
                        format!("Error: {}", e)
                    }
                };

                // Add response to conversation history
                conversation.push(ChatMessage::assistant(response_text.clone()));

                // Determine relevance
                let is_relevant = !response_text.contains("NOT_RELEVANT");
                let confidence = if is_relevant { 0.8 } else { 0.0 };

                // Send response back
                let response = ChunkWorkerResponse {
                    answer: response_text,
                    is_relevant,
                    confidence,
                };

                if msg.response_tx.send(response).is_err() {
                    crate::warn_log!("[Worker:{}] Failed to send response - caller dropped", chunk_id);
                }
            }

            crate::info_log!("[Worker:{}] Shutting down (channel closed)", chunk_id);
        })
    }
}

#[async_trait::async_trait]
impl ToolCapability for QueryFileTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        let args: QueryFileArgs = match serde_json::from_value(call.arguments) {
            Ok(a) => a,
            Err(e) => {
                return Ok(ToolResult::Error {
                    message: format!("Invalid arguments: {}. Expected: {{\"file_path\": \"doc.txt\", \"prompt\": \"Summarize\"}}", e),
                    code: Some("PARSE_ERROR".to_string()),
                    retryable: false,
                });
            }
        };

        let path = expand_tilde(&args.file_path);
        let path_buf = PathBuf::from(&path);
        let file_name = path_buf.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let ext = path_buf.extension().and_then(|e| e.to_str()).unwrap_or("");

        // Emit extraction start status
        if let Some(ref tx) = self.output_tx {
            let _ = tx.send(crate::agent::runtime::orchestrator::OutputEvent::Status {
                message: format!("Processing Document: {}", file_name),
            });
            
            if ext.to_lowercase() == "pdf" {
                let _ = tx.send(crate::agent::runtime::orchestrator::OutputEvent::Status {
                    message: "Extracting PDF layout and identifying pages (this may take a moment)...".to_string(),
                });
            }
        }

        crate::info_log!("[QueryFile] Processing: '{}' with prompt: '{}'", 
            file_name, args.prompt.chars().take(50).collect::<String>());

        // Read full file content based on format
        let extension = path_buf.extension().unwrap_or_default().to_string_lossy().to_lowercase();
        
        let mut file_content = String::new();
        let mut custom_chunk_result = None;
        let mut pdf_page_mapping = Vec::new();
        
        // Determine chunk configuration (50% of context window)
        let chunk_config = TokenChunkConfig {
            worker_context_window: self.worker_context_window,
            utilization_ratio: self.max_utilization,
            overlap_tokens: 1000, // Default 1K overlap
            min_chunk_tokens: 500,
        };
        
        match extension.as_str() {
            "pdf" => {
                crate::info_log!("[QueryFile] Extracting PDF text per page from '{}'", file_name);
                match crate::agent::tools::read_file::extract_pdf_pages(&path_buf).await {
                    Ok(pages) => {
                        let mut chunks = Vec::new();
                        let target_tokens = chunk_config.effective_chunk_size();
                        let mut current_chunk_id = 0;
                        
                        let mut current_pages_content = String::new();
                        let mut current_start_page = 1;
                        let mut current_start_line = 1;
                        let mut current_tokens = 0;
                        let mut current_bytes = 0;
                        
                        for (i, page_content) in pages.iter().enumerate() {
                            let page_num = i + 1;
                            let formatted_page = format!("--- Page {} ---\n{}\n\n", page_num, page_content);
                            let page_tokens = formatted_page.len() / 4;
                            
                            // If adding this page exceeds target tokens (and chunk isn't empty)
                            if !current_pages_content.is_empty() && (current_tokens + page_tokens > target_tokens) {
                                // Save current chunk
                                let chunk_lines = current_pages_content.lines().count();
                                let end_line = current_start_line + chunk_lines - 1;
                                chunks.push(crate::agent::tools::read_file::FileChunk::new(
                                    current_chunk_id,
                                    current_start_line,
                                    end_line,
                                    current_bytes,
                                ));
                                let mapping_text = if current_start_page == page_num - 1 {
                                    format!("Chunk {}: Page {}", current_chunk_id, current_start_page)
                                } else {
                                    format!("Chunk {}: Pages {}-{}", current_chunk_id, current_start_page, page_num - 1)
                                };
                                pdf_page_mapping.push(mapping_text);
                                
                                file_content.push_str(&current_pages_content);
                                
                                current_chunk_id += 1;
                                current_pages_content = String::new();
                                current_start_page = page_num;
                                current_start_line = end_line + 1;
                                current_tokens = 0;
                                current_bytes = 0;
                            }
                            
                            current_pages_content.push_str(&formatted_page);
                            current_tokens += page_tokens;
                            current_bytes += formatted_page.len();
                        }
                        
                        // Add last chunk
                        if !current_pages_content.is_empty() {
                            let chunk_lines = current_pages_content.lines().count();
                            let end_line = current_start_line + chunk_lines - 1;
                            chunks.push(crate::agent::tools::read_file::FileChunk::new(
                                current_chunk_id,
                                current_start_line,
                                end_line,
                                current_bytes,
                            ));
                            let last_page = pages.len();
                            let mapping_text = if current_start_page == last_page {
                                format!("Chunk {}: Page {}", current_chunk_id, current_start_page)
                            } else {
                                format!("Chunk {}: Pages {}-{}", current_chunk_id, current_start_page, last_page)
                            };
                            pdf_page_mapping.push(mapping_text);
                            file_content.push_str(&current_pages_content);
                        }
                        
                        custom_chunk_result = Some(crate::agent::tools::read_file::TokenChunkResult {
                            chunks,
                            _total_tokens: file_content.len() / 4,
                            _tokens_per_chunk: target_tokens,
                            _overlap_tokens: 0,
                        });
                    },
                    Err(e) => {
                        return Ok(ToolResult::Error {
                            message: format!("Failed to extract PDF text: {}", e),
                            code: Some("PDF_EXTRACT_ERROR".to_string()),
                            retryable: false,
                        });
                    }
                }
            },
            "docx" => {
                crate::info_log!("[QueryFile] Extracting DOCX text from '{}'", file_name);
                match crate::agent::tools::read_file::extract_docx_text(&path_buf).await {
                    Ok(text) => file_content = text,
                    Err(e) => {
                        return Ok(ToolResult::Error {
                            message: format!("Failed to extract DOCX text: {}", e),
                            code: Some("DOCX_EXTRACT_ERROR".to_string()),
                            retryable: false,
                        });
                    }
                }
            },
            _ => {
                match tokio::fs::read_to_string(&path_buf).await {
                    Ok(content) => file_content = content,
                    Err(e) => {
                        return Ok(ToolResult::Error {
                            message: format!("Failed to read file content: {}", e),
                            code: Some("READ_ERROR".to_string()),
                            retryable: false,
                        });
                    }
                }
            }
        };

        let file_size = file_content.len();
        let total_lines = file_content.lines().count();

        // Compute chunks if not already computed (i.e., not a PDF)
        let chunk_result = custom_chunk_result.unwrap_or_else(|| {
            compute_chunks_with_tokens(file_size, total_lines, &chunk_config)
        });
        let total_chunks = chunk_result.chunks.len();

        crate::info_log!("[QueryFile] File: {} ({} bytes, {} lines) -> {} chunks", 
            file_name, file_size, total_lines, total_chunks);
        crate::info_log!("[QueryFile] Chunk config: {} tokens/chunk, {} overlap",
            chunk_config.effective_chunk_size(), chunk_config.overlap_tokens);

        // Spawn workers for each chunk
        let mut chunk_ids = Vec::new();
        let mut handles = Vec::new();

        for (index, chunk) in chunk_result.chunks.iter().enumerate() {
            // Create unique chunk_id: "{filename}_chunk_{index}"
            let chunk_id = format!("{}_chunk_{}", file_name, index);
            
            // Extract chunk content
            let chunk_content = Self::extract_chunk_content(&file_content, chunk.line_start, chunk.line_end);
            
            // Create mpsc channel for this worker
            let (tx, rx) = mpsc::channel::<ChunkWorkerMessage>(10);
            
            // Register in registry
            self.registry.register(chunk_id.clone(), tx).await;
            chunk_ids.push(chunk_id.clone());

            crate::info_log!("[QueryFile] Spawning worker {} (lines {}-{}, ~{} tokens)",
                chunk_id, chunk.line_start, chunk.line_end,
                chunk.estimated_tokens());

            // Spawn the worker task
            let handle = self.spawn_chunk_worker(
                chunk_id,
                chunk_content,
                chunk.line_start,
                chunk.line_end,
                rx,
            );
            handles.push(handle);
        }

        // Register file -> chunks mapping for cleanup
        self.registry.register_file_chunks(file_name.clone(), chunk_ids.clone()).await;

        // Map-reduce: Broadcast prompt to all workers and collect responses
        crate::info_log!("[QueryFile] Broadcasting prompt to {} workers...", chunk_ids.len());
        
        if let Some(ref tx) = self.output_tx {
            let _ = tx.send(crate::agent::runtime::orchestrator::OutputEvent::Status {
                message: format!("Spawning {} parallel chunk workers...", chunk_ids.len()),
            });
        }
        
        use futures::stream::{FuturesUnordered, StreamExt};
        let mut futures = FuturesUnordered::new();

        for chunk_id in &chunk_ids {
            let chunk_id = chunk_id.clone();
            let prompt = args.prompt.clone();
            let registry = self.registry.clone();

            futures.push(async move {
                if let Some(tx) = registry.get(&chunk_id).await {
                    let (response_tx, response_rx) = oneshot::channel();
                    
                    let msg = ChunkWorkerMessage {
                        query: prompt,
                        response_tx,
                    };

                    if tx.send(msg).await.is_ok() {
                        // Wait for response with timeout
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(45),
                            response_rx
                        ).await {
                            Ok(Ok(response)) => {
                                let preview: String = response.answer.chars().take(80).collect();
                                crate::info_log!("[QueryFile] {} responded: '{}'...", chunk_id, preview);
                                
                                serde_json::json!({
                                    "chunk_id": chunk_id,
                                    "response": response.answer,
                                    "is_relevant": response.is_relevant,
                                    "confidence": response.confidence,
                                })
                            }
                            Ok(Err(_)) => {
                                crate::warn_log!("[QueryFile] {} response channel dropped", chunk_id);
                                serde_json::json!({
                                    "chunk_id": chunk_id,
                                    "error": "Response channel dropped",
                                })
                            }
                            Err(_) => {
                                crate::warn_log!("[QueryFile] {} timeout waiting for response", chunk_id);
                                serde_json::json!({
                                    "chunk_id": chunk_id,
                                    "error": "Timeout",
                                })
                            }
                        }
                    } else {
                        crate::warn_log!("[QueryFile] {} failed to send message", chunk_id);
                        serde_json::json!({
                            "chunk_id": chunk_id,
                            "error": "Failed to send message",
                        })
                    }
                } else {
                    crate::warn_log!("[QueryFile] {} not found in registry", chunk_id);
                    serde_json::json!({
                        "chunk_id": chunk_id,
                        "error": "Worker not found",
                    })
                }
            });
        }

        let mut responses = Vec::new();
        let mut relevant_count = 0;
        let mut completed_count = 0;
        let total_workers = chunk_ids.len();

        while let Some(result) = futures.next().await {
            completed_count += 1;
            
            if let Some(ref tx) = self.output_tx {
                let _ = tx.send(crate::agent::runtime::orchestrator::OutputEvent::Status {
                    message: format!("Analyzing document: {}/{} chunks completed", completed_count, total_workers),
                });
            }

            if let Some(is_relevant) = result.get("is_relevant").and_then(|v| v.as_bool()) {
                if is_relevant {
                    relevant_count += 1;
                }
            }
            responses.push(result);
        }

        crate::info_log!("[QueryFile] Completed: {}/{} chunks responded with relevant content", 
            relevant_count, chunk_ids.len());

        // Format responses for the main agent to synthesize
        let mut combined_output = String::new();
        if relevant_count == 0 {
            combined_output.push_str("No relevant content found across chunks.");
        } else {
            for res in &responses {
                if let Some(chunk_id) = res.get("chunk_id").and_then(|v| v.as_str()) {
                    if let Some(answer) = res.get("response").and_then(|v| v.as_str()) {
                        if !answer.contains("NOT_RELEVANT") {
                            combined_output.push_str(&format!("--- Chunk {} ---\n{}\n\n", chunk_id, answer));
                        }
                    }
                }
            }
        }

        let mapping_text = if !pdf_page_mapping.is_empty() {
            format!("\nChunk/Page Mapping for specific searches:\n{}", pdf_page_mapping.join("\n"))
        } else {
            String::new()
        };

        let output = format!(
            "Document processed: '{}' ({} chunks, {}/{} relevant)\n\n\
            {}\n\n\
            ---\n\
            For follow-up questions about specific sections, use query_chunk_worker with these chunk_ids: {}{}",
            file_name,
            total_chunks,
            relevant_count,
            chunk_ids.len(),
            combined_output,
            chunk_ids.join(", "),
            mapping_text
        );

        let result = serde_json::json!({
            "file": file_name,
            "total_chunks": total_chunks,
            "chunk_ids": chunk_ids,
            "relevant_responses": relevant_count,
            "responses": responses,
        });

        Ok(ToolResult::Success {
            output,
            structured: Some(result),
        })
    }
}

impl QueryFileTool {
    /// Extract chunk content from full file text
    fn extract_chunk_content(content: &str, line_start: usize, line_end: usize) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let start_idx = (line_start.saturating_sub(1)).min(lines.len());
        let end_idx = line_end.min(lines.len());
        lines[start_idx..end_idx].join("\n")
    }

}

impl Capability for QueryFileTool {
    fn name(&self) -> &'static str {
        "query_file"
    }
}

// ============================================================================
// Tool 2: query_chunk - Send query to a specific chunk worker
// ============================================================================

/// Arguments for query_chunk tool
#[derive(Debug, Clone, serde::Deserialize)]
pub struct QueryChunkArgs {
    /// Chunk ID (format: "{filename}_chunk_{index}")
    pub chunk_id: String,
    /// Query/prompt to send to the chunk
    pub prompt: String,
    /// Timeout in seconds (default: 30)
    #[serde(default = "default_chunk_timeout")]
    pub timeout_secs: u64,
}

fn default_chunk_timeout() -> u64 {
    30
}

/// Tool that queries a specific chunk worker
pub struct QueryChunkTool {
    registry: Arc<ChunkWorkerRegistry>,
}

impl QueryChunkTool {
    pub fn new(registry: Arc<ChunkWorkerRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl ToolCapability for QueryChunkTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        let args: QueryChunkArgs = match serde_json::from_value(call.arguments) {
            Ok(a) => a,
            Err(e) => {
                return Ok(ToolResult::Error {
                    message: format!("Invalid arguments: {}. Expected: {{\"chunk_id\": \"doc_chunk_0\", \"prompt\": \"question\"}}", e),
                    code: Some("PARSE_ERROR".to_string()),
                    retryable: false,
                });
            }
        };

        let chunk_id = args.chunk_id;
        let prompt_preview: String = args.prompt.chars().take(50).collect();
        
        crate::info_log!("[QueryChunk] Querying {}: '{}'...", chunk_id, prompt_preview);

        // Get the worker's sender channel from registry
        let tx = match self.registry.get(&chunk_id).await {
            Some(tx) => tx,
            None => {
                return Ok(ToolResult::Error {
                    message: format!("Chunk worker '{}' not found. Use query_file first to spawn workers.", chunk_id),
                    code: Some("WORKER_NOT_FOUND".to_string()),
                    retryable: false,
                });
            }
        };

        // Create oneshot channel for response
        let (response_tx, response_rx) = oneshot::channel();

        let msg = ChunkWorkerMessage {
            query: args.prompt,
            response_tx,
        };

        // Send message to worker
        if let Err(_) = tx.send(msg).await {
            return Ok(ToolResult::Error {
                message: format!("Failed to send query to worker '{}'. Worker may have shut down.", chunk_id),
                code: Some("WORKER_UNAVAILABLE".to_string()),
                retryable: true,
            });
        }

        // Wait for response with timeout
        let timeout = std::time::Duration::from_secs(args.timeout_secs);
        
        match tokio::time::timeout(timeout, response_rx).await {
            Ok(Ok(response)) => {
                let preview: String = response.answer.chars().take(100).collect();
                crate::info_log!("[QueryChunk] {} responded: '{}'...", chunk_id, preview);

                let output = if response.is_relevant {
                    response.answer.clone()
                } else {
                    format!("Chunk {}: NOT_RELEVANT\n\nResponse: {}", chunk_id, response.answer)
                };

                let result = serde_json::json!({
                    "chunk_id": chunk_id,
                    "response": response.answer,
                    "is_relevant": response.is_relevant,
                    "confidence": response.confidence,
                });

                Ok(ToolResult::Success {
                    output,
                    structured: Some(result),
                })
            }
            Ok(Err(_)) => {
                Ok(ToolResult::Error {
                    message: format!("Worker '{}' response channel closed unexpectedly.", chunk_id),
                    code: Some("WORKER_ERROR".to_string()),
                    retryable: true,
                })
            }
            Err(_) => {
                Ok(ToolResult::Error {
                    message: format!("Timeout waiting for worker '{}' response after {} seconds.", chunk_id, args.timeout_secs),
                    code: Some("TIMEOUT".to_string()),
                    retryable: true,
                })
            }
        }
    }
}

impl Capability for QueryChunkTool {
    fn name(&self) -> &'static str {
        "query_chunk_worker"
    }
}

// ============================================================================
// Tool 3: close_file - Clean up chunk workers for a file
// ============================================================================

/// Arguments for close_file tool
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CloseFileArgs {
    /// File name (as used in query_file)
    pub file_name: String,
}

/// Tool that cleans up chunk workers for a file
pub struct CloseFileTool {
    registry: Arc<ChunkWorkerRegistry>,
}

impl CloseFileTool {
    pub fn new(registry: Arc<ChunkWorkerRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl ToolCapability for CloseFileTool {
    async fn execute(
        &self,
        _ctx: &RuntimeContext,
        call: ToolCall,
    ) -> Result<ToolResult, ToolError> {
        let args: CloseFileArgs = match serde_json::from_value(call.arguments) {
            Ok(a) => a,
            Err(e) => {
                return Ok(ToolResult::Error {
                    message: format!("Invalid arguments: {}. Expected: {{\"file_name\": \"doc.txt\"}}", e),
                    code: Some("PARSE_ERROR".to_string()),
                    retryable: false,
                });
            }
        };

        let file_name = args.file_name;
        crate::info_log!("[CloseFile] Cleaning up workers for: '{}'", file_name);

        // Remove all chunks for this file from registry
        // This drops the tx channels, causing workers to shut down
        let removed_chunks = self.registry.remove_file(&file_name).await;

        if removed_chunks.is_empty() {
            return Ok(ToolResult::Success {
                output: format!("No active workers found for file '{}'.", file_name),
                structured: Some(serde_json::json!({
                    "file_name": file_name,
                    "removed_count": 0,
                    "removed_chunks": [],
                })),
            });
        }

        crate::info_log!("[CloseFile] Removed {} workers for '{}': {}",
            removed_chunks.len(), file_name, removed_chunks.join(", "));

        let output = format!(
            "Cleaned up {} chunk workers for file '{}'.\n\
             Removed: {}",
            removed_chunks.len(),
            file_name,
            removed_chunks.join(", ")
        );

        let result = serde_json::json!({
            "file_name": file_name,
            "removed_count": removed_chunks.len(),
            "removed_chunks": removed_chunks,
        });

        Ok(ToolResult::Success {
            output,
            structured: Some(result),
        })
    }
}

impl Capability for CloseFileTool {
    fn name(&self) -> &'static str {
        "close_file"
    }
}

// Types are already public, no re-exports needed
