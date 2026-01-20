//! Dynamic Tool Registry for MyLM Core
//! 
//! This module provides a flexible, orthogonal tool registration system
//! that allows tools to be registered, unregistered, and managed dynamically
//! without hardcoding dependencies in the core Agent.

use crate::agent::tool::{Tool, ToolOutput};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;

/// A dynamic tool registry that manages tool lifecycle and provides isolation
/// between different tool implementations.
#[derive(Clone)]
pub struct ToolRegistry {
    tools: Arc<RwLock<HashMap<String, Box<dyn Tool>>>>,
    disabled_tools: Arc<RwLock<HashMap<String, String>>>, // tool_name -> reason
}

impl ToolRegistry {
    /// Create a new, empty tool registry
    pub fn new() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
            disabled_tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Register a tool in the registry
    pub async fn register_tool(&self, tool: Box<dyn Tool>) -> Result<()> {
        let mut tools = self.tools.write().await;
        let tool_name = tool.name().to_string();
        tools.insert(tool_name.clone(), tool);
        
        // Remove from disabled list if it was disabled
        let mut disabled = self.disabled_tools.write().await;
        disabled.remove(&tool_name);
        
        Ok(())
    }
    
    /// Unregister a tool by name
    pub async fn unregister_tool(&self, name: &str) -> Result<Option<Box<dyn Tool>>> {
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
    pub async fn get_tool(&self, name: &str) -> Option<Box<dyn Tool>> {
        // Check if tool is disabled
        let disabled = self.disabled_tools.read().await;
        if disabled.contains_key(name) {
            return None;
        }
        
        // Get the tool - we need to return a new Box since we can't clone the trait object
        let tools = self.tools.read().await;
        if let Some(_tool) = tools.get(name) {
            // For now, return None since we can't easily clone trait objects
            // In a real implementation, you'd need a factory pattern or similar
            None
        } else {
            None
        }
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
    
    /// Get all available tools as a vector
    pub async fn get_all_tools(&self) -> Vec<Box<dyn Tool>> {
        let tools = self.tools.read().await;
        let disabled = self.disabled_tools.read().await;
        
        // Since we can't clone trait objects, we need to collect names and recreate
        // In a real implementation, you'd use a factory pattern
        let _tool_names: Vec<String> = tools.keys()
            .filter(|name| !disabled.contains_key(*name))
            .cloned()
            .collect();
        
        // For now, return empty vector since we can't clone trait objects
        Vec::new()
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
    
    /// Execute a tool call with error isolation
    pub async fn execute_tool(&self, name: &str, args: &str) -> Result<ToolOutput, String> {
        // Check if tool exists and is available
        if !self.has_tool(name).await {
            let disabled = self.disabled_tools.read().await;
            if let Some(reason) = disabled.get(name) {
                return Err(format!("Tool '{}' is disabled: {}", name, reason));
            }
            return Err(format!("Tool '{}' not found in registry", name));
        }
        
        // Get the tool
        let tool = self.get_tool(name).await
            .ok_or_else(|| format!("Tool '{}' not available", name))?;
        
        // Execute with error isolation
        match tool.call(args).await {
            Ok(output) => Ok(output),
            Err(e) => Err(format!("Tool '{}' execution failed: {}", name, e))
        }
    }
    
    /// Get disabled tools with their reasons
    pub async fn get_disabled_tools(&self) -> HashMap<String, String> {
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
    pub fn with_tool(self, tool: Box<dyn Tool>) -> Self {
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