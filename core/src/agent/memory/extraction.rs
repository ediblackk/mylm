//! Smart Memory Extraction
//!
//! Automatically extracts high-signal memories from user messages
//! without requiring the LLM to explicitly use the `r` field.
//!
//! This uses pattern matching for high-confidence extractions only,
//! avoiding memory bloat from low-signal messages.

use regex::Regex;

/// Result of memory extraction
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedMemory {
    /// The content to save
    pub content: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Source pattern that matched
    pub source: String,
}

/// Smart memory extractor
///
/// Uses high-signal patterns to extract facts worth remembering.
/// Only extracts when confidence is high - avoids saving noise.
pub struct MemoryExtractor {
    patterns: Vec<(Regex, String, f32)>,
}

impl MemoryExtractor {
    /// Create a new extractor with default patterns
    pub fn new() -> Self {
        let patterns = Self::build_patterns();
        Self { patterns }
    }
    
    /// Build the default extraction patterns
    ///
    /// These are high-signal patterns that indicate explicit user preferences,
    /// constraints, or important facts. Each pattern has a confidence score.
    fn build_patterns() -> Vec<(Regex, String, f32)> {
        vec![
            // Explicit preferences (high confidence)
            (
                Regex::new(r"(?i)(?:i prefer|i like|i love|i hate|i dislike|i always want|i never want)\s+(.{3,200})").unwrap(),
                "explicit_preference".to_string(),
                0.9,
            ),
            // Identity/self-reference (high confidence)
            (
                Regex::new(r"(?i)(?:my name is|i am a|i work as|i'm a|i work at|i'm working on)\s+(.{2,100})").unwrap(),
                "identity".to_string(),
                0.85,
            ),
            // Constraints/requirements (high confidence)
            (
                Regex::new(r"(?i)(?:must be|needs to be|has to be|should be|always use|never use)\s+(.{3,150})").unwrap(),
                "constraint".to_string(),
                0.85,
            ),
            // Goals/objectives (medium-high confidence)
            (
                Regex::new(r"(?i)(?:my goal is|i want to|i'm trying to|i need to|i'm aiming to)\s+(.{5,200})").unwrap(),
                "goal".to_string(),
                0.8,
            ),
            // Corrections (very high confidence - user explicitly correcting us)
            (
                Regex::new(r"(?i)(?:no[,.]?\s+(?:that's |it is |it's )?(?:wrong|incorrect|not right)|that's not right[,.]?\s*(.+)|i said\s+(.{3,150}))").unwrap(),
                "correction".to_string(),
                0.95,
            ),
            // Project context (medium confidence)
            (
                Regex::new(r"(?i)(?:this is a|we are building|this project is|the project uses)\s+(.{5,150})").unwrap(),
                "project_context".to_string(),
                0.75,
            ),
        ]
    }
    
    /// Extract memories from a user message
    ///
    /// Returns extracted memories that meet the confidence threshold.
    pub fn extract(&self, message: &str) -> Vec<ExtractedMemory> {
        let mut extracted = Vec::new();
        
        for (regex, source, base_confidence) in &self.patterns {
            for cap in regex.captures_iter(message) {
                // Get the captured group (the actual content)
                if let Some(content_match) = cap.get(1) {
                    let content = content_match.as_str().trim();
                    
                    // Skip if too short or too long
                    if content.len() < 5 || content.len() > 300 {
                        continue;
                    }
                    
                    // Adjust confidence based on content quality
                    let confidence = self.adjust_confidence(*base_confidence, content);
                    
                    // Only extract if confidence is high enough
                    if confidence >= 0.75 {
                        extracted.push(ExtractedMemory {
                            content: self.clean_content(content),
                            confidence,
                            source: source.clone(),
                        });
                    }
                }
            }
        }
        
        // Deduplicate similar extractions
        extracted = self.deduplicate(extracted);
        
        extracted
    }
    
    /// Check if a message is worth extracting from
    ///
    /// Quick filter to skip obviously low-signal messages
    pub fn should_extract(&self, message: &str) -> bool {
        let msg = message.trim();
        
        // Too short
        if msg.len() < 10 {
            return false;
        }
        
        // Questions usually don't contain facts to remember
        if msg.ends_with('?') {
            return false;
        }
        
        // Greetings/chitchat
        let lower = msg.to_lowercase();
        if lower.starts_with("hi") || lower.starts_with("hello") || lower.starts_with("hey") {
            return false;
        }
        
        // Acknowledgments
        if ["ok", "okay", "got it", "thanks", "thank you"].iter().any(|&s| lower == s) {
            return false;
        }
        
        true
    }
    
