//! System Prompt Generation
//!
//! Handles construction of system prompts with tool descriptions,
//! capabilities context, and scratchpad state.

use std::collections::HashMap;
use std::sync::Arc;
use crate::agent_old::tool::Tool;

/// Builder for generating system prompts.
pub struct PromptBuilder {
    system_prompt_prefix: String,
    tools: HashMap<String, Arc<dyn Tool>>,
    capabilities_context: Option<String>,
}

impl PromptBuilder {
    /// Create a new prompt builder.
    pub fn new(
        system_prompt_prefix: String,
        tools: HashMap<String, Arc<dyn Tool>>,
        capabilities_context: Option<String>,
    ) -> Self {
        Self {
            system_prompt_prefix,
            tools,
            capabilities_context,
        }
    }

    /// Format a capabilities description for a list of tools.
    /// This is used by the delegate tool to provide tool awareness to sub-agents.
    pub fn format_capabilities_for_tools(tools: &[Arc<dyn Tool>]) -> String {
        use crate::agent_old::tool::ToolKind;
        
        let mut sections = Vec::new();
        
        // Group tools by kind
        let mut internal_tools = Vec::new();
        let mut terminal_tools = Vec::new();
        let mut web_tools = Vec::new();
        
        for tool in tools {
            match tool.kind() {
                ToolKind::Internal => internal_tools.push(tool),
                ToolKind::Terminal => terminal_tools.push(tool),
                ToolKind::Web => web_tools.push(tool),
            }
        }
        
        // Format each group
        if !internal_tools.is_empty() {
            sections.push("## Internal Tools".to_string());
            for tool in internal_tools {
                sections.push(format!(
                    "- `{}`: {}\n  Usage: {}",
                    tool.name(),
                    tool.description(),
                    tool.usage()
                ));
            }
        }
        
        if !terminal_tools.is_empty() {
            sections.push("## Terminal Tools".to_string());
            for tool in terminal_tools {
                sections.push(format!(
                    "- `{}`: {}\n  Usage: {}",
                    tool.name(),
                    tool.description(),
                    tool.usage()
                ));
            }
        }
        
        if !web_tools.is_empty() {
            sections.push("## Web Tools".to_string());
            for tool in web_tools {
                sections.push(format!(
                    "- `{}`: {}\n  Usage: {}",
                    tool.name(),
                    tool.description(),
                    tool.usage()
                ));
            }
        }
        
        sections.push("\n## Tool Usage Details".to_string());
        for tool in tools {
            sections.push(format!(
                "- `{}`: {}\n  Usage: {}",
                tool.name(),
                tool.description(),
                tool.usage()
            ));
        }
        
        sections.join("\n")
    }

    /// Generate the system prompt with available tools and Short-Key JSON instructions.
    /// CRITICAL: Response Format comes FIRST before tools so model knows it MUST use JSON.
    pub fn build(&self, scratchpad_content: &str) -> String {
        // If capabilities_context is set, use it instead of regenerating tools description
        // to avoid duplication. This is important for workers that receive pre-formatted capabilities.
        let (tools_section, capabilities_section) = if self.capabilities_context.is_some() {
            // Use capabilities_context as the tools section, don't duplicate
            (String::new(), self.format_capabilities_section())
        } else {
            // No capabilities context, generate tools description normally
            (self.format_tools_description(), String::new())
        };
        
        let scratchpad_section = self.format_scratchpad_section(scratchpad_content);

        format!(
            "{}\n\
            \n\
            # Response Format: SHORT-KEY JSON (MANDATORY - READ THIS FIRST)\n\
            CRITICAL: You MUST respond ONLY with JSON. NO conversational text. NO markdown outside JSON.\n\n\
            ## Short-Key Fields\n\
            - `t`: Thought/reasoning (optional)\n\
            - `f`: Final answer to user - chat only, no action (use this to chat!)\n\
            - `a`: Action/tool name to execute (use this to call tools!)\n\
            - `i`: Input arguments for the action (optional)\n\
            - `c`: Confirm flag - chat first, wait for approval (optional, default false)\n\n\
            ## Rules\n\
            1. ALL output MUST be valid JSON\n\
            2. To chat only: {{\"f\": \"your message\"}}\n\
            3. To act immediately: {{\"t\": \"reasoning\", \"a\": \"tool_name\", \"i\": {{args}}}}\n\
            4. To chat first, act after approval: {{\"t\": \"reasoning\", \"c\": true, \"a\": \"tool_name\", \"i\": {{args}}}}\n\
            5. NEVER say \"I'll\" or \"Let me\" outside JSON - use {{\"f\": \"I'll help\"}} instead\n\n\
            ## Examples\n\
            Chat only (no tool):\n\
            ```json\n\
            {{\"f\": \"Hello! How can I help?\"}}\n\
            ```\n\
            Act immediately:\n\
            ```json\n\
            {{\"t\": \"User wants files\", \"a\": \"list_files\", \"i\": {{\"path\": \".\"}}}}\n\
            ```\n\
            Chat first, confirm before acting (ReAct style):\n\
            ```json\n\
            {{\"t\": \"I can spawn 3 workers to help. Proceed?\", \"c\": true, \"a\": \"delegate\", \"i\": {{\"objective\": \"test\", \"workers\": 3}}}}\n\
            ```\n\n\
            ## WRONG vs CORRECT\n\
            WRONG: I'll help you with that.\n\
            WRONG: Let me check the files.\n\
            CORRECT: {{\"f\": \"I'll help you with that.\"}}\n\
            CORRECT: {{\"t\": \"Checking files\", \"a\": \"list_files\", \"i\": {{}}}}\n\
            CORRECT: {{\"t\": \"Should I check the files?\", \"c\": true, \"a\": \"list_files\", \"i\": {{}}}}\n\n\
            # Available Tools\n\
            {}\
{}\
{}\
            Begin!{}",
            self.system_prompt_prefix,
            tools_section,
            self.format_delegation_section(),
            scratchpad_section,
            capabilities_section
        )
    }

