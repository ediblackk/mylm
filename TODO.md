# V3 Architecture Implementation TODO

## 🎯 Architectural Foundation (CRITICAL - Do First)

These define the contracts. Do not implement until these are locked.

### 1. Core Contract Definitions

```rust
// File: core/src/agent_v3/contract/mod.rs
```

- [x] **KernelEvent** - Events that flow into the kernel
  - UserMessage, ToolResult, ApprovalOutcome, WorkerComplete, Interrupt
  - File: `core/src/agent_v3/contract/events.rs`
  
- [x] **Intent** - What the kernel wants to do
  - CallTool, RequestLLM, RequestApproval, SpawnWorker, EmitResponse, Halt
  - File: `core/src/agent_v3/contract/intents.rs`
  
- [x] **IntentNode** - Intent with DAG structure
  - id: IntentId, intent: Intent, dependencies: Vec<IntentId>, priority: Priority
  - File: `core/src/agent_v3/contract/intents.rs`
  
- [x] **IntentGraph** - DAG of intents emitted by kernel
  - builder pattern, ready_nodes(), is_complete()
  - File: `core/src/agent_v3/contract/graph.rs`
  
- [x] **Observation** - Result of intent execution
  - ToolCompleted, LLMCompleted, ApprovalGiven, WorkerCompleted, RuntimeError
  - File: `core/src/agent_v3/contract/observations.rs`

### 2. Core Traits

- [x] **AgencyKernel** - Pure, sync, deterministic
  ```rust
  fn init(&mut self, config: KernelConfig) -> Result<(), KernelError>;
  fn process(&mut self, events: &[KernelEvent]) -> Result<IntentGraph, KernelError>;
  fn state(&self) -> &AgentState;
  ```
  - File: `core/src/agent_v3/contract/kernel.rs`
  
- [x] **AgencyRuntime** - Async, executes side effects
  ```rust
  async fn execute(&self, intent: Intent) -> Result<Observation, RuntimeError>;
  async fn execute_dag(&self, graph: &IntentGraph) -> Result<Vec<(IntentId, Observation)>, RuntimeError>;
  ```
  - File: `core/src/agent_v3/contract/runtime.rs`
  
- [x] **EventTransport** - Pluggable event queue
  ```rust
  async fn next_batch(&mut self) -> Result<Vec<KernelEvent>, TransportError>;
  async fn publish(&mut self, event: KernelEvent) -> Result<(), TransportError>;
  ```
  - File: `core/src/agent_v3/contract/transport.rs`

### 3. Session with Dynamic DAG Expansion

- [x] **Session** - Orchestration layer
  ```rust
  pub struct Session<K, R, T> {
      kernel: K,
      runtime: R,
      transport: T,
      pending_graph: Option<IntentGraph>,
      completed_intents: HashSet<IntentId>,
  }
  ```
  - File: `core/src/agent_v3/contract/session.rs`
  
- [ ] **Dynamic expansion loop**
  ```rust
  loop {
      // 1. Get events from transport
      let events = transport.next_batch().await?;
      
      // 2. If no pending graph or need to expand, process with kernel
      if pending_graph.is_none() || should_expand(&events) {
          let new_intents = kernel.process(&events)?;
          merge_into_pending(&mut pending_graph, new_intents);
      }
      
      // 3. Execute ready intents from DAG
      let ready = pending_graph.ready_nodes(&completed_intents);
      let observations = runtime.execute_batch(ready).await?;
      
      // 4. Stream observations back to transport for potential expansion
      for (id, obs) in observations {
          completed_intents.insert(id);
          transport.publish(obs.into_event()).await?;
      }
      
      // 5. Check if graph complete
      if pending_graph.is_complete(&completed_intents) {
          pending_graph = None;
      }
  }
  ```

### 4. Event Envelope (for distributed/ordering)

- [x] **KernelEventEnvelope**
  ```rust
  pub struct KernelEventEnvelope {
      pub id: EventId,              // Monotonic/UUID
      pub source: NodeId,           // Which node produced it
      pub timestamp: LogicalClock,  // Lamport clock or sequence
      pub payload: KernelEvent,
  }
  ```
  - File: `core/src/agent_v3/contract/envelope.rs`

---

## 🔴 Phase 7.2: Agent Finishing - Connect Config to Agent ✅ COMPLETE

**Goal**: Wire the unified `Config` (profiles, providers) to the agent system so the agent can actually use configured LLMs.