    /// Adjust confidence based on content quality
    fn adjust_confidence(&self, base: f32, content: &str) -> f32 {
        let mut adjustment = 0.0;
        
        // Penalize very short content
        if content.len() < 15 {
            adjustment -= 0.1;
        }
        
        // Penalize content with many special characters (likely code)
        let special_ratio = content.chars()
            .filter(|&c| !c.is_alphanumeric() && !c.is_whitespace() && c != '.' && c != ',' && c != '-')
            .count() as f32 / content.len() as f32;
        if special_ratio > 0.3 {
            adjustment -= 0.15;
        }
        
        // Boost content that looks like natural language
        let word_count = content.split_whitespace().count();
        if word_count >= 3 && word_count <= 30 {
            adjustment += 0.05;
        }
        
        // Boost first-person statements
        let lower = content.to_lowercase();
        if lower.starts_with("i ") || lower.starts_with("my ") {
            adjustment += 0.05;
        }
        
        (base + adjustment).clamp(0.0, 1.0)
    }
    
    /// Clean up extracted content
    fn clean_content(&self, content: &str) -> String {
        content
            .trim_end_matches('.')
            .trim_end_matches(',')
            .trim_end_matches('!')
            .trim()
            .to_string()
    }
    
    /// Deduplicate similar extractions
    fn deduplicate(&self, mut memories: Vec<ExtractedMemory>) -> Vec<ExtractedMemory> {
        if memories.len() <= 1 {
            return memories;
        }
        
        // Sort by confidence (highest first)
        memories.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        
        let mut result: Vec<ExtractedMemory> = Vec::new();
        
        for mem in memories {
            // Check if similar to any already-accepted memory
            let is_duplicate = result.iter().any(|existing| {
                self.similarity(&mem.content, &existing.content) > 0.8
            });
            
            if !is_duplicate {
                result.push(mem);
            }
        }
        
        result
    }
    
    /// Simple string similarity (Jaccard-like)
    fn similarity(&self, a: &str, b: &str) -> f32 {
        let a_lower = a.to_lowercase();
        let b_lower = b.to_lowercase();
        
        // Exact match
        if a_lower == b_lower {
            return 1.0;
        }
        
        // One contains the other
        if a_lower.contains(&b_lower) || b_lower.contains(&a_lower) {
            return 0.9;
        }
        
        // Word overlap
        let a_words: std::collections::HashSet<_> = a_lower.split_whitespace().collect();
        let b_words: std::collections::HashSet<_> = b_lower.split_whitespace().collect();
        
        let intersection: std::collections::HashSet<_> = a_words.intersection(&b_words).collect();
        let union: std::collections::HashSet<_> = a_words.union(&b_words).collect();
        
        if union.is_empty() {
            return 0.0;
        }
        
        intersection.len() as f32 / union.len() as f32
    }
}

impl Default for MemoryExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to extract memories from a message
pub fn extract_memories(message: &str) -> Vec<ExtractedMemory> {
    let extractor = MemoryExtractor::new();
    
    if !extractor.should_extract(message) {
        return Vec::new();
    }
    
    extractor.extract(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_preference() {
        let extractor = MemoryExtractor::new();
        let memories = extractor.extract("I prefer Python over JavaScript for scripting tasks");
        
        assert!(!memories.is_empty());
        assert!(memories.iter().any(|m| m.content.contains("Python")));
        assert!(memories[0].confidence >= 0.75);
    }

    #[test]
    fn test_extract_identity() {
        let extractor = MemoryExtractor::new();
        let memories = extractor.extract("My name is John and I work as a software engineer");
        
        assert!(!memories.is_empty());
        assert!(memories.iter().any(|m| m.content.contains("John")));
    }

    #[test]
    fn test_extract_constraint() {
        let extractor = MemoryExtractor::new();
        let memories = extractor.extract("The code must be PEP8 compliant and well documented");
        
        assert!(!memories.is_empty());
        assert!(memories.iter().any(|m| m.content.contains("PEP8")));
    }

    #[test]
    fn test_skip_questions() {
        let extractor = MemoryExtractor::new();
        assert!(!extractor.should_extract("What is the weather today?"));
    }

    #[test]
    fn test_skip_greetings() {
        let extractor = MemoryExtractor::new();
        assert!(!extractor.should_extract("Hello there!"));
        assert!(!extractor.should_extract("Hi"));
    }

    #[test]
    fn test_skip_short() {
        let extractor = MemoryExtractor::new();
        assert!(!extractor.should_extract("ok"));
    }

    #[test]
    fn test_deduplication() {
        let extractor = MemoryExtractor::new();
        // Similar messages should be deduplicated
        let msg = "I prefer Python. I really prefer Python for all my work.";
        let memories = extractor.extract(msg);
        
        // Should only have one memory about Python preference
        let python_memories: Vec<_> = memories.iter()
            .filter(|m| m.content.contains("Python"))
            .collect();
        assert_eq!(python_memories.len(), 1);
    }
}