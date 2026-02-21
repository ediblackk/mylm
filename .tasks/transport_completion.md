# Transport System Completion TODO

## Overview
Complete the EventTransport abstraction by implementing missing transports and adding production-ready features.

## Current State
- ✅ `EventTransport` trait defined in `orchestrator/transport.rs`
- ✅ `InMemoryTransport` - single process (in `capabilities/transport.rs`)
- ✅ `CompositeTransport` - multiple backends
- ❌ `ChannelTransport` - cross-thread
- ❌ `FileTransport` - persistence/replay
- ❌ `WebSocketTransport` - distributed (future)

---

## Phase 1: Module Restructure

**Goal:** Clean up transport organization

- [ ] Create `orchestrator/transport/` directory
- [ ] Move trait definition to `orchestrator/transport/mod.rs`
- [ ] Move `InMemoryTransport` to `orchestrator/transport/memory.rs`
- [ ] Move `CompositeTransport` to `orchestrator/transport/composite.rs`
- [ ] Update all imports in codebase
- [ ] Verify `cargo check` passes

---

## Phase 2: ChannelTransport

**Goal:** Cross-thread event passing within same process

- [ ] Create `orchestrator/transport/channel.rs`
- [ ] Implement using `tokio::sync::broadcast` for multi-consumer
- [ ] Support backpressure with bounded channels
- [ ] Add configuration: buffer_size, timeout
- [ ] Write unit tests for send/receive
- [ ] Benchmark: throughput vs InMemoryTransport

**Use Case:** Worker pool pattern - main thread broadcasts to N worker threads

---

## Phase 3: FileTransport (HIGH PRIORITY)

**Goal:** Persistence, replay, and debugging

- [ ] Create `orchestrator/transport/file.rs`
- [ ] Implement append-only log format (JSON Lines)
- [ ] Add log rotation (by size or time)
- [ ] Implement replay mode (read-only)
- [ ] Add compression option (gzip)
- [ ] Ensure atomic writes (flush to disk)
- [ ] Add seek/position for resuming replay
- [ ] Write integration tests

**Use Case:** 
- Debug production issues by replaying exact event sequence
- Audit trail for compliance
- Resume crashed sessions from last checkpoint

**File Format:**
```
# events.log
{"event_id":1,"timestamp":"...","payload":{"UserMessage":"hello"}}
{"event_id":2,"timestamp":"...","payload":{"ToolCompleted":"..."}}
```

---

## Phase 4: Observability & Metrics

**Goal:** Production monitoring

- [ ] Add `TransportMetrics` struct:
  - events_published_count
  - events_received_count
  - bytes_transferred
  - error_count
  - latency_histogram
- [ ] Implement metrics collection in each transport
- [ ] Add `subscribe_metrics()` method
- [ ] Export to Prometheus/OpenTelemetry (optional)

---

## Phase 5: Backpressure & Reliability

**Goal:** Handle overload gracefully

- [ ] Implement bounded channels (drop vs block policy)
- [ ] Add slow consumer detection
- [ ] Implement circuit breaker pattern
- [ ] Add retry logic with exponential backoff
- [ ] Dead letter queue for failed events

---

## Phase 6: Serialization

**Goal:** Efficient event encoding

- [ ] Add `Serializer` trait (JSON, CBOR, MessagePack)
- [ ] Implement JSON serializer (human readable)
- [ ] Implement CBOR serializer (binary, compact)
- [ ] Add schema versioning for compatibility
- [ ] Compression support (zstd, gzip)

---

## Phase 7: Testing Infrastructure

**Goal:** Confidence in transport layer

- [ ] Create `MockTransport` for unit testing
- [ ] Add chaos testing (random delays, drops)
- [ ] Property-based tests (event ordering)
- [ ] Benchmark suite (throughput, latency)
- [ ] Integration test: FileTransport replay accuracy

---

## Phase 8: Documentation

**Goal:** Clear usage guide

- [ ] Transport selection flowchart
- [ ] Configuration examples
- [ ] Performance characteristics table
- [ ] Migration guide (InMemory → File → Distributed)

---

## Priority Order

1. **Phase 3 (FileTransport)** - Most valuable for debugging
2. **Phase 1 (Restructure)** - Cleanup before expansion
3. **Phase 7 (Testing)** - Ensure reliability
4. **Phase 2 (ChannelTransport)** - Multi-thread support
5. **Phase 4-6** - Production polish

---

## Success Criteria

- [ ] Can replay a session from file log identically
- [ ] Can run with 10k events/sec without loss
- [ ] All transports have >80% test coverage
- [ ] Documentation covers when to use each transport

---

## Notes

- Keep trait simple - only `next_batch`, `publish`, `flush`, `close`
- Transport is NOT responsible for ordering - Orchestrator handles that
- FileTransport should be the default for production (audit trail)
- InMemoryTransport stays for testing only