### Current State
- ✅ Terminal/UI uses `Config::load_or_default()` (new unified config)
- ❌ Old Agent Factory uses `ConfigV2` (legacy)
- ❌ New Agent (V3) uses `KernelConfig`, `RuntimeConfig` in `contract/config.rs`
- ❌ LLM Client uses old `LlmConfig` from `core/src/config/agent.rs`

**Problem**: 3 different config systems that don't talk to each other.

### Tasks

#### 1. Create Config Bridge (`core/src/config/bridge.rs`)
- [x] `fn config_to_llm_config(config: &Config, profile: &str) -> LlmConfig`
  - Extract provider base_url, api_key from `config.providers`
  - Extract model, context_window from `config.profiles[profile]`
  - Build `LlmConfig` for LLM client
  
- [x] `fn config_to_kernel_config(config: &Config, profile: &str) -> KernelConfig`
  - Map profile settings to kernel policies
  - Max iterations, temperature, system prompt
  
- [x] `fn config_to_runtime_config(config: &Config) -> RuntimeConfig`
  - Rate limits, retry policies from config

#### 2. Create Agent Session Factory (`core/src/agent/factory.rs`)
- [x] `pub struct AgentSessionFactory`
- [x] `impl AgentSessionFactory`
  - `fn new(config: Config) -> Self`
  - `fn create_session(&self, profile: &str) -> Result<AgencySession, Error>`
    - Use bridge to create LLM config
    - Create `LlmClient` from config
    - Create `ContractRuntime` with tools + LLM
    - Create `CognitiveEngineAdapter` (kernel) with `LLMBasedEngine`
    - Create `InMemoryTransport`
    - Assemble and return `AgencySession`

#### 3. Update LLM Client Integration
- [x] Ensure `LlmClient` can be created from new provider system
- [x] Support provider presets with correct headers:
  - OpenAI: Standard `Authorization: Bearer`
  - Anthropic: `x-api-key` + `anthropic-version` headers
  - OpenRouter: `HTTP-Referer` + `X-Title` headers
  - Google: API key in URL
  - Ollama: No auth

#### 4. Wire Terminal Entry Point
- [x] Create simple REPL loop in main.rs using new factory
- [x] Connect user input → transport → session → response
- [ ] Remove old agent creation code (deferred - need to migrate TUI)

#### 5. Clean Up Legacy Config
- [ ] Remove `ConfigV2` from `core/src/factory.rs`
- [ ] Migrate all usages to unified `Config`

---

## 🔴 Critical Implementation (Blockers for MVP)

### 5. Refactor Existing V3 to Match Contract

- [ ] Remove async from `CognitiveEngine::step()` → rename to `process()`
- [ ] Change return from `Transition` to `IntentGraph`
- [ ] Remove any channels/broadcast from kernel
- [ ] Ensure `KernelConfig` contains only descriptors (no executors)
- [ ] Add `IntentId` generation to kernel state

### 6. Implement Event Transport Implementations

- [ ] **InMemoryTransport** - Single process, channels
- [ ] **ChannelTransport** - Multi-threaded
- [ ] **FileReplayTransport** - Deterministic replay for testing

### 7. Implement DAG Executor

- [ ] Execute intents respecting dependencies
- [ ] Parallel execution for independent intents
- [ ] Priority-based scheduling
- [ ] Streaming observations back

### 8. Working LLM Integration

```
File: runtime/impls/llm_client.rs
```

- [ ] Wire `LlmClientCapability` to actual `LlmClient`
- [ ] Handle streaming responses
- [ ] Token usage tracking
- [ ] Error handling and retries

### 9. Working Tool Registry

```
File: runtime/impls/tool_registry.rs
```

- [ ] Execute tools by name
- [ ] Type-safe argument passing
- [ ] Tool result formatting

---

## 🟡 Important (Needed for Production)

### 10. Real Worker Sessions

```
File: runtime/impls/local_worker.rs
```

- [ ] Workers spawn nested Session with own kernel+runtime
- [ ] Worker results properly feed back as WorkerCompleted events
- [ ] Worker lifecycle management (spawn, monitor, complete)

### 11. Web Search Response Parsing

```
File: runtime/impls/web_search.rs
```

- [ ] Parse Kimi/SerpAPI/Brave JSON responses
- [ ] Extract relevant results
- [ ] Handle errors gracefully

### 12. Vector Store for Memory

```
File: runtime/impls/memory.rs
```

- [ ] Integrate with existing VectorStore
- [ ] Semantic search
- [ ] Memory recording

### 13. Robust Response Parser

```
File: cognition/llm_engine.rs
```

- [ ] Parse JSON mode responses
- [ ] Handle multiple tool calls in one response
- [ ] Parse IntentGraph from LLM (for complex planning)
- [ ] Error recovery for malformed responses

