# Agent V3 Implementation Plan

## Phase 1: MVP (Minimum Viable Product)

### 1.1 Working CLI/Main Entry Point
**Priority**: Critical  
**Estimated Time**: 1-2 hours  
**Files**: `src/main.rs`

- [x] Interactive REPL loop
- [x] Config loading (reuse existing ConfigManager)
- [x] Session creation with real capabilities
- [x] Command parsing (chat, /quit, /tools, /clear)
- [x] Terminal UI integration

**Acceptance Criteria**:
- User can run `cargo run` and chat with the agent
- Agent responds using real LLM
- Basic commands work (/quit, etc.)

---

### 1.2 LLM Integration Testing & Fixes
**Priority**: Critical  
**Estimated Time**: 2-3 hours  
**Files**: `core/src/agent_v3/runtime/impls/llm_client.rs`, `core/src/llm/`

- [x] Test LlmClientCapability with existing LlmClient
- [x] Fix any integration issues
- [x] Ensure proper error handling
- [x] Add logging for LLM calls

**Acceptance Criteria**:
- LLM calls work end-to-end
- Errors are properly propagated
- Response is correctly passed to cognition layer

---

### 1.3 Web Search Implementation
**Priority**: Critical  
**Estimated Time**: 1 hour  
**Files**: `core/src/agent/runtime/tools/web_search.rs`, `core/src/llm/client.rs`

- [x] DuckDuckGo search (free, no API key)
- [x] SerpAPI search support
- [x] Brave search support
- [x] Kimi (Moonshot AI) builtin web search via `$web_search` function
- [x] Circuit breaker to prevent endless retries
- [x] API key testing in settings

**Kimi Web Search Architecture**:
Kimi doesn't expose a standalone search API. Instead, web search works as a builtin function:
1. When `web_search_enabled=true` for MoonshotKimi provider, `$web_search` tool is registered
2. Model returns `finish_reason: "tool_calls"` with `$web_search` when it wants to search
3. Client echoes back the arguments via `role: "tool"` message
4. Model performs the search internally and returns results in the same response

**Acceptance Criteria**:
- Web search returns actual results
- Results are formatted as text for LLM
- Graceful handling of API errors
- Kimi web search works natively through LLM client

---

### 1.4 JSON Mode Response Parser
**Priority**: High  
**Estimated Time**: 2 hours  
**Files**: `core/src/agent_v3/cognition/llm_engine.rs`

- [ ] Support JSON format: `{"tool": "name", "args": "..."}`
- [ ] Support function calling format
- [ ] Handle multiple tool calls
- [ ] Better error messages for malformed responses

**Acceptance Criteria**:
- LLM can respond with JSON instead of XML
- Parser handles both formats
- Clear error messages when parsing fails

---

### 1.5 Integration Tests
**Priority**: High  
**Estimated Time**: 2 hours  
**Files**: `core/src/agent_v3/integration_tests.rs`

- [x] End-to-end test with mock LLM
- [x] End-to-end test with real LLM (optional)
- [x] Tool execution flow test
- [x] Error recovery test

**Acceptance Criteria**:
- All integration tests pass
- Tests cover main use cases
- Tests can run in CI

---

## Phase 2: Production Ready

### 2.1 Real Worker Sessions
**Priority**: High  
**Estimated Time**: 4-6 hours  
**Files**: `core/src/agent_v3/runtime/impls/local_worker.rs`, `core/src/agent_v3/session/`

- [ ] Worker creates nested Session
- [ ] Worker gets its own engine + runtime
- [ ] Worker results feed back to parent session
- [ ] Worker lifecycle management (spawn, monitor, cleanup)

**Acceptance Criteria**:
- Worker can spawn and complete tasks
- Parent session receives worker results
- Workers can use tools

---

### 2.2 Configuration System
**Priority**: High  
**Estimated Time**: 3-4 hours  
**Files**: `core/src/agent_v3/config.rs` (new)

- [ ] AgentConfig struct
- [ ] Load from file/env
- [ ] Tool approval policies
- [ ] Rate limiting config
- [ ] Model selection per capability

**Acceptance Criteria**:
- Config loads from `~/.config/mylm/agent.toml`
- Per-profile settings work
- Hot reload support (optional)

---

### 2.3 Error Recovery & Retries
**Priority**: Medium  
**Estimated Time**: 3 hours  
**Files**: `core/src/agent_v3/runtime/impls/retry.rs`

- [ ] Exponential backoff for LLM calls
- [ ] Circuit breaker for failing tools
- [ ] Graceful degradation
- [ ] Retry policies per capability

**Acceptance Criteria**:
- Failed LLM calls retry automatically
- Rate limits handled gracefully
- Clear error messages to user

---

### 2.4 Vector Store for Memory
**Priority**: Medium  
**Estimated Time**: 4-6 hours  
**Files**: `core/src/agent_v3/runtime/impls/memory.rs`

- [ ] Embedding generation (use fastembed or API)
- [ ] Vector storage (in-memory for now)
- [ ] Similarity search
- [ ] Memory retrieval in prompts

**Acceptance Criteria**:
- Memories are stored with embeddings
- Relevant memories retrieved based on context
- Memory-augmented prompts work

---

## Phase 3: Polish

### 3.1 Streaming Responses
**Priority**: Low  
**Estimated Time**: 4 hours  
**Files**: `core/src/agent_v3/runtime/impls/llm_client.rs`, `core/src/agent_v3/session/`

- [ ] SSE streaming support
- [ ] Stream tokens to UI
- [ ] Cancel streaming

### 3.2 Tool Result Caching
**Priority**: Low  
**Estimated Time**: 2 hours  
**Files**: `core/src/agent_v3/runtime/impls/tool_registry.rs`

- [ ] Cache expensive operations
- [ ] TTL for cache entries
- [ ] Cache invalidation

### 3.3 Conversation Persistence
**Priority**: Low  
**Estimated Time**: 3 hours  
**Files**: `core/src/agent_v3/session/`

- [ ] Save conversation state
- [ ] Resume interrupted sessions
- [ ] Export/import conversations

### 3.4 Plugin System
**Priority**: Low  
**Estimated Time**: 8+ hours  
**Files**: New module

- [ ] Dynamic tool loading
- [ ] WASM plugin support (optional)
- [ ] Plugin registry

---

## Current Status

| Phase | Item | Status | Priority |
|-------|------|--------|----------|
| 1.1 | CLI/Main | 🟢 Complete | Critical |
| 1.2 | LLM Integration | 🟢 Complete | Critical |
| 1.3 | Web Search Parsing | 🟢 Complete | Critical |
| 1.4 | JSON Parser | 🟢 Complete | High |
| 1.5 | Integration Tests | 🟢 Complete | High |
| 2.1 | Worker Sessions | 🟡 Stub | High |
| 2.2 | Configuration | 🔴 Not Started | High |
| 2.3 | Error Recovery | 🟡 Basic | Medium |
| 2.4 | Vector Store | 🟡 Interface | Medium |

**Legend**:
- 🔴 Not Started
- 🟡 Partial/In Progress
- 🟢 Complete

---

## Implementation Order

1. **Start with 1.1** - CLI gives immediate feedback
2. **Then 1.2** - Ensures LLM works
3. **Then 1.3** - Adds web capability
4. **Then 1.4** - Improves reliability
5. **Then 1.5** - Ensures stability
6. **Then 2.x** - Production features

---

## Daily Checklist Template

```markdown
## Day X - [Date]

### Today:
- [ ] Item 1
- [ ] Item 2

### Blockers:
- None

### Notes:
- 
```
