# Architecture Rework: Visible Terminal Execution

## Objective
Decouple the Agent's "Thinking" phase from the "Acting" phase to allow terminal commands to be executed visibly in the PTY pane, driven by the Main Loop. This ensures the user sees the action happening in real-time and allows for better UI state management (Pause/Resume).

## Current Problem
The `Agent::run` method contains a loop that internally calls `tool.call()`. For `ShellTool`, this uses a channel to request execution, but the Agent task remains blocked and "hidden" until the result returns. The architecture couples the decision to act with the execution of the act.

## Proposed Solution
Refactor the Agent to be a State Machine (or Step-based) that yields control back to the application logic when an action is required.

### 1. Agent Logic Refactoring (`src/agent/core.rs`)

Transform the `Agent` from a recursive/looping runner into a step-based processor.

- **Remove**: `Agent::run()` loop.
- **Add**: `Agent::step(&mut history) -> Result<AgentDecision>`
- **Define `AgentDecision` Enum**:
    ```rust
    pub enum AgentDecision {
        /// The LLM produced a text response (final answer or question)
        Message(String),
        /// The LLM wants to execute a tool
        Action {
            tool: String,
            args: String,
            // Logic to determine if this action requires visible execution
            kind: ToolKind, 
        },
        /// The LLM is waiting for an observation (likely not needed as return type, but implicit in state)
        Error(String),
    }
    ```

### 2. Tool Trait Enhancement (`src/agent/tool.rs`)

Update the `Tool` trait to categorize tools, allowing the Agent to decide whether to execute internally or yield an Action.

- **Add**: `kind(&self) -> ToolKind` method.
- **Define `ToolKind`**:
    ```rust
    pub enum ToolKind {
        /// Execute silently/internally (e.g., Memory, WebSearch)
        Internal,
        /// Execute visibly in Terminal (e.g., Shell)
        Terminal,
    }
    ```
- **ShellTool**: Implement `kind()` returning `ToolKind::Terminal`.

### 3. Application Orchestration (`src/terminal/app.rs` & `mod.rs`)

Move the "Agent Loop" out of `core.rs` and into a controlled task in `app.rs` (or a new `src/agent/driver.rs`).

**New Flow (The "Agent Driver"):**
1.  **Start Task**: `App` spawns the Driver Task when user submits a message.
2.  **Step 1: Think**: Call `agent.step(history)`.
3.  **Step 2: Handle Decision**:
    *   **Case `Message`**: 
        *   Send `TuiEvent::AgentResponse`.
        *   Break loop (Turn complete).
    *   **Case `Action (Internal)`**:
        *   Send `TuiEvent::Status("Executing Internal Tool...")`.
        *   Execute `tool.call()` immediately.
        *   Append `Observation` to history.
        *   Loop to Step 1.
    *   **Case `Action (Terminal)`**:
        *   Send `TuiEvent::Status("Executing in Terminal...")`.
        *   **PAUSE AGENT**: The Driver Task sends `TuiEvent::ExecuteTerminalCommand` and *waits*.
        *   **MAIN LOOP**: Receives event, writes to PTY.
        *   **PTY**: Displays output to user.
        *   **CAPTURE**: Main loop buffers output and sends back to Driver Task via channel (as currently done).
        *   **RESUME AGENT**: Driver Task receives output.
        *   Append `Observation` to history.
        *   Loop to Step 1.

### 4. File Changes

#### `src/agent/tool.rs`
- Add `ToolKind` enum.
- Add `fn kind(&self) -> ToolKind` to `Tool` trait.

#### `src/agent/core.rs`
- Rewrite `Agent::run` to `Agent::step`.
- Remove internal tool execution loop for `Terminal` tools.
- Return `AgentDecision` instead of `(String, TokenUsage)`.

#### `src/terminal/app.rs`
- Update `submit_message` to spawn the new Driver Loop instead of calling `agent.run`.
- The Driver Loop will handle the iteration and orchestration.

#### `src/terminal/mod.rs`
- Ensure `run_loop` handles the `ExecuteTerminalCommand` correctly (already mostly there, but verify flow).

## Benefits
- **Visibility**: Terminal commands run in the main PTY, visibly.
- **Control**: The Main Loop controls the pace. We can easily add "Pause/Ask for Approval" steps for *any* tool, not just Shell.
- **Extensibility**: Easier to add other "Interactive" tools (e.g., GUI triggers) in the future.
