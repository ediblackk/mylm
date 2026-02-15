//! Prompt configuration renderer
//!
//! Renders a PromptConfig into a formatted string suitable for LLM system prompts.
//! Handles dynamic section generation, template variable substitution, and tool formatting.

use std::collections::HashMap;

use crate::config::prompt_schema::{PromptConfig, Section};

/// Context for rendering prompts
/// 
/// Provides dynamic data needed for template substitution and section generation
pub struct RenderContext<'a> {
    /// Current working directory
    pub working_directory: Option<String>,
    /// Current datetime string
    pub datetime: Option<String>,
    /// Current git branch
    pub git_branch: Option<String>,
    /// Agent mode
    pub mode: Option<String>,
    /// User instructions
    pub user_instructions: Option<String>,
    /// Scratchpad content
    pub scratchpad: Option<String>,
    /// Available tools for dynamic generation
    pub tools: Option<Vec<ToolInfo>>,
    /// Custom variables
    pub variables: HashMap<String, String>,
    /// Phantom lifetime
    _phantom: std::marker::PhantomData<&'a ()>,
}

/// Information about a tool for dynamic generation
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub usage: String,
    pub category: ToolCategory,
}

/// Tool category for grouping
#[derive(Debug, Clone, PartialEq)]
pub enum ToolCategory {
    Internal,
    Terminal,
    Web,
    Other(String),
}

impl<'a> RenderContext<'a> {
    /// Create a new render context
    pub fn new() -> Self {
        Self {
            working_directory: None,
            datetime: None,
            git_branch: None,
            mode: None,
            user_instructions: None,
            scratchpad: None,
            tools: None,
            variables: HashMap::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set working directory
    pub fn with_working_directory(mut self, dir: impl Into<String>) -> Self {
        self.working_directory = Some(dir.into());
        self
    }

    /// Set datetime
    pub fn with_datetime(mut self, dt: impl Into<String>) -> Self {
        self.datetime = Some(dt.into());
        self
    }

    /// Set git branch
    pub fn with_git_branch(mut self, branch: impl Into<String>) -> Self {
        self.git_branch = Some(branch.into());
        self
    }

    /// Set mode
    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = Some(mode.into());
        self
    }

    /// Set user instructions
    pub fn with_user_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.user_instructions = Some(instructions.into());
        self
    }

    /// Set scratchpad content
    pub fn with_scratchpad(mut self, content: impl Into<String>) -> Self {
        self.scratchpad = Some(content.into());
        self
    }

    /// Set tools
    pub fn with_tools(mut self, tools: Vec<ToolInfo>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Add a custom variable
    pub fn with_variable(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.insert(key.into(), value.into());
        self
    }

    /// Get a variable value by key
    fn get(&self, key: &str) -> Option<String> {
        match key {
            "working_directory" => self.working_directory.clone(),
            "datetime" => self.datetime.clone(),
            "git_branch" => self.git_branch.clone(),
            "mode" => self.mode.clone(),
            "user_instructions" => self.user_instructions.clone(),
            "scratchpad" => self.scratchpad.clone(),
            "tools" => self.tools.as_ref().map(|_| "[DYNAMIC_TOOLS]".to_string()),
            _ => self.variables.get(key).cloned(),
        }
    }
}

impl<'a> Default for RenderContext<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// Renderer for prompt configurations
pub struct PromptRenderer;

impl PromptRenderer {
    /// Render a PromptConfig to a string
    pub fn render(config: &PromptConfig, context: &RenderContext) -> String {
        // If raw_content is set, use it directly
        if let Some(ref raw) = config.raw_content {
            return Self::substitute_variables(raw, context);
        }

        // Sort sections by priority
        let mut sections: Vec<&Section> = config.sections.iter().collect();
        sections.sort_by_key(|s| s.priority.unwrap_or(50));

        // Build output
        let mut parts = Vec::new();

        // Add identity section first
        parts.push(Self::render_identity(&config.identity));

        // Render each section
        for section in sections {
            if Self::should_include_section(section, context) {
                if let Some(content) = Self::render_section(section, context) {
                    if !content.trim().is_empty() {
                        parts.push(content);
                    }
                }
            }
        }

        parts.join("\n\n")
    }

    /// Render identity section
    fn render_identity(identity: &crate::config::prompt_schema::IdentitySection) -> String {
        let mut parts = vec![
            format!("# {}\n\n{}", identity.name, identity.description)
        ];

        if let Some(ref capabilities) = identity.capabilities {
            if !capabilities.is_empty() {
                parts.push(format!(
                    "\nCapabilities: {}",
                    capabilities.join(", ")
                ));
            }
        }

        parts.concat()
    }

    /// Check if a section should be included based on conditions
    fn should_include_section(section: &Section, _context: &RenderContext) -> bool {
        // Check conditions if present
        if let Some(ref conditions) = section.conditions {
            if let Some(ref tools_required) = conditions.tools_required {
                // For now, always include if tools_required is set
                // In a full implementation, we'd check if the tools exist
                if tools_required.is_empty() {
                    return false;
                }
            }
            
            if let Some(min_tools) = conditions.min_tools {
                if min_tools == 0 {
                    return false;
                }
            }
        }

        true
    }

    /// Render a single section
    fn render_section(section: &Section, context: &RenderContext) -> Option<String> {
        // Handle dynamic sections
        if section.dynamic == Some(true) {
            return Self::render_dynamic_section(section, context);
        }

        // Handle static sections
        section.content.as_ref().map(|content| {
            let substituted = Self::substitute_variables(content, context);
            format!("# {}\n\n{}", section.title, substituted)
        })
    }

