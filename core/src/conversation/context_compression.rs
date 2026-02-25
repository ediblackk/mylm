//! Context Compression Module
//!
//! Prevents the "disappearing message" problem by:
//! 1. Smart preservation (extract memories before compression)
//! 2. Visual compression indicators in UI
//! 3. Archive and recovery of trimmed content

use crate::conversation::manager::Message;
// Note: ContextManager integration is in manager.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A segment of compressed/trimmed messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedSegment {
    /// Unique ID for this segment
    pub id: String,
    
    /// When it was compressed
    pub timestamp: DateTime<Utc>,
    
    /// How many messages were pruned
    pub message_count: usize,
    
    /// Approximate tokens saved
    pub tokens_saved: usize,
    
    /// Summary of what was pruned
    pub summary: String,
    
    /// Full content that was pruned
    pub messages: Vec<Message>,
    
    /// Memories extracted before compression
    pub extracted_memories: Vec<String>,
    
    /// Whether user has acknowledged/viewed this
    pub acknowledged: bool,
}

impl CompressedSegment {
    /// Create a new compressed segment
    pub fn new(messages: Vec<Message>, tokens_saved: usize) -> Self {
        let message_count = messages.len();
        let summary = generate_summary(&messages);
        let extracted_memories = extract_memories(&messages);
        
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            message_count,
            tokens_saved,
            summary,
            messages,
            extracted_memories,
            acknowledged: false,
        }
    }
    
    /// Mark as acknowledged
    pub fn acknowledge(&mut self) {
        self.acknowledged = true;
    }
    
    /// Format for display in UI
    pub fn format_indicator(&self) -> String {
        let mem_info = if self.extracted_memories.is_empty() {
            String::new()
        } else {
            format!(" 💾 {} memories auto-saved", self.extracted_memories.len())
        };
        
        format!(
            "💾 Context compressed: {} messages summarized (saved ~{} tokens){}\n   \"{}\"\n   [View] [Restore] [Dismiss]",
            self.message_count,
            self.tokens_saved,
            mem_info,
            self.summary.chars().take(80).collect::<String>()
        )
    }
}

/// Archive for compressed segments
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompressionArchive {
    /// Archive of compressed segments (oldest first)
    segments: VecDeque<CompressedSegment>,
    
    /// Max segments to keep in memory
    max_segments: usize,
}

impl CompressionArchive {
    /// Create new archive with max size
    pub fn new(max_segments: usize) -> Self {
        Self {
            segments: VecDeque::with_capacity(max_segments),
            max_segments,
        }
    }
    
    /// Add a compressed segment
    pub fn push(&mut self, segment: CompressedSegment) {
        if self.segments.len() >= self.max_segments {
            self.segments.pop_front(); // Remove oldest
        }
        self.segments.push_back(segment);
    }
    
    /// Get all segments
    pub fn segments(&self) -> &VecDeque<CompressedSegment> {
        &self.segments
    }
    
    /// Get a specific segment by ID
    pub fn get(&self, id: &str) -> Option<&CompressedSegment> {
        self.segments.iter().find(|s| s.id == id)
    }
    
    /// Get mutable reference to segment
    pub fn get_mut(&mut self, id: &str) -> Option<&mut CompressedSegment> {
        self.segments.iter_mut().find(|s| s.id == id)
    }
    
    /// Restore a segment (returns the messages)
    pub fn restore(&mut self, id: &str) -> Option<Vec<Message>> {
        let pos = self.segments.iter().position(|s| s.id == id)?;
        let segment = self.segments.remove(pos)?;
        Some(segment.messages)
    }
    
    /// Search compression archive
    pub fn search(&self, query: &str) -> Vec<&CompressedSegment> {
        let query_lower = query.to_lowercase();
        self.segments
            .iter()
            .filter(|s| {
                s.summary.to_lowercase().contains(&query_lower)
                    || s.messages.iter().any(|m| {
                        m.content.to_lowercase().contains(&query_lower)
                    })
            })
            .collect()
    }
    
    /// Format for display
    pub fn format_list(&self) -> String {
        if self.segments.is_empty() {
            return "No compressed segments in archive.".to_string();
        }
        
        let mut output = format!("📦 Compression Archive ({} segments)\n\n", self.segments.len());
        
        for (i, segment) in self.segments.iter().enumerate() {
            let status = if segment.acknowledged { "✓" } else { "○" };
            output.push_str(&format!(
                "[{}] #{}: {} - {} messages, ~{} tokens saved\n   \"{}...\"\n\n",
                status,
                i + 1,
                segment.timestamp.format("%H:%M:%S"),
                segment.message_count,
                segment.tokens_saved,
                segment.summary.chars().take(50).collect::<String>()
            ));
        }
        
        output.push_str("Use /restore <number> to restore a segment\n");
        output
    }
}

