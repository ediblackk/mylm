# Agentic Loop Architecture (ReAct Pattern)

This document outlines the design for implementing an autonomous Agentic Loop in the `mylm` Rust application. The goal is to enable "true interactivity" where the model can reason, execute tools, and iterate before providing a final answer.

## 1. Protocol Specification

We will use a text-based protocol inspired by ReAct (Reasoning + Acting) but enhanced with XML tags for robust parsing by the Rust backend. This avoids reliance on provider-specific "Function Calling" APIs and keeps the stack generic.

### 1.1 System Prompt Structure

The system prompt will inject:
1.  **Role Definition**: "You are an autonomous agent..."
2.  **Tool Definitions**: Dynamically generated list of available tools.
3.  **Loop Protocol**: Strict instructions on how to think and act.

**Template:**

```text
You are mylm, an autonomous terminal assistant.

# TOOLS
You have access to the following tools:

<tool_definitions>
{{TOOLS_XML}}
</tool_definitions>

# PROTOCOL
You must use the ReAct pattern to answer requests. You go through a loop of Thinking, Acting, and Observing.

1. **THOUGHT**: Analyze the user's request or the previous tool output. Decide what to do next.
2. **ACTION**: If you need to use a tool, output a tool call.
3. **OBSERVATION**: The system will execute the tool and give you the result.
4. **FINAL ANSWER**: If you have sufficient information, provide the final response to the user.

# OUTPUT FORMAT

To call a tool, use this XML format:
<tool_code>
<name>tool_name</name>
<parameters>
{
  "arg1": "value1"
}
</parameters>
</tool_code>

To provide a final answer, just write the text normally without tags, or wrap it in <final_answer> if explicitly needed (usually not required if no tool is called).

# CONSTRAINTS
- Only use tools defined in the <tool_definitions> section.
- You can only make ONE tool call per turn.
- Wait for the <observation> from the system before proceeding.
```

### 1.2 Tool Definition Schema

Tools will be presented to the LLM in a structured XML format to ensure it understands the signature.

```xml
<tool>
    <name>execute_command</name>
    <description>Execute a shell command safely. Use this to run system commands.</description>
    <parameters>
        {
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to run" }
            },
            "required": ["command"]
        }
    </parameters>
</tool>
```

### 1.3 Response Format (LLM & System)

**LLM Turn (Thinking & Acting):**
```text
I need to check the files in the current directory to answer the user.

<tool_code>
<name>execute_command</name>
<parameters>
{ "command": "ls -la" }
</parameters>
</tool_code>
```

**System Turn (Observing):**
The Rust system parses the block, executes it, and appends the result as a "User" or "System" message (depending on provider API quirks, usually "User" role with a specific header is safest).

```text
<observation>
total 128
drwxr-xr-x 10 edward edward  4096 Jan 10 10:47 .
drwxr-xr-x  5 edward edward  4096 Jan  1 12:00 ..
...
</observation>
```

---

## 2. Rust Architecture

### 2.1 The `Tool` Trait

We need a generic trait to define tools that can be registered with the agent.

```rust
use async_trait::async_trait;
use serde_json::Value;
use anyhow::Result;

#[async_trait]
pub trait Tool: Send + Sync {
    /// The name of the tool (e.g., "execute_command")
    fn name(&self) -> &str;

    /// A description for the prompt
    fn description(&self) -> &str;

    /// JSON Schema for parameters
    fn parameters(&self) -> Value;

    /// Execute the tool with parsed arguments
    async fn execute(&self, args: Value) -> Result<String>;
}
```

### 2.2 The `Agent` Struct

The Agent manages the `LlmClient`, the list of `Tool`s, and the conversation history during a loop.

