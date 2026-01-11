# Implementation Plan: Suggest-and-Pause & Thoughts Rendering

This plan outlines the changes required to implement a "Suggest-and-Pause" flow for shell commands and improve the rendering of agent thoughts in the TUI.

## 1. Architecture Changes

### 1.1 `TuiEvent` Extension
We will add a new event to the `TuiEvent` enum in `src/terminal/app.rs` to signal a command suggestion from the agent.
```rust
pub enum TuiEvent {
    // ... existing variants
    SuggestCommand(String),
}
```

### 1.2 `App` State
We will add a state variable to `App` struct in `src/terminal/app.rs` to control the visibility of thought chains.
```rust
pub struct App {
    // ... existing fields
    pub show_thoughts: bool, // Default: true
}
```

### 1.3 Agent Interception
The `Agent::run` loop in `src/agent/core.rs` will be modified to detect when the LLM wants to call the `execute_command` tool (ShellTool). Instead of executing it immediately:
1.  It will emit `TuiEvent::SuggestCommand(cmd)`.
2.  It will return early, pausing the agent's execution.

### 1.4 Slash Command `/exec`
We will introduce a new slash command `/exec <cmd>` in `App::handle_slash_command`. This command will:
1.  Execute the shell command (using the same logic as `ShellTool`).
2.  Add the result to the chat history.
3.  Trigger the agent to continue processing.

## 2. Implementation Steps

### Step 1: Update `App` and `TuiEvent`
*   **File**: `src/terminal/app.rs`
*   **Action**:
    *   Add `SuggestCommand(String)` to `TuiEvent`.
    *   Add `show_thoughts: bool` to `App` struct.
    *   Initialize `show_thoughts` to `true` in `App::new`.

### Step 2: Implement `/exec` Slash Command
*   **File**: `src/terminal/app.rs`
*   **Action**:
    *   In `handle_slash_command`, add a case for `/exec`.
    *   The `/exec` logic should:
        *   Extract the command string.
        *   Spawn a task to execute it (using `agent.lock().await.tools.get("execute_command").call()`).
        *   Add the output to history.
        *   Trigger `agent.run()`.

### Step 3: Modify Agent Logic
*   **File**: `src/agent/core.rs`
*   **Action**:
    *   In the `run` loop, inside the `if let (Some(tool_name), Some(args))` block:
        *   Check if `tool_name` is `"execute_command"`.
        *   If so, send `TuiEvent::SuggestCommand(args)`.
        *   Return `Ok(("", TokenUsage::default()))` to stop the loop.

### Step 4: Handle Events and Input
*   **File**: `src/terminal/mod.rs`
*   **Action**:
    *   In `run_loop`:
        *   Handle `TuiEvent::SuggestCommand(cmd)`:
            *   Set `app.chat_input` to `/exec <cmd>`.
            *   Set `app.cursor_position` to end of string.
            *   Set `app.focus` to `Focus::Chat`.
            *   Set `app.state` to `AppState::Idle`.
    *   In `TuiEvent::Input`:
        *   Add handling for `Ctrl+t` to toggle `app.show_thoughts`.

### Step 5: Update Rendering
*   **File**: `src/terminal/ui.rs`
*   **Action**:
    *   In `render_chat`:
        *   Remove the logic that completely hides `Thought:` lines in non-verbose mode.
        *   Instead, parse messages line-by-line.
        *   If a line starts with `Thought:`:
            *   If `app.show_thoughts` is true: Render it with `Dim` + `Italic` style.
            *   If `app.show_thoughts` is false: Render a placeholder `(Thinking...)` or skip it (based on preference, "Hide" usually means skip). Let's skip it if hidden, or show a collapsed indicator. The requirement suggests "Show/Hide toggle", so skipping when hidden is fine.

## 3. Verification Plan
1.  **Test Thoughts Toggle**:
    *   Run TUI.
    *   Ask a question that triggers chain-of-thought.
    *   Press `Ctrl+t`. Verify thoughts appear/disappear.
2.  **Test Suggest-and-Pause**:
    *   Ask "List files in current directory".
    *   Verify Agent does *not* execute immediately.
    *   Verify Input is populated with `/exec ls -la`.
    *   Press Enter.
    *   Verify command runs and Agent continues (summarizing the output).
