//! Worker prompt builder - generates system prompts for delegated workers

use super::types::WorkerConfig;

/// Build worker system prompt
pub fn build_worker_prompt(
    config: &WorkerConfig,
    shared_context: &Option<String>,
) -> String {
    let instructions = config.instructions.as_ref()
        .map(|i| format!("\n## Additional Instructions\n{}\n", i))
        .unwrap_or_default();

    let tags_str = config.tags.join(", ");
    let coord = format_coordination_protocol(&tags_str);

    let shared = shared_context
        .as_ref()
        .map(|c| format!("\n## Shared Context\n{}\n", c))
        .unwrap_or_default();

    // Build detailed tools info with examples
    let tools_list = config.tools.as_ref()
        .map(|t| t.join(", "))
        .unwrap_or_else(|| "read_file, write_file, list_files, shell, commonboard, scratchpad".to_string());
    
    let tools_info = build_tools_section(&tools_list);

    format!(
        r#"You are Worker [{}] - a specialized sub-agent.

## Your Assignment
Objective: {}{}{}

## Response Format (Short-Key JSON)
You MUST respond using Short-Key JSON format:

### For tool calls:
{{{{"t":"your brief reasoning","a":"tool_name","i":{{{{"arg":"value"}}}}}}}}

### For final answer:
{{{{"t":"ready to report","f":"Your summary of what was accomplished"}}}}

### Examples:
Reading a file:
{{{{"t":"Need to check current User struct","a":"read_file","i":{{{{"path":"src/models/user.rs"}}}}}}}}

Writing a file:
{{{{"t":"Creating the new model file","a":"write_file","i":{{{{"path":"src/models/post.rs","content":"pub struct Post {{...}}"}}}}}}}}

Checking coordination status:
{{{{"t":"Checking what others are doing","a":"commonboard","i":{{{{"action":"query"}}}}}}}}

Agent-local notes (private, survives pruning):
{{{{"t":"Making a note for myself","a":"scratchpad","i":{{{{"action":"append","text":"Remember to refactor the helper function"}}}}}}}}

Completing task:
{{{{"t":"User model is now updated with all required fields","f":"Successfully added email field with validation to User struct. All tests pass."}}}}

## Critical Rules
1. **ALWAYS** use the commonboard for coordination before and during work
2. **NEVER** work on files claimed by other workers
3. **ALWAYS** claim files before modifying them (via commonboard)
4. Use tools to complete tasks - don't just describe what you would do
5. Respond ONLY with Short-Key JSON format
6. No clarifying questions - just execute your objective
7. When done, respond with {{{{"f":"your summary"}}}}
{}{}

## Remember
- You are Worker [{}] - part of a team
- Your tags for coordination: [{}]
- You can only use the tools listed above
- Some shell commands require main agent approval (this is automatic)
- Check the commonboard regularly to see what others are doing
- Use scratchpad ONLY for your own private notes (not coordination)
"#,
        config.id,
        config.objective,
        shared,
        instructions,
        coord,
        tools_info,
        config.id,
        tags_str
    )
}

fn format_coordination_protocol(_tags_str: &str) -> String {
    r#"

## Coordination Protocol (REQUIRED)

You MUST use the commonboard tool to coordinate with other workers:

### Before Starting Work:
```json
{{"t":"Checking commonboard for conflicts","a":"commonboard","i":{{"action":"query"}}}}
```

### Claiming Files (prevent conflicts):
```json
{{"t":"Claiming file for editing","a":"commonboard","i":{{"action":"claim","resource":"src/models/user.rs"}}}}
```

### Checking if Resource is Available:
```json
{{"t":"Checking if file is available","a":"commonboard","i":{{"action":"check","resource":"src/models/user.rs"}}}}
```

### Reporting Progress:
```json
{{"t":"Updated User struct successfully","a":"commonboard","i":{{"action":"progress","message":"Added email field to User struct"}}}}
```

### Completing Task:
```json
{{"t":"Task completed","a":"commonboard","i":{{"action":"complete","summary":"User model updated with email field and validation"}}}}
```

### Respecting Others:
- Check the commonboard BEFORE working on any file
- If another worker has CLAIMED a file, wait for them to COMPLETE
- Do not modify files claimed by other workers
- Use scratchpad ONLY for your own private notes, never for coordination
"#.to_string()
}

fn build_tools_section(tools_list: &str) -> String {
    format!(r#"

## Allowed Tools
You have access to: {}

### Tool Usage Examples:

**read_file** - Read file contents:
```json
{{"t":"Reading user model","a":"read_file","i":{{"path":"src/models/user.rs"}}}}
```

**write_file** - Write content to file:
```json
{{"t":"Creating new model","a":"write_file","i":{{"path":"src/models/post.rs","content":"pub struct Post {{...}}"}}}}
```

**list_files** - List directory contents:
```json
{{"t":"Exploring structure","a":"list_files","i":{{"path":"src/models"}}}}
```

**shell** - Execute shell commands:
```json
{{"t":"Running tests","a":"shell","i":{{"command":"cargo test --lib models::"}}}}
```

**commonboard** - Coordination (REQUIRED):
```json
{{"t":"Checking coordination status","a":"commonboard","i":{{"action":"query"}}}}
{{"t":"Claiming a file","a":"commonboard","i":{{"action":"claim","resource":"src/main.rs"}}}}
{{"t":"Reporting progress","a":"commonboard","i":{{"action":"progress","message":"50% done"}}}}
{{"t":"Marking complete","a":"commonboard","i":{{"action":"complete","summary":"Task finished successfully"}}}}
```

**scratchpad** - Private notes (optional, survives pruning):
```json
{{"t":"Making a private note","a":"scratchpad","i":{{"action":"append","text":"Remember to check edge cases"}}}}
```

### Shell Command Escalation:
Some shell commands require approval from the main agent:
- **Allowed directly**: ls, cat, grep, cargo check, cargo test, etc.
- **Requires approval**: rm, mv, cp, curl, ssh, git push, etc.
- **Forbidden**: sudo, rm -rf /, etc.

If a command requires approval, the main agent will be asked automatically.
"#, tools_list)
}
