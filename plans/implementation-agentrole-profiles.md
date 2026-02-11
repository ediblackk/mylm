# Implementation: AgentRole Profiles

This document explains the **AgentRole** pattern used in Codex to specialize agent instances with specific capabilities, models, and constraints. This pattern allows for quick creation of specialized "worker" agents without the overhead of full thread management.

---

## 1. Problem Statement

Generic agents often struggle with high-precision tasks or codebase-wide exploration when they have access to too many tools or are running on a model not optimized for the specific task.

### Why Specialize Agents?
- **Tool Noise**: Giving a searching agent "write" access increases the chance of accidental edits.
- **Model Efficiency**: Explorers benefit from high-reasoning models (e.g., `o3-mini`), while executors may need models with better tool-following (e.g., `gpt-4o`).
- **Safety**: "Worker" agents should be restricted by sandbox policies to prevent accidental system-wide changes.
- **Consistency**: Roles provide a standardized way to configure "Explorer", "Worker", or "Reviewer" behavior across different threads.

---

## 2. Architecture Overview

The AgentRole system in Codex consists of three main parts:
1.  **AgentRole Enum**: Defines the available archetypes (Explorer, Worker, Orchestrator).
2.  **AgentProfile Struct**: Holds the hard-coded defaults for each role (model, instructions, sandbox policy).
3.  **Config Application**: A mechanism to merge these defaults into the agent's active configuration.

### Configuration Inheritance
Roles work through **hierarchical overrides**:
1.  **Global Defaults**: System-wide settings from `config.toml`.
2.  **Role Defaults**: Applied when a specific `AgentRole` is selected.
3.  **Explicit Overrides**: Per-instance overrides provided via CLI or API.

---

## 3. Core Components

### AgentRole Enum
Located in [`codex-rs/core/src/agent/role.rs`](codex-rs/core/src/agent/role.rs), this enum defines the persona of the agent.

```rust
pub enum AgentRole {
    Default,      // Inherits parent config
    Orchestrator, // High-level planning and delegation
    Worker,       // Specialized execution (e.g., fixing a bug)
    Explorer,     // High-speed codebase search and question answering
}
```

### AgentProfile Struct
The profile contains the actual values applied to the agent.

```rust
pub struct AgentProfile {
    pub base_instructions: Option<&'static str>,
    pub model: Option<&'static str>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub read_only: bool,
    pub description: &'static str,
}
```

---

## 4. Implementation Patterns

### Mapping Roles to Profiles
Each variant in the `AgentRole` enum is mapped to a static `AgentProfile`.

```rust
impl AgentRole {
    pub fn profile(self) -> AgentProfile {
        match self {
            AgentRole::Default => AgentProfile::default(),
            AgentRole::Explorer => AgentProfile {
                model: Some("gpt-5.2-codex"),
                reasoning_effort: Some(ReasoningEffort::Medium),
                read_only: true, // Explorers should never write
                description: "Optimized for fast codebase exploration.",
                ..Default::default()
            },
            AgentRole::Worker => AgentProfile {
                model: Some("gpt-4o"),
                read_only: false, // Workers need write access
                description: "Optimized for task execution and bug fixes.",
                ..Default::default()
            },
            // ... other roles
        }
    }
}
```

### Applying Roles to Configuration
The `apply_to_config` method ensures that role defaults do not overwrite explicit user overrides unless intended.

```rust
impl AgentRole {
    pub fn apply_to_config(self, config: &mut Config) -> Result<(), String> {
        let profile = self.profile();
        
        if let Some(instr) = profile.base_instructions {
            config.base_instructions = Some(instr.to_string());
        }
        
        if let Some(model) = profile.model {
            config.model = Some(model.to_string());
        }
        
        if profile.read_only {
            config.sandbox_policy.set(SandboxPolicy::new_read_only_policy())?;
        }
        
        Ok(())
    }
}
```

---

## 5. Decision Guide: Role vs. ThreadManager

| Feature | AgentRole | ThreadManager |
| :--- | :--- | :--- |
| **Complexity** | Low (~150 lines) | High (~500+ lines) |
| **State** | Stateless (Config-only) | Stateful (Tracks sub-threads) |
| **Granularity** | Coarse (Archetypes) | Fine (Per-tool/Per-turn control) |
| **Best Use Case** | Specializing a sub-agent for a task | Orchestrating multiple parallel agents |
| **Integration** | CLI flag or Tool argument | Core system architecture |

---

## 6. Implementation Roadmap

### Phase 1: Foundation
- [ ] Define the `AgentRole` enum with at least 5 variants.
- [ ] Create the `AgentProfile` struct to hold model and policy defaults.
- [ ] Implement `AgentRole::profile()` mapping.

### Phase 2: Configuration Integration
- [ ] Add `role: AgentRole` field to the main `Agent` or `Thread` configuration.
- [ ] Implement the merging logic in `apply_to_config`.
- [ ] Update the CLI to accept `--role <name>`.

### Phase 3: Specialization
- [ ] Create specialized system prompts for each role in `templates/agents/`.
- [ ] Define role-specific sandbox policies (e.g., `Explorer` is always read-only).
- [ ] (Optional) Filter available tools based on role.

---

## 7. Best Practices

1.  **Read-Only by Default**: Roles like `Explorer` or `Reviewer` should default to read-only sandbox policies to increase safety.
2.  **Clear Descriptions**: Include descriptions in the profiles so they can be exposed to the LLM in tool definitions (see `SpawnAgent` tool).
3.  **Orthogonal Roles**: Avoid overlap. If a role is "Explorer", don't give it "Write" capabilities.
4.  **Template System**: Use `include_str!` to load large system prompts from markdown files instead of hardcoding strings in Rust.
