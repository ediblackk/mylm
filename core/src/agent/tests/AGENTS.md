# Tests Module

**Purpose:** Integration and architecture tests for the agent system.

## Files

| File | Purpose |
|------|---------|
| `test_architecture.rs` | Architecture verification tests |
| `example_integration.rs` | Integration examples |
| `integration_tests.rs` | Full integration tests |
| `read_file_e2e.rs` | Read file tool end-to-end tests |
| `worker_tests.rs` | Worker system tests (disabled) |

## Running Tests

```bash
# Architecture tests
cargo test --lib agent::tests::test_architecture

# Integration tests
cargo test --lib agent::tests::integration_tests

# All agent tests
cargo test --lib agent::
```

## Test Architecture

Tests are organized by scope:
- **Architecture tests**: Verify module boundaries, no circular deps
- **Integration tests**: Test component wiring
- **E2E tests**: Test full workflows
