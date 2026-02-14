/// Prompt configuration schema definitions.
///
/// This module defines the data structures for configuring prompt templates
/// and their associated metadata, sections, and protocols. These structs enable
/// data-driven prompt composition with conditional logic, dynamic generation,
/// and protocol-specific formatting.
///
/// The schema supports:
/// - Identity configuration for agent persona
/// - Structured prompt sections with priorities and conditions
/// - Protocol-specific formatting (React, JSON keys)
/// - Placeholder substitution and variable management
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Top-level prompt configuration container.
///
/// This struct represents the complete prompt configuration, including
/// identity information, content sections, and protocol specifications.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PromptConfig {
    /// Schema version for compatibility tracking
    pub version: String,
    /// Agent identity and capabilities configuration
    pub identity: IdentitySection,
    /// Ordered list of prompt sections to include
    pub sections: Vec<Section>,
    /// Optional placeholder values for template substitution
    pub placeholders: Option<HashMap<String, String>>,
    /// Protocol-specific formatting rules
    pub protocols: Option<Protocols>,
    /// Global variables accessible throughout the prompt
    pub variables: Option<HashMap<String, String>>,
    /// Raw content for backward compatibility with .md files
    /// When set, this content is used directly without rendering
    #[serde(default)]
    pub raw_content: Option<String>,
}

/// Agent identity configuration.
///
/// Defines the persona, description, and optional capabilities list
/// that characterize the agent's identity in the system prompt.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IdentitySection {
    /// Agent name (e.g., "MYLM", "Worker Agent")
    pub name: String,
    /// Detailed description of the agent's role and purpose
    pub description: String,
    /// Optional list of capability strings for dynamic generation
    pub capabilities: Option<Vec<String>>,
}

/// Individual prompt section definition.
///
/// Represents a modular component of the system prompt that can be
/// conditionally included, dynamically generated, or prioritized.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Section {
    /// Unique identifier for this section
    pub id: String,
    /// Display title for the section
    pub title: String,
    /// Static content template (if not dynamic)
    pub content: Option<String>,
    /// Whether this section should be dynamically generated
    pub dynamic: Option<bool>,
    /// Generator type for dynamic sections: "tools" or "scratchpad"
    pub generator: Option<String>,
    /// Priority order for section placement (lower = higher priority)
    pub priority: Option<u32>,
    /// Conditions that must be met for section inclusion
    pub conditions: Option<SectionConditions>,
}

/// Conditions for conditional section inclusion.
///
/// Defines requirements that must be satisfied for a section to be
/// included in the final prompt.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SectionConditions {
    /// Tool names that must be available for section to appear
    pub tools_required: Option<Vec<String>>,
    /// Minimum number of tools required (alternative to specific list)
    pub min_tools: Option<usize>,
}

/// Protocol configuration for different output formats.
///
/// Specifies formatting rules for various communication protocols
/// used by the agent (e.g., ReAct, JSON structured output).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Protocols {
    /// React/ReAct protocol configuration
    pub react: Option<ReactProtocol>,
    /// Custom JSON key names for structured output
    pub json_keys: Option<JsonKeys>,
}

/// ReAct (Reason + Act) protocol specification.
///
/// Defines the structured format for the ReAct reasoning loop,
/// including key names, formatting rules, and examples.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReactProtocol {
    /// Format specification (e.g., "json", "markdown")
    pub format: String,
    /// Ordered list of required keys in the output
    pub keys: Vec<String>,
    /// Optional formatting rules or constraints
    pub rules: Option<Vec<String>>,
    /// Example output demonstrating the protocol
    pub example: Option<String>,
}

/// Custom JSON key mapping for structured output protocols.
///
/// Allows customization of the standard key names used in JSON-formatted
/// agent responses. All fields are optional; defaults are used for unspecified keys.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsonKeys {
    /// Custom key for thought/reasoning field (default: "thought" or "t")
    pub thought: Option<String>,
    /// Custom key for action/tool call field (default: "action" or "a")
    pub action: Option<String>,
    /// Custom key for input/arguments field (default: "input" or "i")
    pub input: Option<String>,
    /// Custom key for final answer field (default: "final" or "f")
    pub final_answer: Option<String>,
}