```rust
pub struct Agent {
    client: Arc<LlmClient>,
    tools: HashMap<String, Box<dyn Tool>>,
    max_iterations: usize,
}

impl Agent {
    pub fn new(client: Arc<LlmClient>) -> Self { ... }
    
    pub fn register_tool(&mut self, tool: Box<dyn Tool>) { ... }

    /// The main entry point for a user query
    pub async fn run(&self, user_prompt: &str, context: &TerminalContext) -> Receiver<AgentEvent>;
}
```

### 2.3 Agent Events & State Machine

Since the loop takes time and has intermediate steps, we should stream events back to the UI.

```rust
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Thought(String),           // "Thinking about checking files..."
    ToolCall { name: String, args: String },
    ToolOutput(String),        // Result of the tool
    Error(String),
    FinalResponse(String),     // The actual answer
}
```

**Logic Flow (`run` method):**

1.  **Initialize**:
    *   Construct `System Prompt` with tool definitions.
    *   Initialize `messages` history.
2.  **Loop** (up to `max_iterations`):
    *   **Call LLM**: Send current history.
    *   **Parse Response**:
        *   If text contains `<tool_code>`:
            *   Extract `name` and `parameters`.
            *   Emit `AgentEvent::ToolCall`.
            *   **Execute Tool**: Look up tool in `HashMap`.
            *   Emit `AgentEvent::ToolOutput`.
            *   Append `AssistantMessage` (with tool call) and `UserMessage` (with `<observation>output</observation>`) to history.
        *   If no tool call:
            *   Emit `AgentEvent::FinalResponse`.
            *   Break loop.
    *   **Handle Errors**: If tool execution fails, feed the error back as an observation so the agent can retry.
3.  **Fallback**: If max iterations reached, return generic "I tried but couldn't finish" message.

---

## 3. Integration Plan

### 3.1 `src/executor/mod.rs` as a Tool
Wrap the existing `CommandExecutor` in a struct implementing the `Tool` trait.
- **Name**: `execute_command`
- **Args**: `{"command": "string"}`

### 3.2 `src/memory/store.rs` as a Tool
Wrap `VectorStore` in a struct implementing `Tool`.
- **Name**: `search_memory`
- **Args**: `{"query": "string", "limit": "number"}`

### 3.3 UI Updates (`src/terminal/app.rs`)

The current `mpsc::unbounded_channel::<String>` is insufficient for rich status updates.

1.  **Update Channel**: Change `ai_rx` to receive `AgentEvent` (or a wrapper enum that includes simple strings for backward compatibility).
2.  **UI Rendering**:
    *   **Thinking State**: Show a spinner or "MyLM is thinking..."
    *   **Tool Execution**: Show "Executing: `ls -la`..." in a dim color or separate status line.
    *   **Final Answer**: Stream token-by-token (if possible) or display final block.

### 3.4 Event Loop Hook

In `src/terminal/mod.rs`:

```rust
// Instead of simple completion:
// let response = client.complete(&prompt).await?;

// We instantiate the Agent and run the loop:
let agent = Agent::new(client);
agent.register_tool(Box::new(ShellTool::new(...)));
agent.register_tool(Box::new(MemoryTool::new(...)));

let mut stream = agent.run(&prompt, &ctx).await;

while let Some(event) = stream.recv().await {
    match event {
        AgentEvent::Thought(t) => ai_tx.send(format!("💭 {}", t)),
        AgentEvent::ToolCall { name, .. } => ai_tx.send(format!("🛠️ Using {}", name)),
        AgentEvent::FinalResponse(response) => ai_tx.send(response),
        // ... handle others
    }
}
```

## 4. Summary of Work

1.  **Create `src/agent/` module**:
    *   `mod.rs`
    *   `tool.rs` (Trait)
    *   `core.rs` (Agent loop implementation)
    *   `tools/shell.rs`
    *   `tools/memory.rs`
2.  **Update `LlmClient`**: Ensure it can handle the conversation history manipulation required for the loop.
3.  **Refactor `run_tui`**: To use the `Agent` instead of direct `client.complete()`.