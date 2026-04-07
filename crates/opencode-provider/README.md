# opencode-provider

AI model provider adapters for `opencode-rs`.

> **Status**: ✅ Complete — OpenAI, Anthropic, and Google (Gemini) adapters implemented.

---

## Purpose

`opencode-provider` implements the `ModelProvider` trait for each supported AI backend.
Each provider handles auth, request serialisation, streaming SSE parsing, and error mapping.

---

## Supported Providers

| Provider        | Struct              | Auth env var(s)                      |
| --------------- | ------------------- | ------------------------------------ |
| OpenAI          | `OpenAiProvider`    | `OPENAI_API_KEY`                     |
| Anthropic       | `AnthropicProvider` | `ANTHROPIC_API_KEY`                  |
| Google (Gemini) | `GoogleProvider`    | `GOOGLE_API_KEY` or `GEMINI_API_KEY` |

---

## Usage

```rust
use opencode_provider::{ModelRegistry, OpenAiProvider};
use std::sync::Arc;

let registry = ModelRegistry::new();
let auth = OpenAiProvider::default_auth(None);
registry.register("openai", Arc::new(OpenAiProvider::new(auth))).await;
```

Providers are registered into a `ModelRegistry` and injected into `AppState`.
The HTTP server routes `/api/v1/provider/stream` requests through the registry.

---

## Module Map

```
src/
├── lib.rs       — re-exports
├── openai.rs    — OpenAI chat completions + streaming
├── anthropic.rs — Anthropic messages API + streaming
├── google.rs    — Google AI Studio (Gemini) + streaming
├── registry.rs  — ModelRegistry
└── types.rs     — ModelRequest, ModelEvent, ProviderError
```
