# My Little Minion (mylm)

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Terminal AI](https://img.shields.io/badge/Terminal-AI-blue.svg)](#)

> **The AI assistant that actually understands your terminal.** Built in Rust. Designed for developers who want real productivity, not just chat.

**mylm** is a **multi-agent terminal AI assistant** that goes beyond simple Q&A. It sees what you see, remembers your projects, delegates tasks to specialized sub-agents, and safely executes commands—all while keeping you in control.

![mylm Dashboard](assets/hero.png)

---

## 🚀 Why mylm vs. The Alternatives

Recent tools like **OpenClaw** proved there's massive demand for terminal AI assistants. But they also exposed critical flaws: fragile context capture, no memory across sessions, single-threaded reasoning, and limited tool ecosystems.

**mylm was built differently from the ground up:**

| Feature | mylm | Others |
|---------|------|--------|
| **Multi-Agent Architecture** | ✅ Orchestrator + worker agents | ❌ Single agent |
| **Local Vector Memory** | ✅ LanceDB with semantic search | ❌ No memory |
| **Parallel Consensus (PaCoRe)** | ✅ Multi-path reasoning | ❌ Single-shot |
| **Terminal-Native Context** | ✅ tmux integration + full capture | ⚠️ Partial |
| **Background Jobs** | ✅ Async task scheduling | ❌ Blocking |
| **Safety System** | ✅ Allowlists + approval workflow | ⚠️ Basic |
| **Speed** | ✅ Rust-native, sub-100ms startup | ⚠️ Slower |

---

## ✨ What Makes mylm Special

### 🎯 `ai pop` — Context Magic
Your command fails. Instead of copying error messages, just type:
```bash
ai pop
```
mylm captures your terminal history, working directory, git state, environment variables, and recent commands. The AI sees exactly what you see. **No setup. No copy-paste. Just context.**

*Requires tmux (we'll help you set it up).*

### 🧠 Multi-Agent System
Most AI assistants are a single brain trying to do everything. mylm uses an **orchestrator-worker pattern**:

- **Orchestrator** plans and delegates
- **Worker agents** execute subtasks in parallel
- **Delegate tool** spawns specialized agents with their own toolsets
- **Job registry** tracks progress across all agents

Research a library while refactoring code—all at once.

### 💾 Local Vector Memory (LanceDB)
mylm doesn't forget. It stores:
- Project decisions and architecture notes
- Code patterns and preferences
- Conversation history (semantically searchable)
- File relationships and dependencies

Over time, it learns *your* codebase. Ask "How do we handle auth here?" and get relevant answers from past conversations.

### 🔄 PaCoRe: Parallel Consensus Reasoning
When accuracy matters, mylm can run **multi-round reasoning**:
1. Spawn multiple parallel LLM calls with different reasoning paths
2. Let them critique and build on each other's answers
3. Synthesize a consensus response

Better answers for complex debugging and architecture decisions.

### 🛡️ Safety-First Execution
Every command goes through:
1. **Static analysis** — Pattern-based risk detection
2. **Allowlist checking** — Known safe commands
3. **User approval** — You see it before it runs

Run with `--execute` for trusted commands. Use `--force` only when you know what you're doing.

### 🌐 10+ Built-in Tools
- **shell** — Execute with safety checks
- **git** — Status, log, diff analysis
- **fs** — Read/write files
- **web_search** — Real-time information
- **crawl** — Deep documentation extraction
- **memory** — Store and retrieve knowledge
- **delegate** — Spawn sub-agents
- **state** — Persistent key-value storage
- **terminal_sight** — Capture terminal state
- **system** — Resource monitoring

### ⚡ Built for Speed
- **Rust** — Zero-cost abstractions, memory safety
- **Async tokio** — Non-blocking I/O throughout
- **Optimized profiles** — Fast compile in dev, LTO in release
- **Sub-100ms** cold start to interactive

---

## 🎬 Quick Start

### Installation
```bash
git clone https://github.com/ediblackk/mylm.git
cd mylm
chmod +x install.sh
./install.sh
```

**No sudo required.** Installs to `~/.local/bin`.

### First Use
```bash
# Launch the hub
ai

# Quick question
ai "how do I find large files in this repo?"

# Pop terminal context (inside tmux)
 cargo build  # fails...
ai pop        # "What's wrong?"

# Interactive session
ai interactive
```

---

## 📚 Core Commands

| Command | Description |
|---------|-------------|
| `ai` | Hub — start conversations, manage sessions, configure |
| `ai "question"` | One-shot query with context |
| `ai pop` | Pop terminal context into AI (tmux) |
| `ai interactive` | Full TUI session |
| `ai session list` | View saved sessions |
| `ai session resume <id>` | Continue a conversation |
| `ai config` | Settings dashboard |
| `ai --version` | Show version & build info |

---

## 🏗️ Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                         CLI (ai)                            │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐  │
│  │   Hub    │  │  TUI     │  │ One-Shot │  │  Daemon    │  │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └─────┬──────┘  │
├───────┴─────────────┴─────────────┴──────────────┴─────────┤
│                       mylm-core                             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              Agent V2 (Orchestrator)                  │  │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────────┐  │  │
│  │  │  Reason    │→ │   Plan     │→ │    Delegate    │  │  │
│  │  └────────────┘  └────────────┘  └────────────────┘  │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │  Tools   │  │  Memory  │  │  PaCoRe  │  │  Jobs    │   │
│  │ Registry │  │ VectorDB │  │  Engine  │  │Scheduler │   │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │         Context Engine (git, sys, terminal)          │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              LLM Client (Multi-Provider)              │  │
│  │   Gemini · OpenAI · Anthropic · Ollama · DeepSeek    │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## 🔧 Configuration

Config lives in `~/.config/mylm/mylm.yaml`:

```yaml
profile: default
profiles:
  default:
    provider: Gemini
    model: gemini-2.0-flash-exp
    api_key: "${GEMINI_API_KEY}"
    base_url: "https://generativelanguage.googleapis.com/v1beta"
    max_iterations: 50
    
features:
  memory:
    enabled: true
  web_search:
    enabled: true
    provider: searxng
```

Or use the interactive dashboard: `ai config`

---

## 🔒 Security & Privacy

- **Local-first**: Vector DB runs locally (LanceDB)
- **No telemetry**: Your data stays yours
- **Command safety**: Approval workflow, allowlists, pattern detection
- **API key handling**: Stored in config, never logged
- **Sandboxed execution**: Commands run in isolated PTY

---

## 🛠️ Supported Providers

**Local (Free, Private):**
- Ollama
- LM Studio
- HuggingFace (via inference API)

**Cloud (API Key Required):**
- Google Gemini
- OpenAI (GPT-4, GPT-3.5)
- Anthropic (Claude)
- DeepSeek
- StepFun
- Kimi (Moonshot)

---

## 🧪 Advanced Features

### Batch Processing (PaCoRe)
```bash
# Run multi-round consensus on a dataset
ai batch --input questions.jsonl --output results.jsonl \
  --model gemini-2.0-flash-exp --rounds "3,2,1"
```

### Background Jobs
```bash
ai  # Hub → Background Jobs
# View, monitor, and manage long-running tasks
```

### Custom Prompts
Edit per-profile prompts in `~/.config/mylm/prompts/`:
```bash
ai config edit prompt
```

---

## 🚧 Roadmap

- [x] Multi-agent architecture with delegation
- [x] Local vector memory with LanceDB
- [x] PaCoRe parallel consensus reasoning
- [x] Job scheduling and background execution
- [x] Session persistence and management
- [ ] MCP (Model Context Protocol) integration
- [ ] Plugin system for custom tools
- [ ] Web dashboard for job monitoring
- [ ] Team sharing for memory stores

---

## 🤝 Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

**New to the project?** Check out [ONBOARDING.md](ONBOARDING.md) for a gentle introduction to the codebase.

---

## 🙏 Acknowledgements

Built on the shoulders of giants:
- **Rust** — For performance and safety
- **ratatui** — Beautiful terminal UIs
- **tokio** — Async runtime
- **LanceDB** — Vector search
- **Google, Anthropic, OpenAI, Meta** — For pushing AI forward

And countless open-source contributors. And coffee. ☕

---

## 📄 License

MIT — See [LICENSE](LICENSE) for details.

---

<p align="center">
  <strong>Stop copying errors. Start <code>ai pop</code>.</strong>
</p>

<p align="center">
  <sub>Keywords: Terminal AI, CLI LLM, AI Agent, Multi-Agent System, Developer Productivity, Local LLM, Vector Memory, Rust CLI, tmux AI, Autonomous Coding Assistant</sub>
</p>
