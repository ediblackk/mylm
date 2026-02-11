# Token-Efficient Orchestration Patterns for Large Codebases

## 🎯 Core Principle: Orchestrator NEVER Reads Files Directly

The orchestrator's job is to **coordinate, not investigate**. Every piece of information should come from subtask results.

---

## PATTERN 1: Surgical Debug (Investigation-First)

**Use when:** App crashes, errors occur, something doesn't work

### Workflow:
```
User: "App crashes when accessing configuration menu"

STEP 1 - LOCATE (Ask Mode)
├── Subtask: "List all files related to 'configuration' or 'config menu'. 
│            Only return file paths and a ONE-LINE description of each.
│            Do NOT read file contents in full."
└── Result: "Found 3 files: src/config/menu.rs (UI component), src/config/mod.rs (module root), src/state/config.rs (state management)"

STEP 2 - IDENTIFY ENTRY POINT (Ask Mode)  
├── Subtask: "In src/config/menu.rs, find the function that handles menu opening.
│            Return ONLY: function name, line numbers, and function signature.
│            Do NOT paste the full implementation."
└── Result: "Function `open_menu()` at lines 45-89, signature: pub fn open_menu(state: &AppState) -> Result<(), ConfigError>"

STEP 3 - TRACE ERROR PATH (Debug Mode)
├── Subtask: "In src/config/menu.rs lines 45-89, identify all Result/Option unwraps
│            and potential panic points. List them with line numbers only."
└── Result: "Line 52: unwrap() on state.settings, Line 67: expect() on file read"

STEP 4 - VERIFY HYPOTHESIS (Debug Mode)
├── Subtask: "Read ONLY lines 50-55 of src/config/menu.rs and explain what
│            state.settings.unwrap() expects. What could make it None?"
└── Result: "It expects Settings struct. Could be None if config file missing or parse fails"

STEP 5 - FIX (Code Mode)
├── Subtask: "In src/config/menu.rs, replace line 52's unwrap() with proper error handling.
│            Return AppError::ConfigNotLoaded if None. Show ONLY the changed lines."
└── Result: [Minimal fix applied]
```

---

## PATTERN 2: File-Scoped Delegation

**Use when:** Need to modify a specific feature

### Workflow:
```
User: "Add dark mode toggle to settings"

STEP 1 - MAP ARCHITECTURE (Architect Mode)
├── Subtask: "Examine the project structure and identify:
│            1. Where UI components live (just folder path)
│            2. Where state/settings are managed (just folder path)  
│            3. The naming convention used
│            Return a 5-line summary maximum."
└── Result: "UI: src/terminal/ui.rs, State: src/state/, Convention: snake_case modules"

STEP 2 - FIND SIMILAR PATTERN (Ask Mode)
├── Subtask: "Find an existing toggle or boolean setting in the settings UI.
│            Return: file path, function name, and line range (max 10 lines of code)."
└── Result: "Found 'auto_save' toggle in src/terminal/ui.rs:234-244"

STEP 3 - UNDERSTAND STATE (Ask Mode)
├── Subtask: "How is 'auto_save' setting stored and persisted?
│            Name the struct, field, and save mechanism. No code dumps."
└── Result: "Stored in AppSettings.auto_save: bool, persisted via serde to config.json"

STEP 4 - IMPLEMENT (Code Mode)
├── Subtask: "Add 'dark_mode: bool' field to AppSettings struct following the 
│            exact pattern of 'auto_save'. File: src/state/mod.rs
│            Change ONLY what's needed. Show diff."
└── Result: [Field added]

STEP 5 - ADD UI (Code Mode)
├── Subtask: "Add dark mode toggle to settings UI in src/terminal/ui.rs
│            following the exact pattern at lines 234-244 (auto_save toggle).
│            Place it after that toggle. Show diff only."
└── Result: [Toggle added]
```

---

## PATTERN 3: Layer-by-Layer Analysis

**Use when:** Understanding how something flows through the system

