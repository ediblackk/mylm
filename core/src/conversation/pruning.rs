//! Smart Pruning System
//!
//! Prevents the "disappearing message" problem by:
//! 1. Smart preservation (extract memories before pruning)
//! 2. Visual pruning indicators in UI
//! 3. Archive and recovery of pruned content

use crate::agent::cognition::history::{Message, MessageRole};
// Note: ContextManager integration is in manager.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A segment of pruned messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrunedSegment {
    /// Unique ID for this segment
    pub id: String,
    
    /// When it was pruned
    pub timestamp: DateTime<Utc>,
    
    /// How many messages were pruned
    pub message_count: usize,
    
    /// Approximate tokens saved
    pub tokens_saved: usize,
    
    /// Summary of what was pruned
    pub summary: String,
    
    /// Full content that was pruned
    pub messages: Vec<Message>,
    
    /// Memories extracted before pruning
    pub extracted_memories: Vec<String>,
    
    /// Whether user has acknowledged/viewed this
    pub acknowledged: bool,
}

impl PrunedSegment {
    /// Create a new pruned segment
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

/// Archive for pruned segments
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrunedHistory {
    /// Archive of pruned segments (oldest first)
    segments: VecDeque<PrunedSegment>,
    
    /// Max segments to keep in memory
    max_segments: usize,
}

impl PrunedHistory {
    /// Create new archive with max size
    pub fn new(max_segments: usize) -> Self {
        Self {
            segments: VecDeque::with_capacity(max_segments),
            max_segments,
        }
    }
    
    /// Add a pruned segment
    pub fn push(&mut self, segment: PrunedSegment) {
        if self.segments.len() >= self.max_segments {
            self.segments.pop_front(); // Remove oldest
        }
        self.segments.push_back(segment);
    }
    
    /// Get all segments
    pub fn segments(&self) -> &VecDeque<PrunedSegment> {
        &self.segments
    }
    
    /// Get a specific segment by ID
    pub fn get(&self, id: &str) -> Option<&PrunedSegment> {
        self.segments.iter().find(|s| s.id == id)
    }
    
    /// Get mutable reference to segment
    pub fn get_mut(&mut self, id: &str) -> Option<&mut PrunedSegment> {
        self.segments.iter_mut().find(|s| s.id == id)
    }
    
    /// Restore a segment (returns the messages)
    pub fn restore(&mut self, id: &str) -> Option<Vec<Message>> {
        let pos = self.segments.iter().position(|s| s.id == id)?;
        let segment = self.segments.remove(pos)?;
        Some(segment.messages)
    }
    
    /// Search pruned history
    pub fn search(&self, query: &str) -> Vec<&PrunedSegment> {
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
            return "No pruned segments in archive.".to_string();
        }
        
        let mut output = format!("📦 Pruned History ({} segments)\n\n", self.segments.len());
        
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

/// Configuration for smart pruning
#[derive(Debug, Clone)]
pub struct SmartPruningConfig {
    /// Patterns that indicate important content
    pub preserve_patterns: Vec<String>,
    
    /// Auto-extract memories before pruning
    pub auto_extract_memories: bool,
    
    /// Keep first N messages (setup/context)
    pub keep_first: usize,
    
    /// Keep last N messages (recent conversation)
    pub keep_last: usize,
    
    /// Max segments to archive
    pub max_archive_size: usize,
}

impl Default for SmartPruningConfig {
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

/// Generate a summary of pruned messages
fn generate_summary(messages: &[Message]) -> String {
    if messages.is_empty() {
        return "No messages".to_string();
    }
    
    // Count message types
    let user_count = messages.iter().filter(|m| m.role == MessageRole::User).count();
    let assistant_count = messages.iter().filter(|m| m.role == MessageRole::Assistant).count();
    let tool_count = messages.iter().filter(|m| m.role == MessageRole::Tool).count();
    
    // Extract key topics from user messages
    let topics: Vec<String> = messages
        .iter()
        .filter(|m| m.role == MessageRole::User)
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
    
    format!("{} {} {}", parts.join(", "), topic_str, "pruned")
}

/// Extract memories from messages before pruning
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
        if msg.role == MessageRole::User {
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

/// Smart pruning result
#[derive(Debug)]
pub struct SmartPruneResult {
    /// Messages to keep
    pub kept: Vec<Message>,
    
    /// Messages that were pruned (archived)
    pub pruned: Vec<Message>,
    
    /// The pruned segment (for UI indicator)
    pub segment: PrunedSegment,
    
    /// Whether pruning was needed
    pub was_pruned: bool,
}

/// Perform smart pruning
pub fn smart_prune(
    messages: Vec<Message>,
    token_limit: usize,
    config: &SmartPruningConfig,
) -> SmartPruneResult {
    let total_tokens: usize = messages.iter()
        .map(|m| m.content.len() / 4 + 1)
        .sum();
    
    // If under limit, no pruning needed
    if total_tokens <= token_limit {
        return SmartPruneResult {
            kept: messages,
            pruned: vec![],
            segment: PrunedSegment::new(vec![], 0),
            was_pruned: false,
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
    let mut pruned = vec![];
    let mut current_tokens = important_tokens;
    
    // Work backwards through remaining messages
    for msg in remaining.into_iter().rev() {
        let msg_tokens = msg.content.len() / 4 + 1;
        if current_tokens + msg_tokens <= token_limit {
            kept.push(msg);
            current_tokens += msg_tokens;
        } else {
            pruned.push(msg);
        }
    }
    
    // Reverse to maintain order
    kept.reverse();
    pruned.reverse();
    
    // Create pruned segment
    let tokens_saved = pruned.iter().map(|m| m.content.len() / 4 + 1).sum();
    let segment = PrunedSegment::new(pruned.clone(), tokens_saved);
    
    SmartPruneResult {
        kept,
        pruned,
        segment,
        was_pruned: true,
    }
}

/// Check if a message should be preserved
fn should_preserve(msg: &Message, config: &SmartPruningConfig) -> bool {
    let content_lower = msg.content.to_lowercase();
    
    // Check preserve patterns
    for pattern in &config.preserve_patterns {
        if content_lower.contains(pattern) {
            return true;
        }
    }
    
    // Preserve system messages
    if msg.role == MessageRole::System {
        return true;
    }
    
    // Preserve tool results (they contain facts)
    if msg.role == MessageRole::Tool {
        return true;
    }
    
    // Preserve user corrections
    if msg.role == MessageRole::User {
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
    pub segments: Vec<PrunedSegment>,
    /// Whether any segments were found
    pub found: bool,
    /// Search keywords that matched
    pub keywords: Vec<String>,
}

/// Check if user message references pruned content
/// 
/// This is called before processing user input to see if we need
/// to auto-restore any pruned context.
pub fn check_auto_restore(user_message: &str, history: &PrunedHistory) -> AutoRestoreResult {
    if history.segments().is_empty() {
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
    
    for segment in history.segments() {
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

/// Extension trait for ContextManager to use smart pruning
pub trait SmartPruning {
    /// Perform smart pruning and return segment for UI
    fn smart_prune_with_indicator(&mut self, config: &SmartPruningConfig) -> Option<PrunedSegment>;
    
    /// Get archive of pruned segments
    fn pruned_history(&self) -> &PrunedHistory;
    
    /// Get mutable archive
    fn pruned_history_mut(&mut self) -> &mut PrunedHistory;
    
    /// Check if user message references pruned content
    fn check_auto_restore(&self, user_message: &str) -> AutoRestoreResult;
}
