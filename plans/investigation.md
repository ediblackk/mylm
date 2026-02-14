# Infinite Retry Loop Investigation

## Current Behavior
When the LLM provider returns a 400 error (e.g., context too long, invalid request):
1. The error propagates up from `LlmClient` to `ContractRuntime`.
2. `ContractRuntime` catches it and returns an `Observation::RuntimeError`.
3. The `Session` sees the `Observation` and publishes it as a `KernelEvent::RuntimeError`.
4. The `LLMBasedEngine` (kernel) receives the `RuntimeError`.
5. **CRITICAL ISSUE**: The engine treats the error as an input event and asks the LLM "What should I do?".
6. The LLM call fails again with the same 400 error.
7. Repeat infinite loop.

## Root Causes

1. **`LLMBasedEngine` Response to Errors**:
   - In `core/src/agent/cognition/llm_engine.rs`, `InputEvent::RuntimeError` triggers an exit:
     ```rust
     Some(InputEvent::RuntimeError { error, .. }) => {
         // ...
         Ok(Transition::exit(
             state.clone(),
             AgentExitReason::Error(format!("Runtime error: {}", error))
         ))
     }
     ```
   - This returns `AgentDecision::Exit(AgentExitReason::Error(...))`.

2. **`KernelAdapter` Conversion**:
   - In `core/src/agent/cognition/kernel_adapter.rs`:
     ```rust
     AgentDecision::Exit(reason) => {
         crate::info_log!("[KERNEL_ADAPTER] Exit decision: {:?}", reason);
         Intent::Halt(match reason {
             AgentExitReason::Error(msg) => ExitReason::Error(msg),
             // ...
         })
     }
     ```
   - It returns `Intent::Halt(ExitReason::Error(...))`.

