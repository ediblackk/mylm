# Planned Features

## Model Metadata Service

Currently, **we cannot fetch model data dynamically** in the existing codebase.

### Current State
1.  **No Discovery Mechanism**: The `LlmClient` (`core/src/llm/client.rs`) is strictly built for sending prompts and receiving replies. It has no methods to call `/v1/models` or similar endpoints.
2.  **Static Configuration**: The system relies entirely on hardcoded defaults (cost: 0.0, context: 32k) or what you manually provide in `config.toml`.
3.  **Removed Tracking**: The newer `config/v2.rs` explicitly simplified model handling to just "connection strings," removing the old metadata structures.

### How to Fix This (Proposal)
If you want this feature, we need to build a **Model Metadata Service**:

1.  **Extend `LlmClient`**: Add a `fetch_metadata(model_id)` method that queries the provider's API.
    *   *Note: Standard OpenAI-compatible APIs often have a `/v1/models` endpoint we can leverage.*
2.  **Update `AgentFactory`**: When creating an agent, it should:
    *   Check a local cache for model info.
    *   If missing, fetch it from the provider.
    *   Inject the *actual* context window and pricing into the Agent's state.
3.  **Use It**: The Agent can then warn you: "I'm at 90% of my 128k context window" instead of guessing.