    /// Get the tools description section for debugging.
    pub fn get_tools_description(&self) -> String {
        self.format_tools_description()
    }

    /// Get the full system prompt for debugging purposes.
    pub fn get_full_prompt(&self, scratchpad_content: &str) -> String {
        self.build(scratchpad_content)
    }

    /// Format the tools description section.
    fn format_tools_description(&self) -> String {
        let mut tools_desc = String::new();
        for tool in self.tools.values() {
            tools_desc.push_str(&format!(
                "- {}: {}\n  Usage: {}\n",
                tool.name(),
                tool.description(),
                tool.usage()
            ));
        }
        tools_desc
    }

    /// Format the scratchpad section.
    fn format_scratchpad_section(&self, content: &str) -> String {
        if content.trim().is_empty() {
            String::new()
        } else {
            format!(
                "\n\
                # Scratchpad\n\
                {}\n",
                content
            )
        }
    }

    /// Format the capabilities section (when using capabilities_context).
    fn format_capabilities_section(&self) -> String {
        self.capabilities_context.clone().unwrap_or_default()
    }

    /// Format the delegation section explaining parallel worker usage.
    fn format_delegation_section(&self) -> String {
        // Check if delegate tool is available
        let has_delegate = self.tools.contains_key("delegate");
        if !has_delegate {
            return String::new();
        }

        r#"

## Parallel Worker Delegation

Use the `delegate` tool to spawn differentiated workers with shared scratchpad coordination.

### Example: Refactoring with Multiple Workers

```json
{
  "a": "delegate",
  "i": {
    "shared_context": "Refactoring auth module",
    "workers": [
      {
        "id": "models",
        "objective": "Update User and Session models",
        "tools": ["read_file", "write_file"],
        "allowed_commands": ["cargo check --lib"],
        "tags": ["models"]
      },
      {
        "id": "handlers",
        "objective": "Update login/logout handlers",
        "tools": ["read_file", "write_file"],
        "tags": ["handlers"],
        "depends_on": ["models"]
      }
    ]
  }
}
```

### Worker Configuration

| Field | Description | Example |
|-------|-------------|---------|
| `id` | Unique worker identifier | `"models"` |
| `objective` | Specific task | `"Update User model"` |
| `instructions` | Additional system prompt | `"Focus on error handling"` |
| `tools` | Allowed tools (subset) | `["read_file", "write_file"]` |
| `allowed_commands` | Auto-approved patterns | `["cargo check *"]` |
| `forbidden_commands` | Blocked patterns | `["rm -rf *"]` |
| `tags` | Scratchpad coordination tags | `["models"]` |
| `depends_on` | Wait for workers | `["models"]` |
| `context` | Worker-specific data | `{ "file": "src/models.rs" }` |

### Coordination Protocol

Workers share a scratchpad and use these messages:
- **CLAIM**: Before working on a file
- **PROGRESS**: After milestones
- **COMPLETE**: When finished

Monitor: `{"a": "scratchpad", "i": {"action": "list"}}`

Monitor coordination:
```json
{"a": "scratchpad", "i": {"action": "list"}}
```

### Coordination Best Practices

1. **Assign distinct scopes**: Each worker should work on non-overlapping files/modules
2. **Use meaningful IDs**: `"auth-models"` is better than `"worker-1"`
3. **CLAIM before working**: Workers automatically write CLAIM entries to scratchpad
4. **Check scratchpad**: Monitor `list_jobs` and `scratchpad` for progress
5. **Set dependencies**: Use `depends_on` to sequence work (e.g., tests after implementation)

### Monitoring Workers

Check worker status:
```json
{"t": "Checking worker progress", "a": "list_jobs", "i": {}}
```

View coordination scratchpad:
```json
{"t": "Viewing coordination updates", "a": "scratchpad", "i": {"action": "list"}}
```

Filter by worker tag:
```json
{"t": "Checking models worker progress", "a": "scratchpad", "i": {"action": "list", "tags": ["models"]}}
```
"#.to_string()
    }
}
