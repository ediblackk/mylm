# mylm (My Language Model)

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Terminal AI](https://img.shields.io/badge/Terminal-AI-blue.svg)](#)

**mylm** is a high-performance, globally available **Terminal AI Assistant** designed for power users of the command line. It provides a blazing-fast, Rust-powered interface to interact with your favorite LLMs while maintaining deep awareness of your terminal context.

Available as the simple `ai` command, it bridges the gap between natural language and shell execution.

> **Note on LLM Providers**: This application has been primarily tested and optimized for the **Google Gemini** API (specifically `gemini-3-pro-preview`). While **OpenAI**, **Anthropic**, and other providers are supported in the configuration, they may require further tuning. Full support is planned for future releases.

---

## 🚀 Key Features

- **Blazing Fast Performance**: Built with Rust for near-instant startup and minimal resource footprint.
- **Autonomous Agentic Loop**: A powerful interactive mode where the AI can think, plan, and execute multi-step tasks autonomously.
- **Live Web Search & Crawling**: Integrated tools allow the AI to search the web and crawl websites in real-time to gather the most up-to-date information.
- **Deep Terminal Context**: Automatically gathers relevant environment data—current directory, Git status, and system specs—to provide highly relevant answers.
- **Smart Command Execution**: Analyze, validate, and execute shell commands suggested by the AI with built-in safety guardrails.
- **Multi-Provider & Endpoint Support**: Seamlessly switch between local models (**Ollama**, **LM Studio**) and cloud providers (**OpenAI**, **Anthropic**, **Google Gemini**) using a unified interface.
- **Interactive Hub**: A powerful TUI (Terminal User Interface) to manage sessions, resume past conversations, and configure your workspace.
- **Persistent Memory (RAG)**: Store and retrieve local knowledge using integrated vector search.

---

## 🛡 Security Philosophy: Build from Source

`mylm` is intentionally distributed as source code to be compiled locally. Given its **ultra-deep integration** into your operating system and terminal, we believe users should have the opportunity to:
1. **Audit the codebase**: Use an AI in your IDE or manual review to scan for malicious patterns.
2. **Verify Dependencies**: Inspect the `Cargo.toml` and lockfile before execution.
3. **Control the Build**: Ensure the binary you run is exactly what you see in the source.

This "transparency-first" approach ensures that you remain the ultimate authority over what runs in your terminal.

---

## 🛠 Prerequisites

### Supported Platforms
- **Linux**: Primary focus (Optimized for performance).
- **macOS**: Fully supported.
- **Windows**: Support is currently in development.

### Build Dependencies (Linux)
Beyond Rust and Cargo, ensure the following system libraries and tools are installed for compiling native dependencies and optimizing build times:

**Ubuntu/Debian:**
```bash
sudo apt-get install pkg-config libssl-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev clang sccache
```

**Fedora:**
```bash
sudo dnf install pkgconf-pkg-config openssl-devel libxcb-devel clang sccache
```

**Arch Linux:**
```bash
sudo pacman -S pkgconf openssl libxcb clang sccache
```

*Note: `sccache` is highly recommended for faster subsequent builds.*

---

## 📦 Installation

### Local Installation (Recommended)
Since this project is designed to be built from source, we recommend cloning the repository and running the included installation script. This script handles dependency checks, building the binary, and setting up the environment.

1. **Clone the repository:**
   ```bash
   git clone https://github.com/ediblackk/mylm.git
   cd mylm
   ```

2. **Run the installation script:**
   ```bash
   chmod +x install.sh
   ./install.sh
   ```

   > **Note:** Currently, this project is in active development. `install.sh` will automatically invoke `./dev-install.sh` to produce a **debug build**. This ensures faster compilation times for frequent updates. Once the core features are stable, `install.sh` will default back to optimized release builds.

   **What the script does:**
1. **Dependency Check**: Automatically detects your Linux distribution and installs required build tools (`clang`, `pkg-config`, `openssl`, etc.) and Rust/Cargo if missing.
2. **Version Awareness**: Compares your installed version with the current source to suggest updates or skip redundant builds.
3. **Fresh Installation**: Performs a clean build, sets up the `ai` alias, and runs the configuration wizard.
4. **Seamless Update**: Updates the binary while preserving your existing profiles, configuration, and aliases.
5. **Optimization**: Configures `sccache` support if available for lightning-fast subsequent compilations.

---

## 📖 Usage Guide

### 1. The Interactive Hub & Agentic Mode
Start the main interaction menu to resume sessions or explore features:
```bash
ai
```
Or jump directly into the full **Agentic TUI**:
```bash
ai interactive
```

In this mode, `mylm` operates in a "Think-Plan-Execute" loop. It can:
- **Run Shell Commands**: Safely execute terminal commands to solve problems.
- **Web Search**: Query the internet for real-time data using integrated search engines.
- **Crawl Websites**: Extract content from specific URLs to provide deep analysis.
- **Access Memory**: Store and retrieve information from its persistent vector database.

### 2. Direct Queries
Ask questions directly from your shell. Your current context is automatically analyzed:
```bash
ai "how do I revert the last three git commits safely?"
```

### 3. Smart Command Execution
Analyze and run a specific command with AI safety checks:
```bash
ai execute "find . -name '*.tmp' -exec rm {} +"
```

### 4. Switching Providers
Override your default model/endpoint on the fly:
```bash
ai -e openai "Write a python script to parse this directory's logs"
```

---

## ⚙️ Configuration

`mylm` uses a YAML-based configuration located at `~/.config/mylm/mylm.yaml`.

### Managing Profiles
You can manage your endpoints and prompts interactively:
```bash
ai config edit prompt  # Edit your global AI instructions
ai config select       # Switch between configured provider profiles
```

### Example Profile Structure
```yaml
default_endpoint: local-ollama
endpoints:
  - name: local-ollama
    provider: openai # (Ollama supports OpenAI-compatible API)
    base_url: http://localhost:11434/v1
    model: llama3.2
    api_key: none

  - name: anthropic-claude
    provider: anthropic
    model: claude-3-5-sonnet-latest
    api_key: ${ANTHROPIC_API_KEY}
```

---

## 🛡 License & Safety

Distributed under the **MIT License**.

**Note on Command Execution:** While `mylm` includes safety analysis, always review commands before execution. Use the `--dry-run` flag with the `execute` subcommand to see what would happen without making changes.

---

## 🔍 SEO Keywords
Terminal AI assistant, CLI LLM, Rust AI tool, Ollama terminal, OpenAI CLI, Local LLM assistant, Command line AI, Anthropic Claude CLI, Google Gemini terminal, Developer productivity tools.

---

### 🌟 Special Thanks & Acknowledgements
*   **The Rust Team**: For the language that makes this possible.
*   **VSCode Team**: For the editor environment.
*   **Google DeepMind & The Gemini Team (Worldwide)**: For the intelligence powering the agent (`gemini-3-pro-preview`).
*   **Linux & Git**: For the foundation of our workflow.
*   **Open Source Community**: Special thanks to the authors of `ratatui`, `tokio`, `portable-pty`, `serde`, `clap`, `lancedb`, and all other dependencies used in this project.

**Global AI Innovations:**
We acknowledge the global community of researchers and engineers advancing the frontier of intelligence:

*   **OpenAI**: Creators of ChatGPT and GPT-5.
*   **Anthropic**: Creators of Claude and Constitutional AI.
*   **DeepSeek**: Pushing boundaries in efficient AI and coding.
*   **Meta AI**: For Llama and open research.
*   **Hugging Face**: The democratizing force of the AI community.
*   **Zhipu AI**: Creators of the GLM (General Language Model) series.
*   **Moonshot AI**: For Kimi and long-context breakthroughs.
*   **Minimax**: For creators of Minimax M2.1 and free usage