/// Configuration for context compression
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Patterns that indicate important content
    pub preserve_patterns: Vec<String>,
    
    /// Auto-extract memories before compression
    pub auto_extract_memories: bool,
    
    /// Keep first N messages (setup/context)
    pub keep_first: usize,
    
    /// Keep last N messages (recent conversation)
    pub keep_last: usize,
    
    /// Max segments to archive
    pub max_archive_size: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            preserve_patterns: vec![
                "remember".to_string(),
                "important".to_string(),
                "critical".to_string(),
                "always".to_string(),
                "never".to_string(),
            ],
            auto_extract_memories: true,
            keep_first: 1,  // Keep system prompt
            keep_last: 4,   // Keep last 2 exchanges
            max_archive_size: 10,
        }
    }
}

/// Generate a summary of compressed messages
fn generate_summary(messages: &[Message]) -> String {
    if messages.is_empty() {
        return "No messages".to_string();
    }
    
    // Count message types
    let user_count = messages.iter().filter(|m| m.role == "user").count();
    let assistant_count = messages.iter().filter(|m| m.role == "assistant").count();
    let tool_count = messages.iter().filter(|m| m.role == "tool").count();
    
    // Extract key topics from user messages
    let topics: Vec<String> = messages
        .iter()
        .filter(|m| m.role == "user")
        .take(3)
        .map(|m| {
            // Extract first few words
            let words: Vec<&str> = m.content.split_whitespace().take(5).collect();
            words.join(" ")
        })
        .collect();
    
    let mut parts = vec![];
    if user_count > 0 {
        parts.push(format!("{} user", user_count));
    }
    if assistant_count > 0 {
        parts.push(format!("{} assistant", assistant_count));
    }
    if tool_count > 0 {
        parts.push(format!("{} tool", tool_count));
    }
    
    let topic_str = if topics.is_empty() {
        "messages".to_string()
    } else {
        format!("about {}", topics.join(", "))
    };
    
    format!("{} {} {}", parts.join(", "), topic_str, "compressed")
}

/// Extract memories from messages before compression
fn extract_memories(messages: &[Message]) -> Vec<String> {
    let mut memories = vec![];
    
    for msg in messages {
        // Look for JSON with "r" or "remember" field
        if let Some(memory) = extract_json_memory(&msg.content) {
            memories.push(memory);
        }
        
        // Look for explicit remember markers
        if msg.content.to_lowercase().contains("remember") {
            // Extract the sentence containing "remember"
            for sentence in msg.content.split('.') {
                if sentence.to_lowercase().contains("remember") {
                    memories.push(sentence.trim().to_string());
                }
            }
        }
        
        // Extract user preferences/corrections
        if msg.role == "user" {
            // Patterns like "I prefer", "Use X not Y", etc.
            if msg.content.contains("prefer") 
                || msg.content.contains("use ")
                || msg.content.contains("don't use")
            {
                memories.push(format!("User preference: {}", 
                    msg.content.chars().take(100).collect::<String>()));
            }
        }
    }
    
    memories
}

/// Try to extract memory from JSON content
fn extract_json_memory(content: &str) -> Option<String> {
    // Look for {"r": "..."} or {"remember": "..."}
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(r) = json.get("r").and_then(|v| v.as_str()) {
            return Some(r.to_string());
        }
        if let Some(rem) = json.get("remember").and_then(|v| v.as_str()) {
            return Some(rem.to_string());
        }
    }
    None
}

/// Context compression result
#[derive(Debug)]
pub struct CompressionResult {
    /// Messages to keep
    pub kept: Vec<Message>,
    
    /// Messages that were trimmed (archived)
    pub trimmed: Vec<Message>,
    
    /// The compressed segment (for UI indicator)
    pub segment: CompressedSegment,
    
    /// Whether compression was needed
    pub was_compressed: bool,
}

