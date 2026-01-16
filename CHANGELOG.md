# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-alpha] - 2026-01-16

### Added
- **Interactive Terminal UI**: Full TUI interface powered by `ratatui` with support for window resizing.
- **Persistent Memory**: Local vector database (LanceDB) integration for long-term context retention.
- **Agentic Loop**: ReAct-based reasoning engine (Think-Plan-Execute).
- **`ai pop`**: Context injection from `tmux` pane history, environment variables, and running processes.
- **Multi-Provider Support**: Unified interface for OpenAI, Anthropic, Gemini, DeepSeek, and local models (Ollama).
- **Tool Framework**: Extensible system for web search, crawling, and shell execution.
- **Web Search & Crawl**: Built-in tools for real-time information gathering.
- **Configuration Management**: YAML-based config for prompts, models, and API keys.

### Known Issues
- Parsing issues with certain OpenAI models.
- Memory consolidation process is still being refined.
- Documentation is currently minimal.