### Workflow:
```
User: "Terminal output is garbled"

LAYER 1 - OUTPUT (Ask Mode)
├── Subtask: "What is the final function that writes to terminal?
│            File path and function name only."
└── Result: "src/terminal/pty.rs::write_output()"

LAYER 2 - PROCESSING (Ask Mode)
├── Subtask: "What transforms data before it reaches write_output()?
│            List the call chain as: file:function -> file:function"
└── Result: "src/terminal/app.rs:process() -> src/terminal/mod.rs:format() -> pty.rs:write_output()"

LAYER 3 - IDENTIFY SUSPECT (Debug Mode)
├── Subtask: "In src/terminal/mod.rs, what does format() do to the data?
│            Summarize in 2-3 sentences. Does it handle encoding?"
└── Result: "It converts raw bytes to String using from_utf8_lossy. No explicit encoding handling."

LAYER 4 - ROOT CAUSE (Debug Mode)
├── Subtask: "Check if from_utf8_lossy is appropriate for terminal ANSI sequences.
│            What happens to escape codes? Answer in 2 sentences."
└── Result: "from_utf8_lossy replaces invalid UTF-8 with replacement chars. ANSI codes are valid UTF-8 so that's not the issue."

[Continue narrowing until root cause found]
```

---

## PATTERN 4: Test-Driven Isolation

**Use when:** Bug is intermittent or hard to reproduce

### Workflow:
```
User: "Sometimes saves don't persist"

STEP 1 - IDENTIFY SAVE PATH (Ask Mode)
├── Subtask: "What is the save function and where is data written?
│            Function name, file path, and destination path only."
└── Result: "save_state() in src/state/mod.rs, writes to ~/.mylm/state.json"

STEP 2 - CHECK ERROR HANDLING (Debug Mode)
├── Subtask: "Does save_state() have any silent error swallowing?
│            Look for: empty catch, ignored Results, _ patterns.
│            List suspicious lines."
└── Result: "Line 89: `let _ = fs::write(...)` - Result ignored!"

STEP 3 - VERIFY (Debug Mode)
├── Subtask: "What error conditions could fs::write fail with?
│            Check if parent directory creation happens before write."
└── Result: "No mkdir_p before write. Fails if ~/.mylm/ doesn't exist."

STEP 4 - FIX (Code Mode)
├── Subtask: "In src/state/mod.rs save_state(), add create_dir_all before write
│            and proper error propagation. Show minimal diff."
└── Result: [Fix applied]
```

---

## 📋 Ready-to-Use Subtask Prompt Templates

### Template A: Locate Files
```
MODE: ask
PROMPT: "List all files related to [FEATURE/KEYWORD]. Return ONLY:
- File paths
- One-line description of each file's purpose
Do NOT read or dump file contents. Maximum 10 files."
```

### Template B: Find Function
```
MODE: ask  
PROMPT: "In [FILE_PATH], find the function that [DOES X].
Return ONLY:
- Function name
- Line numbers (start-end)
- Function signature
Do NOT return the implementation body."
```

### Template C: Trace Execution
```
MODE: debug
PROMPT: "Starting from [FUNCTION_NAME] in [FILE_PATH], trace the call chain for [ACTION].
Return as: file:function -> file:function -> ...
Maximum 5 hops. Do NOT read full files."
```

### Template D: Analyze Specific Lines
```
MODE: debug
PROMPT: "Read ONLY lines [X-Y] of [FILE_PATH].
Answer: [SPECIFIC QUESTION]
Do NOT expand beyond these lines."
```

### Template E: Implement Fix
```
MODE: code
PROMPT: "In [FILE_PATH] at line [X], make this specific change: [CHANGE].
Follow the existing code patterns exactly.
Return ONLY the diff or changed lines.
Do NOT refactor or modify unrelated code."
```

### Template F: Find Similar Pattern
```
MODE: ask
PROMPT: "Find an existing implementation of [SIMILAR_FEATURE] in this codebase.
Return: file path, function name, line range.
Show maximum 15 lines of the most relevant code."
```

---

## ⚡ Token Efficiency Rules

1. **Never ask for "all the code"** - Always specify line ranges
2. **Never ask for "explain the file"** - Ask specific questions
3. **One concern per subtask** - Don't combine locate + analyze + fix
4. **Summaries over dumps** - "Describe in 2 sentences" not "show me"
5. **Build incrementally** - Each subtask's result informs the next
6. **Stop early** - If a subtask reveals the issue, don't continue mapping

---

## 🚀 Quick Start for Your Project

For your immediate bug, describe it like this:
```
"[SYMPTOM] happens when [ACTION]"
```

I will then orchestrate using Pattern 1 (Surgical Debug):
1. First subtask: Locate relevant files (no reading)
2. Second subtask: Find entry point function (signature only)
3. Third subtask: Identify error-prone lines (line numbers)
4. Fourth subtask: Analyze suspect lines (minimal context)
5. Fifth subtask: Apply targeted fix (diff only)

**What specific bug or issue should I help you debug using this pattern?**