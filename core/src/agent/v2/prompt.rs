//! System Prompt Generation
//!
//! Handles construction of system prompts with tool descriptions,
//! capabilities context, and scratchpad state.

use std::collections::HashMap;
use std::sync::Arc;
use crate::agent::tool::Tool;

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

    /// Generate the system prompt with available tools and Short-Key JSON instructions.
    pub fn build(&self, scratchpad_content: &str) -> String {
        let tools_desc = self.format_tools_description();
        let capabilities_section = self.format_capabilities_section();
        let scratchpad_section = self.format_scratchpad_section(scratchpad_content);

        format!(
            "{}\n\n\
            # Available Tools\n\
            {}\n{}\n\
            # Response Format: Short-Key JSON Protocol\n\
            You MUST respond using the Short-Key JSON protocol. This format minimizes token usage and ensures structural integrity.\n\n\
            ## Schema\n\
            - `t`: Thought. Your internal reasoning and next steps (optional; may be omitted for a direct final answer).\n\
            - `a`: Action. The name of the tool to execute (optional if providing final answer).\n\
            - `i`: Input. The arguments for the tool in strict JSON format (optional).\n\
            - `f`: Final Answer. Your final response to the user (optional).\n\n\
            ## Examples\n\
            ### Single Tool Call\n\
            ```json\n\
            {{\"t\": \"I need to list files in the current directory.\", \"a\": \"execute_command\", \"i\": \"ls\"}}\n\
            ```\n\n\
            ### Parallel Tool Calls\n\
            You can execute multiple tools in parallel by returning an array of objects. Use this for independent operations like reading multiple files or searching different sources.\n\
            ```json\n\
            [\n\
              {{\"t\": \"Checking config...\", \"a\": \"execute_command\", \"i\": \"cat config.json\"}},\n\
              {{\"t\": \"Checking logs...\", \"a\": \"execute_command\", \"i\": \"tail -n 20 error.log\"}}\n\
            ]\n\
            ```\n\n\
            ### Final Answer\n\
            ```json\n\
            {{\"f\": \"The project has been successfully initialized.\"}}\n\
            ```\n\n\
            IMPORTANT: Always wrap your JSON in a code block or return it as the raw response. Ensure all tool inputs are valid JSON objects.\n\n\
            Begin!{}",
            self.system_prompt_prefix,
            tools_desc,
            scratchpad_section,
            capabilities_section
        )
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

    /// Format the capabilities context section.
    fn format_capabilities_section(&self) -> String {
        self.capabilities_context
            .as_ref()
            .map(|ctx| format!("\n\n{}\n", ctx))
            .unwrap_or_default()
    }

    /// Format the scratchpad section if content exists.
    fn format_scratchpad_section(&self, scratchpad_content: &str) -> String {
        if !scratchpad_content.is_empty() {
            format!(
                "\n\n## CURRENT SCRATCHPAD (Working Memory)\n{}\n",
                scratchpad_content
            )
        } else {
            String::new()
        }
    }
}