/// Perform context compression
pub fn compress_context(
    messages: Vec<Message>,
    token_limit: usize,
    config: &CompressionConfig,
) -> CompressionResult {
    let total_tokens: usize = messages.iter()
        .map(|m| m.content.len() / 4 + 1)
        .sum();
    
    // If under limit, no pruning needed
    if total_tokens <= token_limit {
        return CompressionResult {
            kept: messages,
            trimmed: vec![],
            segment: CompressedSegment::new(vec![], 0),
            was_compressed: false,
        };
    }
    
    // Identify important messages to preserve
    let (important, remaining): (Vec<Message>, Vec<Message>) = messages
        .into_iter()
        .partition(|m| should_preserve(m, config));
    
    // Calculate remaining budget
    let important_tokens: usize = important.iter()
        .map(|m| m.content.len() / 4 + 1)
        .sum();
    let _remaining_budget = token_limit.saturating_sub(important_tokens);
    
    // Keep recent messages within budget
    let mut kept = important;
    let mut trimmed = vec![];
    let mut current_tokens = important_tokens;
    
    // Work backwards through remaining messages
    for msg in remaining.into_iter().rev() {
        let msg_tokens = msg.content.len() / 4 + 1;
        if current_tokens + msg_tokens <= token_limit {
            kept.push(msg);
            current_tokens += msg_tokens;
        } else {
            trimmed.push(msg);
        }
    }
    
    // Reverse to maintain order
    kept.reverse();
    trimmed.reverse();
    
    // Create compressed segment
    let tokens_saved = trimmed.iter().map(|m| m.content.len() / 4 + 1).sum();
    let segment = CompressedSegment::new(trimmed.clone(), tokens_saved);
    
    CompressionResult {
        kept,
        trimmed,
        segment,
        was_compressed: true,
    }
}

/// Check if a message should be preserved
fn should_preserve(msg: &Message, config: &CompressionConfig) -> bool {
    let content_lower = msg.content.to_lowercase();
    
    // Check preserve patterns
    for pattern in &config.preserve_patterns {
        if content_lower.contains(pattern) {
            return true;
        }
    }
    
    // Preserve system messages
    if msg.role == "system" {
        return true;
    }
    
    // Preserve tool results (they contain facts)
    if msg.role == "tool" {
        return true;
    }
    
    // Preserve user corrections
    if msg.role == "user" {
        if content_lower.starts_with("no,")
            || content_lower.starts_with("wrong")
            || content_lower.starts_with("incorrect")
            || content_lower.contains("i said")
            || content_lower.contains("actually")
        {
            return true;
        }
    }
    
    false
}

/// Auto-restore result
#[derive(Debug, Clone)]
pub struct AutoRestoreResult {
    /// Segments that matched and should be restored
    pub segments: Vec<CompressedSegment>,
    /// Whether any segments were found
    pub found: bool,
    /// Search keywords that matched
    pub keywords: Vec<String>,
}

/// Check if user message references compressed content
/// 
/// This is called before processing user input to see if we need
/// to auto-restore any compressed context.
pub fn check_auto_restore(user_message: &str, archive: &CompressionArchive) -> AutoRestoreResult {
    if archive.segments().is_empty() {
        return AutoRestoreResult {
            segments: vec![],
            found: false,
            keywords: vec![],
        };
    }
    
    // Extract key terms from user message (nouns, verbs, proper nouns)
    let keywords = extract_keywords(user_message);
    
    if keywords.is_empty() {
        return AutoRestoreResult {
            segments: vec![],
            found: false,
            keywords: vec![],
        };
    }
    
    // Find segments that contain these keywords
    let mut matching_segments = vec![];
    let mut matched_keywords = vec![];
    
    for segment in archive.segments() {
        let segment_text = format!(
            "{} {}",
            segment.summary,
            segment.messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join(" ")
        ).to_lowercase();
        
        let mut segment_matches = 0;
        for keyword in &keywords {
            if segment_text.contains(&keyword.to_lowercase()) {
                segment_matches += 1;
                if !matched_keywords.contains(keyword) {
                    matched_keywords.push(keyword.clone());
                }
            }
        }
        
        // If segment matches at least 2 keywords or 50% of keywords, restore it
        let threshold = if keywords.len() == 1 { 1 } else { 2 };
        if segment_matches >= threshold || (segment_matches as f32 / keywords.len() as f32) >= 0.5 {
            matching_segments.push(segment.clone());
        }
    }
    
    let found = !matching_segments.is_empty();
    
    AutoRestoreResult {
        segments: matching_segments,
        found,
        keywords: matched_keywords,
    }
}

