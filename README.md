# mylm (My Language Model)

A globally available, high-performance terminal AI assistant designed for developers. `mylm` (available as the `ai` command) provides instant access to LLMs with terminal context awareness and safe command execution capabilities.

## 🚀 Features

- **Context-Aware Queries**: Automatically includes current directory, git status, and system info in your prompts.
- **Safe Command Execution**: Analyze terminal commands before running them.
- **Multi-Endpoint Support**: Easily switch between Ollama, OpenAI, LM Studio, or any OpenAI-compatible API.
- **Blazing Fast**: Written in Rust for maximum performance and minimal footprint.
- **Interactive Setup**: Easy configuration with `ai setup`.
- **Multi-Provider**: Supports OpenAI, Google Gemini, OpenRouter, and local models (Ollama/LM Studio).

## 🛠 Prerequisites

- **Rust**: [Install Rust and Cargo](https://www.rust-lang.org/tools/install) (version 1.70+)
- **LLM Endpoint**: A running instance of [Ollama](https://ollama.com/), [LM Studio](https://lmstudio.ai/), or an OpenAI API key.

## 📦 Installation

### Interactive Installation (Recommended)

Run the installation script to build, install, and configure `mylm`:

```bash
chmod +x install.sh
./install.sh
```

The script will:
1. Build the binary in release mode.
2. Install/Update it to `/usr/local/bin/ai`.
3. Prompt you to configure your LLM provider (this step is skipped if a configuration already exists, unless you choose to overwrite).

### 🔄 Upgrading
To upgrade to the latest version, simply re-run the `install.sh` script. Your existing configuration in `~/.config/mylm/mylm.yaml` will be preserved by default.

### Manual Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/youruser/mylm.git
   cd mylm
   ```
2. Build for release:
   ```bash
   cargo build --release
   ```
3. Install to your path:
   ```bash
   sudo cp target/release/mylm /usr/local/bin/ai
   ```
4. Create the configuration file at `~/.config/mylm/mylm.yaml` (see example below).

## ⚙️ Configuration

The configuration file is located at `~/.config/mylm/mylm.yaml`. 

### Example Configuration

```yaml
default_endpoint: ollama
endpoints:
  - name: ollama
    provider: openai
    base_url: http://localhost:11434/v1
    model: llama3.2
    api_key: none
    timeout_seconds: 60

  - name: openai
    provider: openai
    base_url: https://api.openai.com/v1
    model: gpt-4o
    api_key: sk-your-api-key-here
    timeout_seconds: 60

commands:
  allow_execution: false
  allowlist_paths: []
```

## 📖 Usage Examples

### 1. General Queries
Ask anything directly (no subcommand needed). Your terminal context is automatically included:
```bash
ai how do I revert the last git commit?
```

### 2. Interactive Setup
Reconfigure your LLM endpoint at any time with a guided wizard:
```bash
ai setup
```

### 3. Command Analysis & Execution
Analyze a command for safety and intent:
```bash
ai execute "find . -name '*.log' -delete"
```

### 3. Check Context
See exactly what context is being sent to the AI:
```bash
ai context
```

### 4. Switch Endpoints
Use a specific endpoint for a single query:
```bash
ai -e openai explain this complex regex
```

### 5. System Info
Get a quick summary of your system as seen by the AI:
```bash
ai system --brief
```

## 🔍 Troubleshooting

### Command Conflict (e.g., Open Interpreter)
If running `ai` opens another tool (like Open Interpreter), it's likely because `ai` is aliased in your shell. Check with:
```bash
alias ai
```
If it is aliased, you can remove it by editing your `.bashrc`, `.zshrc`, or `.bash_profile` and removing the line `alias ai='...'`. You can also temporarily unalias it with:
```bash
unalias ai
```

## 🛡 License

MIT
