# Architecture Explanation & Deep Dive

## 1. Executive Summary

This document addresses key architectural behaviors and UX concerns identified during the usage of `mylm`. Specifically, it explains why the Agent repeats command output (Redundancy), why the terminal scrolling is currently non-functional, and how the Agent "perceives" the terminal environment.

**Key Takeaways:**
*   **Redundancy**: The Agent repeats output because it receives the command result as a text "Observation" to verify success. It then naturally includes this in its reasoning or final answer.
*   **Scrolling**: The scrolling logic is implemented in the application state (`App`) but is **disconnected** from the UI rendering layer (`ui.rs`), causing the visual freezing issue when scrolling.
*   **Perception**: The Agent does not "see" the terminal pixels; it reads a text serialization of the pseudo-terminal (PTY) state.

---

## 2. Deep Dive: Command Execution Flow

The execution of a shell command involves a round-trip coordination between the AI Agent, the Application State, and the Pseudo-Terminal (PTY).

### The Path: `Agent -> PTY -> Terminal`

1.  **Agent Decision**: The Agent decides to run a command (e.g., `ls -la`) and calls the `execute_command` tool.
2.  **Tool Execution**: The `ShellTool` constructs a `TuiEvent::ExecuteTerminalCommand` and sends it to the main application loop.
3.  **PTY Execution**: The application receives this event, writes the command to the active PTY, and listens for the output.
4.  **Capture & Return**: The output is captured from the PTY and returned to the `ShellTool` via a `oneshot` channel.
5.  **Observation**: The `ShellTool` packages this output (combined with recent terminal context) and returns it to the Agent as an "Observation".

### Why the Agent receives the output
The Agent *must* receive the output to determine if the command succeeded or failed. Without this feedback loop, the Agent would be "flying blind," unable to react to errors or chain commands based on previous results (e.g., "find the file, *then* read it").

### Commentary on Redundancy
Because the Agent treats the "Observation" as new information, it often feels compelled to summarize or repeat it to the user in its "Final Answer."
*   **Mitigation**: We can reduce this by instructing the system prompt to be less verbose or by filtering "Observation" blocks from the chat UI in non-verbose modes (which is partially implemented in `src/terminal/ui.rs`).

---

## 3. Deep Dive: Terminal Rendering & Scrolling

The terminal UI is built using `ratatui` and `tui-term`.

### Rendering Logic
*   **State**: The `App` struct holds the `PtyManager` and a `vt100::Parser` which maintains the state of the virtual terminal (lines, cursor position, colors).
*   **Visuals**: In `src/terminal/ui.rs`, the `render_terminal` function creates a `PseudoTerminal` widget using the screen data from `app.terminal_parser.screen()`.

### Commentary on Bug: The Disconnected Scroll
The scrolling functionality is currently broken due to a disconnect between the state and the UI.

*   **Logic Exists**: In `src/terminal/app.rs`, the logic for `scroll_terminal_up` and `scroll_terminal_down` exists and correctly updates `app.terminal_scroll`.
*   **UI Ignored**: However, in `src/terminal/ui.rs`, the `PseudoTerminal` widget is constructed **without** referencing `app.terminal_scroll`. It simply renders the current view of the parser.
    ```rust
    // src/terminal/ui.rs
    let vt100_screen = app.terminal_parser.screen();
    let terminal = PseudoTerminal::new(vt100_screen)
        .block(block);
    // Missing: .scroll(...) or offset logic
    frame.render_widget(terminal, area);
    ```
*   **Result**: You press keys, `app.terminal_scroll` changes, but the rendered widget never shifts its viewport.

---

## 4. Deep Dive: Agent Perception

When the Agent says "I don't see your terminal," it is being literal in a technical sense.

*   **No Pixels**: The Agent has no visual feed. It does not use computer vision.
*   **Data Strings**: When the Agent "looks" at the terminal, it is actually reading a serialized string of text provided by `ShellTool`.
    *   See `src/agent/tools/shell.rs`: The tool fetches `screen_content` (text) and combines it with the command output.
*   **Implication**: If the text serialization strips colors or formatting, the Agent loses that context. If the buffer is too large, it gets truncated (currently capped at ~50k tokens/200k chars).

---

## 5. Code References

*   **UI Rendering**: [`src/terminal/ui.rs`](src/terminal/ui.rs)
    *   `render_terminal` (Line 130): Defines how the terminal widget is drawn.
    *   `render_chat` (Line 163): Defines how chat history is drawn and filtered.
*   **Application Logic**: [`src/terminal/app.rs`](src/terminal/app.rs)
    *   `App` struct (Line 44): Holds `terminal_scroll` state.
    *   `run_agent_loop` (Line 453): Manages the Agent's execution cycle.
*   **Tool Logic**: [`src/agent/tools/shell.rs`](src/agent/tools/shell.rs)
    *   `ShellTool::call` (Line 46): capturing screen content and command output.
