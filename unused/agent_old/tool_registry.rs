use tokio::sync::RwLock;
use anyhow::Result;

/// Normalize a tool name to handle common model hallucinations.
///
/// Models often output tool names like `functions.wait` or `tools.web_search`
/// instead of the bare name. This helper strips common prefixes and normalizes
/// the string.
///
/// # Rules
/// 1. Trim whitespace
/// 2. If it contains '.', take the last segment (e.g. `functions.wait` -> `wait`)
/// 3. Return the result
pub fn normalize_tool_name(name: &str) -> &str {
    let trimmed = name.trim();
    if let Some(idx) = trimmed.rfind('.') {
        &trimmed[idx + 1..]
    } else {
        trimmed
    }
}

/// A dynamic tool registry that manages tool lifecycle and provides isolation
/// between different tool implementations.
#[derive(Clone)]
pub struct ToolRegistry {
    tools: std::sync::Arc<RwLock<std::collections::HashMap<String, std::sync::Arc<dyn crate::agent_old::tool::Tool>>>>,
    disabled_tools: std::sync::Arc<RwLock<std::collections::HashMap<String, String>>>, // tool_name -> reason
}

impl ToolRegistry {
    /// Create a new, empty tool registry
    pub fn new() -> Self {
        Self {
            tools: std::sync::Arc::new(RwLock::new(std::collections::HashMap::new())),
            disabled_tools: std::sync::Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Get a tool reference for generating definitions (non-async, returns Arc)
    pub async fn get_tool_arc(&self, name: &str) -> Option<std::sync::Arc<dyn crate::agent_old::tool::Tool>> {
        // Check if tool is disabled
        let disabled = self.disabled_tools.read().await;
        if disabled.contains_key(name) {
            return None;
        }
        drop(disabled);
        
        // Get the tool Arc
        let tools = self.tools.read().await;
        tools.get(name).cloned()
    }
    
    /// Register a tool in the registry
    pub async fn register_tool(&self, tool: Box<dyn crate::agent_old::tool::Tool>) -> Result<()> {
        let mut tools = self.tools.write().await;
        let tool_name = tool.name().to_string();
        // Convert Box to Arc for storage
        tools.insert(tool_name.clone(), std::sync::Arc::from(tool));
        
        // Remove from disabled list if it was disabled
        let mut disabled = self.disabled_tools.write().await;
        disabled.remove(&tool_name);
        
        Ok(())
    }
    
    /// Register a tool from an Arc (avoids unnecessary conversion)
    pub async fn register_tool_arc(&self, tool: std::sync::Arc<dyn crate::agent_old::tool::Tool>) -> Result<()> {
        let mut tools = self.tools.write().await;
        let tool_name = tool.name().to_string();
        tools.insert(tool_name.clone(), tool);
        
        // Remove from disabled list if it was disabled
        let mut disabled = self.disabled_tools.write().await;
        disabled.remove(&tool_name);
        
        Ok(())
    }
    
    /// Unregister a tool by name
    pub async fn unregister_tool(&self, name: &str) -> Result<Option<std::sync::Arc<dyn crate::agent_old::tool::Tool>>> {
        let mut tools = self.tools.write().await;
        Ok(tools.remove(name))
    }
    
    /// Disable a tool (prevent it from being used) with a reason
    pub async fn disable_tool(&self, name: &str, reason: String) -> Result<()> {
        let mut disabled = self.disabled_tools.write().await;
        disabled.insert(name.to_string(), reason);
        Ok(())
    }
    
    /// Enable a previously disabled tool
    pub async fn enable_tool(&self, name: &str) -> Result<()> {
        let mut disabled = self.disabled_tools.write().await;
        disabled.remove(name);
        Ok(())
    }
    
    /// Get a tool by name if it exists and is not disabled
    /// 
    /// NOTE: This method returns an Arc<dyn Tool> for cheap cloning.
    /// Use this instead of get_tool() which was designed for Box<dyn Tool>.
    pub async fn get_tool(&self, name: &str) -> Option<std::sync::Arc<dyn crate::agent_old::tool::Tool>> {
        // Check if tool is disabled
        let disabled = self.disabled_tools.read().await;
        if disabled.contains_key(name) {
            return None;
        }
        drop(disabled);
        
        // Get the tool Arc
        let tools = self.tools.read().await;
        tools.get(name).cloned()
    }
    
    /// Check if a tool exists and is available
    pub async fn has_tool(&self, name: &str) -> bool {
        let disabled = self.disabled_tools.read().await;
        if disabled.contains_key(name) {
            return false;
        }
        
        let tools = self.tools.read().await;
        tools.contains_key(name)
    }
    
    /// Get all available tool names
    pub async fn get_tool_names(&self) -> Vec<String> {
        let tools = self.tools.read().await;
        let disabled = self.disabled_tools.read().await;
        
        tools.keys()
            .filter(|name| !disabled.contains_key(*name))
            .cloned()
            .collect()
    }
    
    /// Get all available tools as a vector of Arc references
    pub async fn get_all_tools(&self) -> Vec<std::sync::Arc<dyn crate::agent_old::tool::Tool>> {
        let tools = self.tools.read().await;
        let disabled = self.disabled_tools.read().await;
        
        tools.values()
            .filter(|tool| !disabled.contains_key(tool.name()))
            .cloned()
            .collect()
    }
    
    /// Get tool definitions for LLM context
    pub async fn get_tool_definitions(&self) -> Vec<crate::llm::chat::ChatTool> {
        let tools = self.get_all_tools().await;
        let mut definitions = Vec::new();
        
        for tool in tools {
            definitions.push(crate::llm::chat::ChatTool {
                type_: "function".to_string(),
                function: crate::llm::chat::ChatFunction {
                    name: tool.name().to_string(),
                    description: Some(tool.description().to_string()),
                    parameters: Some(tool.parameters()),
                },
            });
        }
        
        definitions
    }
    
    /// Get tool kind for a specific tool (used by Agent::step)
    pub async fn get_tool_kind(&self, name: &str) -> Option<crate::agent_old::tool::ToolKind> {
        let disabled = self.disabled_tools.read().await;
        if disabled.contains_key(name) {
            return None;
        }
        drop(disabled);
        
        let tools = self.tools.read().await;
        tools.get(name).map(|t| t.kind())
    }
    
    /// Execute a tool call with error isolation
    pub async fn execute_tool(&self, name: &str, args: &str) -> Result<crate::agent_old::tool::ToolOutput, String> {
        // Check if tool exists and is available
        let disabled = self.disabled_tools.read().await;
        if let Some(reason) = disabled.get(name) {
            return Err(format!("Tool '{}' is disabled: {}", name, reason));
        }
        drop(disabled);
        
        // Get the tool Arc
        let tools = self.tools.read().await;
        let tool = tools.get(name)
            .cloned()
            .ok_or_else(|| format!("Tool '{}' not found in registry", name))?;
        drop(tools);
        
        // Execute with error isolation
        match tool.call(args).await {
            Ok(output) => Ok(output),
            Err(e) => Err(format!("Tool '{}' execution failed: {}", name, e))
        }
    }
    
    /// Get disabled tools with their reasons
    pub async fn get_disabled_tools(&self) -> std::collections::HashMap<String, String> {
        let disabled = self.disabled_tools.read().await;
        disabled.clone()
    }
    
    /// Get registry statistics
    pub async fn get_stats(&self) -> ToolRegistryStats {
        let tools = self.tools.read().await;
        let disabled = self.disabled_tools.read().await;
        
        ToolRegistryStats {
            total_tools: tools.len(),
            enabled_tools: tools.len() - disabled.len(),
            disabled_tools: disabled.len(),
            tool_names: tools.keys().cloned().collect(),
            disabled_tool_names: disabled.keys().cloned().collect(),
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the tool registry
#[derive(Debug, Clone)]
pub struct ToolRegistryStats {
    pub total_tools: usize,
    pub enabled_tools: usize,
    pub disabled_tools: usize,
    pub tool_names: Vec<String>,
    pub disabled_tool_names: Vec<String>,
}

/// Builder for creating tool registries with common tool sets
pub struct ToolRegistryBuilder {
    registry: ToolRegistry,
}

impl ToolRegistryBuilder {
    pub fn new() -> Self {
        Self {
            registry: ToolRegistry::new(),
        }
    }
    
    /// Build the registry
    pub fn build(self) -> ToolRegistry {
        self.registry
    }
    
    /// Add a tool to the registry
    pub fn with_tool(self, tool: Box<dyn crate::agent_old::tool::Tool>) -> Self {
        // This is a synchronous version for builder pattern
        // In real usage, you'd need to handle the async nature
        // For now, we'll use a blocking approach in the builder
        let rt = tokio::runtime::Handle::current();
        let _ = rt.block_on(self.registry.register_tool(tool));
        self
    }
}

impl Default for ToolRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tool_name() {
        // Basic trim
        assert_eq!(normalize_tool_name("  wait  "), "wait");
        assert_eq!(normalize_tool_name("web_search"), "web_search");
        
        // Strip prefixes
        assert_eq!(normalize_tool_name("functions.wait"), "wait");
        assert_eq!(normalize_tool_name("tools.web_search"), "web_search");
        assert_eq!(normalize_tool_name("a.b.c.d"), "d");
        
        // Edge cases
        assert_eq!(normalize_tool_name(""), "");
        assert_eq!(normalize_tool_name("."), "");
        assert_eq!(normalize_tool_name(" . "), "");
        assert_eq!(normalize_tool_name("no_prefix"), "no_prefix");
    }
}
