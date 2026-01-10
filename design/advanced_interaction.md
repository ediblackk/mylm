# Advanced Session & Interaction Features Design

## 1. Overview
This document outlines the architecture for enhancing user interaction and session management in `mylm`. The key goals are to provide interactive session controls (resume/fresh), implement context limits, allow user interruption, and offer visibility into the agent's thought process.

## 2. Interactive Session Start
**Goal:** Allow users to choose between starting a fresh session or resuming a previous one.

### Architecture Changes
*   **`src/cli/hub.rs`**:
    *   Modify `show_hub` to include a "Resume Session" option if a previous session exists.
    *   The "Start TUI Session" option will default to a fresh session.
*   **`src/terminal/app.rs`**:
    *   Add functionality to load `chat_history` from a file.
    *   When "Resume" is selected, the app initializes with the loaded history.
*   **`src/main.rs`**:
    *   Update `HubChoice` handling to support the new flow.
    *   Implement session persistence (saving `chat_history` to `~/.local/share/mylm/sessions/latest.json` on exit/update).

### Data Flow
1.  User starts `mylm`.
2.  `show_hub` checks for `latest.json`.
3.  If present, "Resume Session" is shown.
4.  User selects "Resume".
5.  `latest.json` is deserialized into `Vec<ChatMessage>`.
6.  `App::new` is called with this history.

## 3. Context Limits
**Goal:** Prevent context window overflow by implementing a token/line limit.

### Architecture Changes
*   **`src/config/mod.rs`**:
    *   Add `max_history_lines` (default 50k) and `max_history_tokens` (default 100k) to `Config` struct.
*   **`src/agent/core.rs`**:
    *   In `run`, before sending `history` to `llm_client`, implement a pruning strategy.
    *   **Strategy:** Keep `System` prompt always. Keep the last N messages that fit within the token/line limit.
    *   *Note:* The existing `condense_history` is a good start, but a hard limit is needed for safety.
*   **Pruning Logic:**
    1.  Calculate total tokens/lines of `history`.
    2.  If > limit:
        a.  Keep index 0 (System).
        b.  Take the last K messages such that sum(tokens) < limit.
        c.  Discard middle messages (potentially summarizing them if condensation is active, but for hard limits, dropping is safer to guarantee size).

## 4. User-in-the-Loop & Interruption
**Goal:** Allow the user to stop generation and provide input during the loop.

### Architecture Changes
*   **`src/terminal/app.rs`**:
    *   **Interruption:**
        *   Listen for `Ctrl+C` or a specific key (e.g., `Esc`) during `AppState::Processing`.
        *   If triggered, set an `interrupt_flag` shared with the Agent task.
    *   **User Input Request:**
        *   Enhance `ask_user` tool integration.
        *   When `ask_user` is called, the TUI should switch to `Focus::Chat` and wait for input, pausing the Agent loop.
*   **`src/agent/core.rs`**:
    *   Add `interruption_signal: Arc<AtomicBool>` to `Agent`.
    *   Check this signal inside the loop (e.g., before each LLM call or tool execution).
    *   If signaled, break the loop and return "Interrupted by user".

### Interaction Flow (Interruption)
1.  User types `Esc` while "Thinking...".
2.  `App` sets `agent_abort_flag`.
3.  `Agent` loop checks flag.
4.  `Agent` stops, returns partial result or "Interrupted".
5.  TUI state returns to `Idle`.

### Interaction Flow (Ask User)
1.  LLM calls `ask_user(question="...")`.
2.  `Tool::call` returns a special "WAIT_FOR_INPUT" signal or hangs (async) until input is received.
    *   *Better approach:* The `ask_user` tool sends a `TuiEvent::RequestInput(question)` and awaits a response channel.
    *   TUI renders the question, user types, hits Enter.
    *   Input is sent back to the tool's waiting channel.
    *   Tool returns user input as observation.

## 5. Step-by-Step Visibility
**Goal:** Toggle between showing intermediate steps (Thoughts/Actions) or only final results.

### Architecture Changes
*   **`src/config/mod.rs`**:
    *   Add `show_intermediate_steps: bool` to `Config`.
*   **`src/terminal/ui.rs`**:
    *   In `render_chat`, filter `chat_history`.
    *   If `show_intermediate_steps` is false:
        *   Hide `MessageRole::Tool` messages.
        *   Hide `MessageRole::Assistant` messages that contain `Action:`/`Thought:` but no `Final Answer` (or parse them out).
        *   *Simpler:* Just hide `Tool` roles and maybe `Assistant` messages that don't look like final responses.
*   **`src/terminal/app.rs`**:
    *   Add a keybind (e.g., `Ctrl+v`) to toggle `config.show_intermediate_steps` live.

## 6. Implementation Plan

### Phase 1: Context & Session
1.  Update `Config` struct.
2.  Implement `save_session` and `load_session` in `src/terminal/session.rs` or `app.rs`.
3.  Modify `src/cli/hub.rs` to expose "Resume".
4.  Implement pruning logic in `src/agent/core.rs`.

### Phase 2: Interruption & Visibility
1.  Add `Arc<AtomicBool>` for cancellation to `Agent`.
2.  Wire up `Esc` in `App::handle_input` to trigger cancellation.
3.  Implement `TuiEvent::RequestInput` handling for `ask_user` (refactor `ShellTool` or create dedicated `InteractionTool`).
4.  Implement "Step-by-Step" toggle in `ui.rs` rendering logic.
