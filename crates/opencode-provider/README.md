# opencode-provider

Provider abstraction and concrete LLM adapters for the Rust workspace.

## Status

Active. This crate contains real provider implementations and registry logic used by the manual server harness today.

## What Exists Today

- `LanguageModel` trait
- `ModelRegistry`
- auth resolver helpers
- SSE parsing helpers
- concrete adapters for OpenAI, Anthropic, and Google
- tests that exercise registry behavior and provider streaming paths

## Current Usage In The Workspace

The `opencode` binary registers providers into a `ModelRegistry` when `OPENCODE_MANUAL_HARNESS=1` is set, and `opencode-server` exposes `POST /api/v1/provider/stream` as a manual validation route.

That route is not positioned as a stable public API. It exists to validate real streaming behavior against providers.

## Supported Providers

| Provider id | Current role |
| --- | --- |
| `openai` | real adapter |
| `anthropic` | real adapter |
| `google` | real adapter |

## Configuration

Auth may come from config or environment depending on the provider path in use. Common environment variables include:

- `OPENAI_API_KEY`
- `ANTHROPIC_API_KEY`
- `GOOGLE_API_KEY`
- `GEMINI_API_KEY`

## Test

```sh
cargo test -p opencode-provider
```

## Workspace Role

This crate is the provider-facing edge of the Rust workspace. The full agent loop is still incomplete, but provider streaming and registry behavior are already implemented here.
