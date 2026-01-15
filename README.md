# mylm — Terminal AI, done right

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Terminal AI](https://img.shields.io/badge/Terminal-AI-blue.svg)](#)

**mylm (My Language Model)** is a high-performance **Terminal AI Assistant** built for developers who actually spend their day in the CLI. 

It’s not a fancy chatbot wrapper or a toy script. `mylm` is an **agentic terminal companion** that understands your system, reasons through tasks, and bridges natural language with actual shell execution.

You install it as the `ai` command, and it fits right into your workflow instead of getting in the way.

---

## What makes it different?

Most "AI CLI tools" are just thin API frontends: stateless, slow, and blind to your environment. `mylm` treats the terminal as a first-class citizen.

### ⚡ Pop Terminal & Context (`ai pop`)
This is the "killer feature." `ai pop` grabs your current `tmux` pane history, running processes, and env vars, and drops them into an AI session. You don't have to copy-paste errors; the AI is already looking at them.

### 🫧 Clean UI & Smart Reflow
Commands run in the background so they don't clutter your chat, but the AI still sees every bit of output. The TUI (powered by `ratatui`) handles window resizing perfectly without breaking the layout.

### 🧠 Agentic Loop (Think-Plan-Execute)
In interactive mode, the AI doesn't just talk. It uses a ReAct loop to:
1.  **Reason** about your request.
2.  **Plan** a multi-step solution.
3.  **Execute** shell commands, check git, or search the web.
*Everything is guarded by your approval.*

### 🖥 Context Awareness
`mylm` automatically tracks:
*   Your current directory (CWD).
*   Git status (branch, diffs, logs).
*   System info (OS, CPU, etc.).
*   Execution history.

### 🌐 Live Web Search & Crawling
Built-in tools for real-time searching and crawling. No more stale training data—if there’s a new library update, `mylm` can find the docs.

### 🔁 Multi-Provider Support
One interface for everything:
*   **Local**: Ollama, LM Studio.
*   **Cloud**: Gemini, OpenAI, Anthropic, DeepSeek.

### 🗂 Local Memory (RAG)
Includes a local vector database (LanceDB) to store project notes, past decisions, and technical references so the AI gets smarter the more you use it.

---

## Installation

### Prerequisites
*   **Rust** (if you don't have it, the installer will help).
*   **tmux** (highly recommended for the `pop` feature).

### Build from source
```bash
git clone https://github.com/ediblackk/mylm.git
cd mylm
chmod +x install.sh
./install.sh
```

The installer builds the binary locally and sets up the `ai` alias. We distribute as source-only because a tool with this much power over your shell should be transparent.

---

## Usage

*   **`ai`**: Opens the Hub (Pop, Resume, Interactive, Config).
*   **`ai pop`**: The "contextual" entry point.
*   **`ai "how do I fix my git history?"`**: Direct query.
*   **`ai interactive`**: Fresh start.

## Configuration
Settings are in `~/.config/mylm/mylm.yaml`. You can edit prompts, switch models, and manage API keys directly in the UI.

## Roadmap
*   **V2 Cognitive Engine**: Transitioning to a multi-layered worker architecture.
*   Background task queues.
*   Master-agent orchestration.

---

## Acknowledgements
Built on the shoulders of giants: **Rust, Linux, Git, ratatui, tokio, lancedb**, and the amazing research from **Google, Anthropic, OpenAI, Meta, and the Open Source AI community.**

---

## Keywords
Terminal AI, CLI LLM, Rust AI tool, Local LLM assistant, Ollama CLI, OpenAI terminal, Anthropic Claude CLI, Gemini terminal, Developer productivity, Command-line AI, tmux AI, Agentic Loop, ReAct Agent.
