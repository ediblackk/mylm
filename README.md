# My Little Minion (mylm)

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Terminal AI](https://img.shields.io/badge/Terminal-AI-blue.svg)](.)

<p align="center">
  <img src="assets/logo.png" alt="mylm Logo" width="180">
</p>

## The AI assistant that actually understands your terminal

**mylm** is a privacy-focused, multi-agent terminal AI assistant built in Rust. It sees your terminal, remembers your projects, delegates tasks to specialized sub-agents, and safely executes commands—all while keeping you in control.

### Features

- **Persistent vector memory** — LanceDB with semantic search across sessions
- **Multi-agent orchestration** — Parallel workers with job tracking
- **Full terminal context** — tmux integration, zero manual context sharing
- **Background job system** — Async delegation, non-blocking
- **Safety-first execution** — Allowlists, approval workflow, PTY isolation
- **Lightweight & fast** — Rust-native, sub-100ms cold start

---

## Quick Start

### Installation

git clone https://github.com/ediblackk/mylm.git
cd mylm
chmod +x install.sh
./install.sh

Installs to `~/.local/bin` without sudo.

### First Use

ai                    # Start the interactive hub
ai "question"         # Ask a one-off question
ai pop                # Pop terminal context (requires tmux)
ai interactive        # Full TUI session

---

## ✨ What Makes mylm Special

### 🎯 `ai pop` — Context Magic
Your command fails. Instead of copying error messages, just type:

ai pop

mylm captures your terminal history, working directory, git state, environment variables, and recent commands. The AI sees exactly what you see. **No setup. No copy-paste. Just context.**

*Requires tmux (we'll help you set it up).*

### 🧠 Multi-Agent System
Most AI assistants are a single brain trying to do everything. mylm uses an **orchestrator-worker pattern**:

- **Orchestrator** plans and delegates
- **Worker agents** execute subtasks in parallel
- **Delegate tool** spawns specialized agents with their own toolsets
- **Job registry** tracks progress across all agents

Research a library while refactoring code—all at once.

### 🔄 PaCoRe: Parallel Consensus Reasoning
(https://github.com/stepfun-ai/PaCoRe)
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
- Any OpenAI Compatible endpoint
- Google Gemini 3
- OpenAI (GPT-5.2)
- Anthropic (Claude Sonnet 4.5)
- DeepSeek V3.2
- StepFun 3.5 Flash
- Kimi K2.5 (Moonshot)

---

## 🙏 Acknowledgements

Built with assistance from every AI that would talk to me: Claude, GPT, Gemini,
Kimi, DeepSeek, StepFun, Z.AI, MiniMax and probably others I've forgotten. Also 147 Rust crates I didn't write. And Linux/Debian.And VS Code. And ASUS. And liters of coffee.

---

## 📄 License

MIT — See [LICENSE](LICENSE) for details.