### 14. Approval System

```
File: runtime/impls/terminal_approval.rs
```

- [ ] Policy-based approval (not hardcoded logic)
- [ ] Async approval flow
- [ ] Timeout handling

### 15. Configuration System

- [ ] Config files for kernel policies
- [ ] Per-profile settings
- [ ] Tool registry configuration
- [ ] Rate limits

---

## 🟢 Nice to Have (Polish)

### 16. Additional Transports

- [ ] WebSocketTransport
- [ ] RedisStreamTransport
- [ ] KafkaTransport

### 17. Advanced Features

- [ ] Streaming responses (token-by-token)
- [ ] Tool result caching
- [ ] Conversation persistence
- [ ] Time-travel debugging (event log replay)
- [ ] Multi-modal support (images)
- [ ] Plugin system for dynamic tools

---

## 📋 Implementation Order

### Phase 0: Contract (DO NOT SKIP) ✅ COMPLETE

**Contract Definitions:**
- [x] KernelEvent, Intent, IntentNode, IntentGraph, Observation
- [x] AgencyKernel trait (pure, sync, deterministic)
- [x] AgencyRuntime trait (async, side effects)
- [x] EventTransport trait (pluggable)
- [x] Session trait with dynamic DAG expansion

**Post-Review Fixes:**
- [x] **Deterministic IntentId**: Changed from counter to `IntentId::from_step(step_count, intent_index)`
  - High 32 bits = kernel step_count
  - Low 32 bits = intent index within graph
  - Ensures replay generates identical IDs
- [x] **Distributed Execution Semantics**: Added explicit rules
  - Single Leader (Session) owns DAG
  - Multiple Workers (Runtime) execute intents
  - At-least-once delivery, exactly-once execution via dedup
  - Intents are idempotent by contract
  - Leader crash = session dies (no consensus)
- [x] **Event Ordering**: Kernel only sees ordered events
  - Transport preserves FIFO per session
  - LogicalClock assigned before kernel

**All contract files:**
- `core/src/agent_v3/contract/mod.rs` - Module with distributed semantics docs
- `core/src/agent_v3/contract/ids.rs` - IntentId with deterministic generation
- `core/src/agent_v3/contract/events.rs` - KernelEvent types
- `core/src/agent_v3/contract/intents.rs` - Intent, IntentNode, Priority
- `core/src/agent_v3/contract/observations.rs` - Observation types
- `core/src/agent_v3/contract/config.rs` - KernelConfig, policies
- `core/src/agent_v3/contract/graph.rs` - IntentGraph, IntentGraphBuilder (at_step)
- `core/src/agent_v3/contract/envelope.rs` - KernelEventEnvelope
- `core/src/agent_v3/contract/kernel.rs` - AgencyKernel trait
- `core/src/agent_v3/contract/runtime.rs` - AgencyRuntime trait
- `core/src/agent_v3/contract/transport.rs` - EventTransport trait
- `core/src/agent_v3/contract/session.rs` - Session with distributed tracking

### Phase 1: MVP (IN PROGRESS)

**Completed:**
- [x] **Refactor existing V3 to match contract**
  - Created `kernel_adapter.rs` to bridge CognitiveEngine -> AgencyKernel
  - Adapter converts between old and new types
  
- [x] **Implement InMemoryTransport**
  - File: `core/src/agent_v3/runtime/impls/in_memory_transport.rs`
  - Single-process FIFO transport using mpsc channels
  - Supports batching, preserves ordering
  
- [x] **Implement DAG Executor**
  - File: `core/src/agent_v3/runtime/impls/dag_executor.rs`
  - Executes IntentGraph respecting dependencies
  - Parallel execution up to configurable limit
  - Handles ready intents, tracks completion

**In Progress:**
- [ ] **Config Bridge** - Connect unified Config to agent
- [ ] **Agent Session Factory** - Create sessions from config
- [ ] **Wire LLM client** - Started ContractRuntime, needs type bridging
- [ ] **Wire basic tools** - Depends on LLM wiring
- [ ] **Minimal CLI with Session** - Pending above
- [ ] **Integration tests** - Pending above

**Notes:**
- ContractRuntime skeleton created in `core/src/agent_v3/runtime/contract_runtime.rs`
- Type bridging between old V3 types and new contract types needed
- ToolRegistry uses old `ToolCall` type, contract uses new `ToolCall` type
- Similar type mismatches for LLM and Worker capabilities

### Phase 2: Production
```
├── 14. Real worker sessions
├── 15. Web search parsing
├── 16. Vector store integration
├── 17. Robust response parsing (JSON mode)
├── 18. Approval system
└── 19. Configuration system
```