    /// Render a dynamic section
    fn render_dynamic_section(section: &Section, context: &RenderContext) -> Option<String> {
        let generator = section.generator.as_deref()?;

        match generator {
            "tools" => {
                if let Some(ref tools) = context.tools {
                    Some(Self::format_tools_section(&section.title, tools))
                } else {
                    // No tools available, skip section
                    None
                }
            }
            "scratchpad" => {
                if let Some(ref scratchpad) = context.scratchpad {
                    if !scratchpad.trim().is_empty() {
                        Some(format!(
                            "# {}\n\n{}",
                            section.title,
                            scratchpad
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => {
                // Unknown generator, try content as fallback
                section.content.as_ref().map(|content| {
                    format!("# {}\n\n{}", section.title, content)
                })
            }
        }
    }

    /// Format tools section with grouping by category
    fn format_tools_section(title: &str, tools: &[ToolInfo]) -> String {
        let mut sections = Vec::new();

        // Group tools by category
        let mut internal = Vec::new();
        let mut terminal = Vec::new();
        let mut web = Vec::new();
        let mut other: HashMap<String, Vec<&ToolInfo>> = HashMap::new();

        for tool in tools {
            match tool.category {
                ToolCategory::Internal => internal.push(tool),
                ToolCategory::Terminal => terminal.push(tool),
                ToolCategory::Web => web.push(tool),
                ToolCategory::Other(ref cat) => {
                    other.entry(cat.clone()).or_default().push(tool);
                }
            }
        }

        // Format each group
        if !internal.is_empty() {
            sections.push("## Internal Tools".to_string());
            for tool in internal {
                sections.push(Self::format_tool_entry(tool));
            }
        }

        if !terminal.is_empty() {
            sections.push("## Terminal Tools".to_string());
            for tool in terminal {
                sections.push(Self::format_tool_entry(tool));
            }
        }

        if !web.is_empty() {
            sections.push("## Web Tools".to_string());
            for tool in web {
                sections.push(Self::format_tool_entry(tool));
            }
        }

        // Other categories
        for (cat, cat_tools) in other {
            sections.push(format!("## {} Tools", cat));
            for tool in cat_tools {
                sections.push(Self::format_tool_entry(tool));
            }
        }

        format!("# {}\n\n{}", title, sections.join("\n"))
    }

    /// Format a single tool entry
    fn format_tool_entry(tool: &ToolInfo) -> String {
        format!(
            "- `{}`: {}\n  Usage: {}",
            tool.name,
            tool.description,
            tool.usage
        )
    }

    /// Substitute template variables in content
    /// 
    /// Variables are in the format {variable_name}
    fn substitute_variables(content: &str, context: &RenderContext) -> String {
        let mut result = content.to_string();

        // Find all {variable} patterns
        let re = regex::Regex::new(r"\{(\w+)\}").unwrap();
        
        for cap in re.captures_iter(content) {
            let full_match = cap.get(0).unwrap().as_str();
            let var_name = cap.get(1).unwrap().as_str();

            if let Some(value) = context.get(var_name) {
                result = result.replace(full_match, &value);
            }
            // If variable not found, leave the placeholder as-is
        }

        result
    }

    /// Render just the tools section (for when we need tools separately)
    pub fn render_tools(tools: &[ToolInfo]) -> String {
        Self::format_tools_section("Available Tools", tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> PromptConfig {
        PromptConfig {
            version: "1.0".to_string(),
            identity: crate::config::prompt_schema::IdentitySection {
                name: "Test Agent".to_string(),
                description: "A test agent".to_string(),
                capabilities: None,
            },
            sections: vec![
                Section {
                    id: "test".to_string(),
                    title: "Test Section".to_string(),
                    content: Some("Hello {name}!".to_string()),
                    dynamic: Some(false),
                    generator: None,
                    priority: Some(1),
                    conditions: None,
                },
            ],
            placeholders: None,
            protocols: None,
            variables: None,
            raw_content: None,
        }
    }

    #[test]
    fn test_render_basic() {
        let config = create_test_config();
        let mut context = RenderContext::new();
        context.variables.insert("name".to_string(), "World".to_string());

        let result = PromptRenderer::render(&config, &context);
        
        assert!(result.contains("Test Agent"));
        assert!(result.contains("Hello World!"));
    }

    #[test]
    fn test_substitute_variables() {
        let mut context = RenderContext::new();
        context.variables.insert("name".to_string(), "Alice".to_string());
        context.variables.insert("task".to_string(), "testing".to_string());

        let content = "Hello {name}, your task is {task}. Unknown: {missing}";
        let result = PromptRenderer::substitute_variables(content, &context);

        assert_eq!(result, "Hello Alice, your task is testing. Unknown: {missing}");
    }

    #[test]
    fn test_render_tools_section() {
        let tools = vec![
            ToolInfo {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                usage: "read_file <path>".to_string(),
                category: ToolCategory::Internal,
            },
            ToolInfo {
                name: "shell".to_string(),
                description: "Execute shell command".to_string(),
                usage: "shell <command>".to_string(),
                category: ToolCategory::Terminal,
            },
        ];

        let result = PromptRenderer::render_tools(&tools);

        assert!(result.contains("Internal Tools"));
        assert!(result.contains("Terminal Tools"));
        assert!(result.contains("read_file"));
        assert!(result.contains("shell"));
    }

    #[test]
    fn test_render_context_builder() {
        let context = RenderContext::new()
            .with_working_directory("/home/user")
            .with_mode("test")
            .with_variable("custom", "value");

        assert_eq!(context.working_directory, Some("/home/user".to_string()));
        assert_eq!(context.mode, Some("test".to_string()));
        assert_eq!(context.variables.get("custom"), Some(&"value".to_string()));
    }
}
