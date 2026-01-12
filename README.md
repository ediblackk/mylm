# mylm — Terminal AI, done right

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Terminal AI](https://img.shields.io/badge/Terminal-AI-blue.svg)](#)

**mylm (My Language Model)** is a high‑performance **Terminal AI Assistant** built for developers and power users who live in the command line.

It is not a chatbot wrapper.
It is not a toy CLI.

`mylm` is a **context‑aware, agentic terminal companion** that understands your environment, reasons about tasks, and safely bridges natural language with shell execution.

Installed as the simple `ai` command, it integrates directly into your workflow instead of pulling you out of it.

---

## Why mylm

Most "AI CLI tools" today are thin API frontends:

*   stateless
*   slow to start
*   blind to your system
*   unsafe around command execution

**mylm is different by design.**

It treats the terminal as a *first‑class environment*, not a text box.

---

## Core Capabilities

### ⚡ Seamless Terminal Evolution

`mylm` doesn't just "chat" — it evolves your current terminal session.

*   **Pop Terminal (`ai pop`)**: Instantly "pops" your current shell state into the AI interface.
*   **Restores Context**: Uses `tmux` to capture your active pane's history, running processes, and environment variables.
*   **No Context Switching**: Resume exactly where you left off, but now with an AI assistant that sees what you see.

### 🫧 Clean Execution & Smart Reflow

*   **Clean Execution**: Commands run visibly in a dedicated terminal pane, but their raw output doesn't clutter the chat. The AI sees the output, but you get a clean, readable conversation.
*   **Smart Reflow**: The interface dynamically resizes and reflows content as you adjust your terminal window. No broken layouts or cutoff text.

### 🧠 Agentic Think‑Plan‑Execute Loop

*   Interactive mode where the AI can:
    *   reason about a problem
    *   plan multi‑step solutions
    *   execute validated shell commands
*   Designed for real work, not demos

### 🖥 Deep Terminal Context Awareness

`mylm` automatically understands:

*   current working directory
*   Git repository status
*   system information
*   execution history

This context is continuously fed into the model to produce **relevant, actionable answers**.

### 🔐 Smart & Guarded Command Execution

*   Commands proposed by the AI are **analyzed before execution**
*   Optional dry‑run mode for zero‑risk inspection
*   You stay in control at every step

### 🌐 Live Web Search & Crawling

*   Real‑time search for up‑to‑date information
*   Website crawling for deeper technical analysis
*   No stale training‑data hallucinations

### 🔁 Multi‑Provider & Local Model Support

One unified interface for:

*   **Local models**: Ollama, LM Studio
*   **Cloud providers**: Google Gemini, OpenAI, Anthropic

Switch providers or endpoints without changing your workflow.

### 🗂 Persistent Memory (RAG)

*   Local vector database for long‑term knowledge
*   Store project notes, decisions, and references
*   Retrieve them naturally during future sessions

---

## Security Philosophy — Build From Source

`mylm` is intentionally distributed **as source code only**.

This tool integrates deeply with your operating system and shell. That level of power demands transparency.

You are encouraged to:

1.  **Audit the code** (manually or with AI assistance)
2.  **Inspect dependencies** via `Cargo.toml` and the lockfile
3.  **Build locally**, knowing exactly what binary you are running

There are no hidden installers, no prebuilt binaries, and no silent updates.

You stay in control.

---

## Supported Platforms

*   **Linux** — primary target, fully optimized
*   **macOS** — fully supported
*   **Windows** — in active development

---

## Installation (Recommended)

### Prerequisites

*   **Rust Toolchain** (installed automatically if missing)
*   **tmux** (highly recommended for Seamless Terminal Evolution)

### Build From Source

```bash
git clone https://github.com/ediblackk/mylm.git
cd mylm
chmod +x install.sh
./install.sh
```

### What the installer does

*   Detects your Linux distribution
*   Installs missing system dependencies (including **tmux**)
*   Builds the project locally
*   Sets up the `ai` command
*   Configures **tmux auto-start** (optional) for seamless context capture
*   Preserves existing configuration on updates

> During active development, the installer defaults to **debug builds** for faster iteration. Release builds will become the default once the core feature set stabilizes.

---

## Usage

### 🚀 Seamless Terminal Evolution

```bash
ai pop
```

This is the most powerful way to use `mylm`. It "pops" your current terminal session into the AI interface, carrying over your command history and context. Requires `tmux`.

### ✨ Fresh Interactive Session

```bash
ai interactive
```

Starts a fresh, clean AI session. Ideal for new tasks unrelated to your current terminal history.

### 🤖 Interactive Hub

```bash
ai
```

Opens the central hub menu where you can choose between Pop Terminal, Resume Session, Interactive Mode, or Configuration.

### Direct Queries

```bash
ai "how do I safely revert the last three git commits?"
```

### Command Analysis & Execution

```bash
ai execute "find . -name '*.tmp' -exec rm {} +"
```

### Switch Providers on the Fly

```bash
ai -e openai "write a python script to parse these logs"
```

---

## Configuration

Configuration lives at:

```text
~/.config/mylm/mylm.yaml
```

### Manage Profiles

```bash
ai config edit prompt
ai config select
```

### Example Configuration

```yaml
default_endpoint: local-ollama
endpoints:
  - name: local-ollama
    provider: openai
    base_url: http://localhost:11434/v1
    model: llama3.2
    api_key: none

  - name: anthropic-claude
    provider: anthropic
    model: claude-3-5-sonnet-latest
    api_key: ${ANTHROPIC_API_KEY}
```

---

## Roadmap

*   Background jobs & task queue
*   Multi‑server orchestration (master → agents)
*   Windows native support
*   Extended TUI workflows

---

## License

MIT License

---

## Acknowledgements & Ecosystem Respect

This project exists thanks to the work of many open‑source and research communities.

### Core Foundations

*   **Rust** — systems programming without compromise
*   **Linux & Git** — the backbone of modern development
*   **ratatui, tokio, serde, clap, portable‑pty, lancedb** — and many more

### AI Research & Model Providers

Mentioned respectfully for attribution, compatibility, and ecosystem context:

*   **Google DeepMind** — Gemini models
*   **OpenAI** — GPT models and tooling
*   **Anthropic** — Claude and Constitutional AI
*   **Meta AI** — Llama models
*   **Hugging Face** — open ML infrastructure
*   **DeepSeek**, **Zhipu AI**, **Moonshot AI**, **Minimax** — advancing efficient and accessible language models

No affiliation or endorsement is implied.

---

## Keywords

Terminal AI, CLI LLM, Rust AI tool, Local LLM assistant, Ollama CLI, OpenAI terminal, Anthropic Claude CLI, Gemini terminal, Developer productivity, Command‑line AI, tmux AI