3. **`Session` Loop Logic Flaw**:
   - In `core/src/agent/contract/session.rs`, `run()` loop calls `process_events`.
   - `process_events` -> `kernel.process` -> returns graph with `Intent::Halt`.
   - `Session` merges this into `pending_graph`.
   - `execute_ready_intents` executes `Intent::Halt`.
   - `ContractRuntime` returns `Observation::Halted`.
   
   **BUT**: The `execute_ready_intents` function also has this logic:
   ```rust
   // Check for LLM/runtime errors
   if let Observation::RuntimeError { error, .. } = obs {
       has_error = true;
       // ...
       let _ = self.output_tx.send(OutputEvent::Error { ... });
   }
   ```
   And `Session::run` loop:
   ```rust
   // Wait for either transport events or user input
   tokio::select! {
       batch = self.transport.next_batch() => {
           // ...
           let new_graph = self.process_events(events).await?;
           // ...
           let observations = self.execute_ready_intents().await?;
           
           // Check for halt observation
           for (_, obs) in &observations {
               if matches!(obs, Observation::Halted { .. }) {
                   return Ok(SessionResult { ... });
               }
           }
           
           // Convert to events and publish back to transport
           for (_, obs) in observations {
               self.publish_event(obs.into_event()).await?;
           }
       }
   }
   ```

   **The Loop**:
   1. `LlmClient` fails -> `Observation::RuntimeError`.
   2. `Session` publishes `KernelEvent::RuntimeError`.
   3. Transport sends it back to `Session` (next batch).
   4. `Session` calls `kernel.process(RuntimeError)`.
   5. `Kernel` (via Adapter & Engine) returns `Intent::Halt`.
   6. `Session` executes `Intent::Halt`.
   7. `Runtime` returns `Observation::Halted`.
   8. `Session` sees `Observation::Halted` and returns `Ok`.

   **So why isn't it stopping?**

   **Hypothesis E**: The `RuntimeError` is NOT being generated as `Observation::RuntimeError`.
   - `LlmClientCapability` (core/src/agent/runtime/impls/llm_client.rs):
     ```rust
     Err(e) => Err(LLMError::new(e.to_string())),
     ```
   - `ContractRuntime` (core/src/agent/runtime/contract_runtime.rs):
     ```rust
     .map_err(|e| RuntimeError::LLMRequestFailed { ... })?;
     ```
   - `DagExecutor` (core/src/agent/runtime/impls/dag_executor.rs):
     ```rust
     Ok((id, Err(e))) => {
         errors.push((id, e));
     }
     ```
   - `ContractRuntime::execute_dag`:
     ```rust
     observations.push((
         *intent_id,
         Observation::RuntimeError { ... },
     ));
     ```
   
   It seems the path to `Observation::RuntimeError` is solid.

   **Hypothesis F**: The `KernelAdapter` implementation of `process` is flawed.
   ```rust
   fn process(&mut self, events: &[KernelEvent]) -> Result<IntentGraph, KernelError> {
       // ...
       for event in events {
           // ...
           if let Some(input) = self.convert_event(event) {
               let transition = self.engine.step(&self.state, Some(input))?;
               // ...
               if !matches!(transition.decision, AgentDecision::None) {
                    // Adds intent to graph
               }
           }
       }
       // ...
   }
   ```
   - If `convert_event` returns `Some(InputEvent::RuntimeError)`, `engine.step` should return `Exit`.
   - `convert_decision` should return `Intent::Halt`.
   
   **Hypothesis G**: `InputEvent::RuntimeError` handler in `LLMBasedEngine` was added RECENTLY and might not be compiled/running in the version exhibiting the bug?
   - The file content I read shows it exists.
   
   **Hypothesis H**: The `LlmClient` error is NOT a runtime error but a `ToolResult::Error`?
   - No, LLM calls are `Intent::RequestLLM`, not `Intent::CallTool`.
   
   **Hypothesis I**: The infinite loop is happening inside `LlmClient`'s retry logic despite `is_retryable_error` logic?
   - `RetryLLM` calls `is_retryable_error`.
   - If `400` is returned, it returns `Err`.
   
   **Hypothesis J**: The error message does NOT contain "400 bad request" or other strings checked in `is_retryable_error`?
   - `is_retryable_error` (core/src/agent/runtime/impls/retry.rs):
     ```rust
     if msg.contains("400 bad request") ...
     ```
   - If the error from `reqwest` is just "HTTP status client error (400 Bad Request)" maybe it matches.
   - But if it's "context length exceeded" (often 400), does it match?
     ```rust
     || msg.contains("context length")
     ```
   - If it DOES NOT match, `RetryLLM` retries.
   - `RetryConfig` defaults to 3 retries.
   - It sleeps between retries.
   - This would delay for a few seconds, but eventually fail.
   - This fits the "UI Freezing" description if the UI is blocked during sleep (which uses `tokio::time::sleep`, so it shouldn't block unrelated tasks unless they are on the same thread/runtime without yield points).
   
   **Hypothesis K**: The `LlmClient` logic for 400 errors:
   - `retry_with_backoff`:
     ```rust
     if status.is_client_error() ... { return Ok(response); }
     ```
   - It returns success to `chat_openai`.
   - `chat_openai`:
     ```rust
     status => { bail!("API request failed ({}): {}", status, error_msg); }
     ```
   - Returns `Err`.
   - `RetryLLM` sees `Err`.
   - Checks `is_retryable_error`.
   - If `true`, it retries.
   - If `false`, it returns `Err` immediately.
   
   **CRITICAL**: If `is_retryable_error` returns `true` for a 400 error, `RetryLLM` will retry it 3 times.
   - AND `LLMBasedEngine` might be receiving a generic error message that it tries to "fix" by asking the LLM again?
   
   **Wait**, if `RetryLLM` fails after retries, it returns `Err`.
   - `ContractRuntime` returns `Observation::RuntimeError`.
   - `Session` passes to Kernel.
   - Kernel exits.
   - Session halts.
   
   **So where is the loop?**
   Maybe the `Session` loop logic regarding `pending_graph`?
   ```rust
   // Check if graph complete
   if let Some(ref graph) = self.pending_graph {
       if graph.is_complete(...) {
           // ...
           self.pending_graph = None;
       }
   }
   ```
   - If `Observation::RuntimeError` is returned, the intent is marked as completed (failed).
   - Graph becomes complete.
   - `pending_graph` = None.
   - `Session` waits for events.
   - `Observation::RuntimeError` event arrives.
   - Kernel processes it -> returns `Halt`.
   - `pending_graph` = Some(Halt).
   - `execute_ready_intents` -> executes Halt -> `Observation::Halted`.
   - `Session` loop checks `Observation::Halted` -> returns.
   
   **Unless...** `LLMBasedEngine` does NOT return `Exit` for `RuntimeError`.
   - Let's re-read `core/src/agent/cognition/llm_engine.rs`.
   
   ```rust
   // Runtime error - exit with error instead of retrying infinitely
   Some(InputEvent::RuntimeError { error, .. }) => {
       crate::info_log!("[LLM_ENGINE] RuntimeError received: {}. Exiting.", error);
       Ok(Transition::exit(
           state.clone(),
           AgentExitReason::Error(format!("Runtime error: {}", error))
       ))
   }
   
   // Default - no action (was causing infinite loop by requesting LLM)
   _ => {
       Ok(Transition::new(state.clone(), AgentDecision::None))
   }
   ```
   - If `InputEvent` is NOT `RuntimeError` (e.g. if `convert_event` fails), it hits `_`.
   - `AgentDecision::None`.
   - `KernelAdapter` converts `None` to `Intent::Halt(ExitReason::Completed)`.
   - Session halts.
   
   **Is it possible that `InputEvent::RuntimeError` case is NOT causing an exit?**
   - The code says `Transition::exit`.
   
   **Maybe the Error is not `RuntimeError`?**
   - If `RetryLLM` swallows the error? No.
   
   **Maybe `LlmClient` is the one looping infinitely?**
   - `retry_with_backoff` loop.
   - `loop { ... }`.
   - `max_retries = 5`.
   - It breaks if `attempt >= max_retries`.
   - It sleeps.
   - This isn't infinite.
   
   **What if `LLMBasedEngine` asks the LLM again?**
   - The user report says: "Investigate the cause of the infinite retry loop... when an LLM provider returns a 400 error."
   - If the code I'm reading ALREADY HAS the fix (`RuntimeError -> Exit`), then the code I'm looking at might be newer than what the user thinks, OR the fix isn't working.
   
   **Let's assume the fix isn't working.**
   - Why would `RuntimeError` be treated as something else?
   - In `KernelAdapter::convert_event`:
     ```rust
     KernelEvent::RuntimeError { intent_id, error } => {
         // ...
         Some(InputEvent::RuntimeError { ... })
     }
     ```
   
   **Hypothesis L**: The `LlmClient` error message `e.to_string()` contains "400" but `is_retryable_error` logic is flawed or the message format is different.
   
   **Hypothesis M**: The `Session` receives `Observation::RuntimeError` but `publish_event` fails?
   - `self.transport.publish(envelope).await`
   - `InMemoryTransport` shouldn't fail.
   
   **Let's instrument `LLMBasedEngine` to see if it receives the error.**
   
## Action Items
1.  Add logging to `LlmClient` to print the exact error message on failure.
2.  Add logging to `KernelAdapter` to confirm `RuntimeError` conversion.
3.  Add logging to `LLMBasedEngine::step` to confirm input receipt.