/// Extract keywords from user message
fn extract_keywords(text: &str) -> Vec<String> {
    let text_lower = text.to_lowercase();
    
    // Common stop words to filter out
    let stop_words: std::collections::HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "must", "shall", "can", "need", "dare",
        "ought", "used", "to", "of", "in", "for", "on", "with", "at", "by",
        "from", "as", "into", "through", "during", "before", "after", "above",
        "below", "between", "under", "and", "but", "or", "yet", "so", "if",
        "because", "although", "though", "while", "where", "when", "that",
        "which", "who", "whom", "whose", "what", "this", "these", "those",
        "i", "you", "he", "she", "it", "we", "they", "me", "him", "her",
        "us", "them", "my", "your", "his", "her", "its", "our", "their",
        "mine", "yours", "hers", "ours", "theirs", "what", "which", "who",
        "whom", "whose", "this", "that", "these", "those", "am", "it", "did",
        "does", "doing", "done", "get", "got", "getting", "gotten", "make",
        "made", "making", "go", "went", "gone", "going", "come", "came",
        "coming", "see", "saw", "seen", "seeing", "know", "knew", "known",
        "knowing", "think", "thought", "thinking", "say", "said", "saying",
        "tell", "told", "telling", "ask", "asked", "asking", "seem", "seemed",
        "seeming", "feel", "felt", "feeling", "try", "tried", "trying", "want",
        "wanted", "wanting", "use", "used", "using", "work", "worked", "working",
        "call", "called", "calling", "try", "tried", "trying", "need", "needed",
        "needing", "become", "became", "becoming", "leave", "left", "leaving",
        "put", "puts", "putting", "mean", "meant", "meaning", "keep", "kept",
        "keeping", "let", "lets", "letting", "begin", "began", "begun",
        "beginning", "help", "helped", "helping", "show", "showed", "shown",
        "showing", "hear", "heard", "hearing", "play", "played", "playing",
        "run", "ran", "running", "move", "moved", "moving", "like", "liked",
        "liking", "live", "lived", "living", "believe", "believed", "believing",
        "hold", "held", "holding", "bring", "brought", "bringing", "happen",
        "happened", "happening", "write", "wrote", "written", "writing",
        "provide", "provided", "providing", "sit", "sat", "sitting", "stand",
        "stood", "standing", "lose", "lost", "losing", "pay", "paid", "paying",
        "meet", "met", "meeting", "include", "included", "including", "continue",
        "continued", "continuing", "set", "sets", "setting", "learn", "learned",
        "learning", "change", "changed", "changing", "lead", "led", "leading",
        "understand", "understood", "understanding", "watch", "watched",
        "watching", "follow", "followed", "following", "stop", "stopped",
        "stopping", "create", "created", "creating", "speak", "spoke", "spoken",
        "speaking", "read", "reading", "allow", "allowed", "allowing", "add",
        "added", "adding", "spend", "spent", "spending", "grow", "grew",
        "grown", "growing", "open", "opened", "opening", "walk", "walked",
        "walking", "win", "won", "winning", "offer", "offered", "offering",
        "remember", "remembered", "remembering", "love", "loved", "loving",
        "consider", "considered", "considering", "appear", "appeared",
        "appearing", "buy", "bought", "buying", "wait", "waited", "waiting",
        "serve", "served", "serving", "die", "died", "dying", "send", "sent",
        "sending", "expect", "expected", "expecting", "build", "built",
        "building", "stay", "stayed", "staying", "fall", "fell", "fallen",
        "falling", "cut", "cuts", "cutting", "reach", "reached", "reaching",
        "kill", "killed", "killing", "remain", "remained", "remaining",
        "suggest", "suggested", "suggesting", "raise", "raised", "raising",
        "pass", "passed", "passing", "sell", "sold", "selling", "require",
        "required", "requiring", "report", "reported", "reporting", "decide",
        "decided", "deciding", "pull", "pulled", "pulling", "good", "great",
        "nice", "well", "better", "best", "bad", "worse", "worst", "far",
        "further", "furthest", "many", "more", "most", "much", "some",
        "any", "all", "both", "each", "few", "other", "another", "such",
        "only", "own", "same", "so", "than", "too", "very", "just", "now",
        "then", "here", "there", "also", "back", "still", "even", "only",
        "way", "may", "say", "how", "its", "who", "did", "get", "via",
        "yes", "no", "not", "ok", "okay", "sure", "thanks", "thank", "please",
    ].iter().cloned().collect();
    
    // Extract words (3+ chars, not stop words)
    let words: Vec<String> = text_lower
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.len() >= 3 && !stop_words.contains(*w))
        .map(|w| w.to_string())
        .collect::<std::collections::HashSet<_>>() // Deduplicate
        .into_iter()
        .collect();
    
    words
}

/// Extension trait for ContextManager to use context compression
pub trait ContextCompression {
    /// Perform context compression and return segment for UI
    fn compress_with_indicator(&mut self, config: &CompressionConfig) -> Option<CompressedSegment>;
    
    /// Get compression archive
    fn compression_archive(&self) -> &CompressionArchive;
    
    /// Get mutable compression archive
    fn compression_archive_mut(&mut self) -> &mut CompressionArchive;
    
    /// Check if user message references compressed content
    fn check_auto_restore(&self, user_message: &str) -> AutoRestoreResult;
}