### Phase 3: Scale
```
├── 20. Additional transports (WebSocket, Redis)
├── 21. Distributed worker support
├── 22. Streaming responses
├── 23. Persistence and replay
└── 24. Plugin system
```

---

## 🎯 Quick Wins (After Contract)

These can be done quickly once contract is stable:

- [ ] Fix web search JSON parsing (~50 lines)
- [ ] Create minimal CLI loop with Session (~100 lines)
- [ ] Wire LlmClientCapability (~30 lines)
- [ ] Add JSON mode response parser (~60 lines)

---

## 🚫 Anti-Patterns to Avoid

1. **NO async in kernel** - Keep it pure
2. **NO channels in kernel** - Transport is separate layer
3. **NO execution in kernel config** - Descriptors only
4. **NO direct UI access from kernel** - Everything through transport
5. **NO runtime state in kernel** - Kernel is reducer, not state owner
6. **NO hardcoded policies** - Inject as configuration

---

## ✅ Definition of Done

- [ ] Kernel is pure sync function
- [ ] Runtime executes async side effects
- [ ] Transport is pluggable
- [ ] Session orchestrates dynamic DAG expansion
- [ ] Same kernel runs in CLI, daemon, and tests
- [ ] Can replay session deterministically from event log
- [ ] Can swap LLM without touching kernel
- [ ] Can add new transport without touching kernel
- [ ] Can run distributed with shared transport

---

## 🎯 CURRENT FOCUS: MyLM-Core + Terminal Integration

**Date**: 2026-02-13
**Status**: Project compiles with warnings. Contract layer is solid. Need to wire TUI to new agent.

### Current Architecture State

```
mylm/                          # Root crate (Terminal UI + CLI)
├── src/
│   ├── main.rs               # Entry point - uses quick_query() with new factory ✅
│   ├── tui/                  # TUI implementation - STUBBED, needs wiring
│   └── ...
└── Cargo.toml

core/                          # MyLM-Core library
├── src/
│   ├── agent/                # ✅ NEW V3 Architecture (clean, working)
│   │   ├── contract/         # ✅ Core types & traits (AgencyKernel, etc.)
│   │   ├── cognition/        # ✅ Pure engine (LLMBasedEngine, etc.)
│   │   ├── runtime/          # ⚠️ Partial (stubs + some impls)
│   │   ├── session/          # ⚠️ Skeleton exists
│   │   ├── types/            # ✅ IntentId, Intent, etc.
│   │   ├── factory.rs        # ✅ AgentSessionFactory (used by quick_query)
│   │   └── ...
│   ├── agent_old/            # 🗑️ Legacy V1/V2 (to delete)
│   ├── config/               # ✅ Unified Config + bridge
│   ├── llm/                  # ✅ LLM client
│   └── ...
└── Cargo.toml
```

### What Works Now
1. ✅ `quick_query()` in `main.rs` uses new `AgentSessionFactory` - WORKING
2. ✅ Contract types are solid and tested
3. ✅ `IntentId::from_step()` deterministic generation
4. ✅ Config bridge connects unified Config to agent

### What's Blocked
1. ❌ TUI uses stubbed agent responses - needs wiring to `AgentSessionFactory`
2. ❌ `ContractRuntime` needs completion for tool execution
3. ❌ Some type mismatches between old and new types need cleanup

---

## 📝 Plan for Current Sprint

### Step 1: Complete ContractRuntime ✅ DONE
**Files**: `core/src/agent/runtime/contract_runtime.rs`

The `ContractRuntime` now fully implements the `AgencyRuntime` trait:
- ✅ `Intent::CallTool` - Executes via `ToolRegistry` with safety checks
- ✅ `Intent::RequestLLM` - Calls LLM via `LlmClientCapability`
- ✅ `Intent::RequestApproval` - Auto-approves (TODO: wire to terminal approval)
- ✅ `Intent::SpawnWorker` - Spawns workers via `LocalWorkerCapability`
- ✅ `Intent::EmitResponse` - Returns response observation
- ✅ `Intent::Halt` - Returns halt observation with reason
- ✅ Telemetry events emitted for all intents
- ✅ `execute_with_id()` method added to trait for proper intent tracking
- ✅ DAG executor updated to use `execute_with_id`

### Step 2: Wire TUI to New Agent ✅ DONE
**Files**: `src/tui/mod.rs`, `src/tui/stub.rs`, `core/src/agent/contract/session.rs`

TUI now uses real agent via `AgentSessionFactory` instead of stubbed responses.

