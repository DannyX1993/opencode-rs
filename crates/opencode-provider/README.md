# opencode-provider

Provider abstraction and concrete LLM adapters for the Rust workspace.

## Status

Active. This crate contains real provider implementations and registry logic used by both the manual server harness and the bounded Rust session runtime tool loop.

## What Exists Today

- `LanguageModel` trait
- `ModelRegistry`
- auth resolver helpers
- SSE parsing helpers
- concrete adapters for OpenAI, Anthropic, and Google
- tests that exercise registry behavior, provider streaming paths, and tool replay contracts

## Current Usage In The Workspace

- `opencode-session` uses these adapters for live session prompts.
- Anthropic and Google currently support provider-driven tool execution in that session runtime slice.
- The `opencode` binary also registers providers into a `ModelRegistry` when `OPENCODE_MANUAL_HARNESS=1` is set, and `opencode-server` exposes `POST /api/v1/provider/stream` as a manual validation route.

That harness route is not positioned as a stable public API. It exists to validate real streaming behavior against providers and does not cover the full persisted session loop by itself.

## Supported Providers

| Provider id | Current role |
| --- | --- |
| `openai` | real adapter, text-only in the current session runtime MVP |
| `anthropic` | real adapter, supports bounded session tool execution |
| `google` | real adapter, supports bounded session tool execution |

## Tool replay notes

- Anthropic and Google both receive runtime tool declarations from `ModelRequest.tools`.
- Google/Gemini request building strips `additionalProperties` from function parameter schemas because Gemini rejects that keyword in this path.
- Google/Gemini `thoughtSignature` is replayed on the enclosing `Part`, not nested inside `functionCall`.
- Session history stores tool results under runtime `tool` role; the Google adapter normalizes replay to API-compatible `user` role when emitting `functionResponse` parts.

Those details matter for parity with the live Google wire contract even though the persisted runtime history stays provider-agnostic.

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
