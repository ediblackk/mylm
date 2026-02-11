# My Little Minion | My Language Model | My Learning Machine | My Local Model (MyLM) 

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Terminal AI](https://img.shields.io/badge/Terminal-AI-blue.svg)](.)

## Demo

https://github.com/user-attachments/assets/placeholder1

https://github.com/user-attachments/assets/placeholder2

https://github.com/user-attachments/assets/placeholder3


## Safe Personal AI assistance into your terminal

**MyLM** is a privacy-focused, multi-agent terminal AI assistant built in Rust. It can see in your terminal, remember your projects, delegates tasks to specialized sub-agents, and safely executes commands—all while keeping you in control.

### Features

- **Persistent vector memory** — LanceDB with semantic search across sessions
- **Multi-agent orchestration** — Parallel workers with job tracking
- **Full terminal context** — tmux integration, zero manual context sharing
- **Background job system** — Async delegation, non-blocking
- **Safety-first execution** — Allowlists, approval workflow, PTY isolation
- **Lightweight & fast** — Fast cold starts and quick execution

<p align="center">
  <img src="assets/logo.png" alt="mylm Logo" width="180">
</p>


---

## Quick Start

### Installation

git clone https://github.com/ediblackk/mylm.git
cd mylm
chmod +x install.sh
./install.sh

Installs to `~/.local/bin` without sudo.

### First Use

Unless you changed the alias, simply type "ai" into your terminal. Proceed to Config your preferred providers, select main model, set your context limit and costs and then simply proceed to "Start TUI Session".

I also highly recommend you set the web_search API too, as it enhances the model's capacity to provide relevant answers.

I use it as chat, mainly tired of key aspects when using llm's from main providers:
- On long contexts, chat apps crash
- Even on not so long, they can still lag/crash
- I don't like the fact that all my data is stored externally
- I tried other personal AI assistants apps and simply did not match very well.

This does not require docker or any other special configurations to work. It can be a little bit tricky to get everything required to build and it does take some time to finish (especially the optimized release version, on my local i7-7700HQ it took almost 40 minutes to finish). But the bianry itself is pretty small and quick.

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

### KNOWN ISSUES / SOLVING SOON
- **Context grows uncontrollably** - when simply conversating with some history, context can grow way more than required; most likely some repeated data/content not caching; 
- **On stall, workers are not approved to contine** - in order to prevent unending loops, wasted resources and precise actions, sub-agent workers require approval for more actions after a number of actions (15); as they reach it, main agent does not react and does not allow further actions;
- **Memory needs refining** - scribe function is currently disabled; it is suppossed to continuously memorise actions and information, and inject information relevant to context to make the main model aware; this adds complexity and for basic functions is a bit overkill; totally refining this later though;
- **Code organisation and cleanup** - I am aware there is still mess around in  codebase; my main concern was to finish it and have something fast, stable and usable in day to day tasks; so far I am glad with how it runs, it's my first Rust project and I love it.


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