**Changes Made:**

1. **`src/tui/stub.rs`** - Added session channels to `App`:
   - `output_rx: Option<broadcast::Receiver<OutputEvent>>` - receive events
   - `input_tx: Option<mpsc::Sender<UserInput>>` - send input
   - `pending_approval: Option<(u64, String, String)>` - track approvals
   - `current_response: String` - buffer for streaming
   - Added `set_session_channels()`, `submit_message()`, `submit_approval()` methods

2. **`src/tui/mod.rs`** - Complete rewrite of event loop:
   - Create `AgentSessionFactory` and session at startup
   - Get `output_rx` via `session.subscribe_output()`
   - Get `input_tx` via `session.input_sender()` (new method)
   - Spawn `session.run()` in background task
   - Use `tokio::select!` to handle crossterm + agent events
   - `handle_agent_event()` maps `OutputEvent` → `AppState`
   - `handle_key_event()` calls `app.submit_message()` on Enter
   - Approval keys (Y/N) call `app.submit_approval()`

3. **`core/src/agent/contract/session.rs`**:
   - Added `input_sender()` method to expose `input_tx` clone
   - Allows TUI to send input while session runs

4. **UI already supported** (`src/tui/ui.rs`):
   - `AppState::AwaitingApproval` shows "⚠️ Approve: {tool}? (Y/N)"
   - Status bar shows tool execution, thinking, streaming states

### Step 3: Cleanup Type Duplications ✅ DONE
**Files**: `core/src/agent/contract/kernel.rs`, `core/src/agent/worker.rs`, `core/src/agent/mod.rs`

**Changes Made:**

1. **Removed unused `Message` and `Role` from `contract/kernel.rs`**:
   - These types were defined but never used
   - `AgentState.history` now uses `Message` re-exported from `types/intents`

2. **Renamed `worker::WorkerSpec` to `WorkerSpawnParams`**:
   - Avoids confusion with `types/intents::WorkerSpec` (the contract type)
   - Added doc comment explaining the distinction
   - Updated all references in `worker.rs` and test code
   - Updated re-export in `agent/mod.rs`

**Type Architecture Now:**
```
core/src/agent/types/          # Single source of truth for all contract types
├── intents.rs                 # ToolCall, LLMRequest, WorkerSpec, ApprovalRequest
├── events.rs                  # KernelEvent, ToolResult, LLMResponse
└── observations.rs            # Observation

core/src/agent/cognition/      # Internal cognition types (layered architecture)
├── history.rs                 # Message, MessageRole (cognition-internal)
└── ...

core/src/agent/worker.rs       # WorkerSpawnParams (internal worker params)
```

**Note**: `cognition/history.rs::Message` is intentionally separate from `types/intents::Message` - the cognition layer uses its own internal representation while the contract types are for the kernel/runtime boundary.

### Step 4: Remove agent_old
**Files**: `core/src/agent_old/` (entire folder)

Once TUI is wired to new agent:
- Delete `core/src/agent_old/`
- Remove exports from `core/src/lib.rs`
- Remove legacy re-exports

### Step 5: Server Module
**Files**: `src/server/`

Create a WebSocket server that:
- Exposes `AgentSessionFactory` over WebSocket
- Allows external UIs to connect
- Documents the protocol

---

## ✅ Sprint Checklist

- [x] **Step 1**: ContractRuntime executes tools and LLM calls
- [x] **Step 2**: TUI shows real AI responses (not stubs)
- [x] **Step 3**: No duplicate types (WorkerSpec renamed, unused Message removed)
- [x] **Step 4**: `agent_old/` moved to `unused/` (already done by user)
- [ ] **Step 5**: WebSocket server runs and accepts connections

---

## 🎯 Success Criteria

**MyLM-Core** (backend library):
- ✅ Clean public API via `AgentSessionFactory`
- ✅ Pure kernel, async runtime, pluggable transport
- ✅ All tools execute correctly
- ✅ LLM calls work with configured providers

**Terminal** (dumb UI):
- ✅ TUI connects to core via factory
- ✅ Chat interface works with real AI
- ✅ Tool execution visible in UI
- ✅ Approval flow works (Y/N prompts)

**Server** (bridge):
- ✅ WebSocket server starts
- ✅ Protocol documented
- ✅ External clients can create sessions
- ✅ Events flow bidirectionally

---

## 🚧 Blockers & Risks

1. **Type bridging complexity** - May need to unify type hierarchies
2. **TUI state management** - Current stub doesn't handle agent states well
3. **Approval flow** - Needs careful UI/core separation

**Mitigation**: Small PRs, test each layer independently.
