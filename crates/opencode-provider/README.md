# opencode-provider

Provider abstraction, domain services, and concrete LLM adapters for the Rust workspace.

## Status

Active. This crate now has two responsibilities: runtime model adapters and provider/auth/account parity domain services.

## What Exists Today

- Runtime layer:
  - `LanguageModel` trait
  - `ModelRegistry`
  - SSE parsing helpers
  - concrete adapters for OpenAI, Anthropic, and Google
- Provider parity domain layer:
  - `ProviderCatalogService` (`/api/v1/provider`, `/api/v1/config/providers` data contracts)
  - `ProviderAuthService` (auth method discovery + authorize/callback handshake)
  - `AccountService` (persist/list/use/remove provider accounts via storage)
- tests covering registry behavior, provider streaming paths, catalog filtering/default selection, auth method mapping, OAuth lifecycle, and account-state invariants

## Current Usage In The Workspace

- `opencode-session` uses runtime adapters for live model prompts.
- `opencode-server` uses domain services for public provider/auth/account/config routes.
- `opencode` startup seeds catalog metadata from cached `.opencode/models.json` when available.
- `ModelRegistry` registration in `opencode` remains tied to `OPENCODE_MANUAL_HARNESS=1` for raw stream checks.

## Supported built-in providers

| Provider id | Current role |
| --- | --- |
| `openai` | real adapter, text-only in current session runtime MVP; includes API-key + OAuth method metadata |
| `anthropic` | real adapter, supports bounded session tool execution |
| `google` | real adapter, supports bounded session tool execution |

## Public/manual route contract alignment

- Public routes (via `opencode-server`) consume this crate's domain services:
  - `GET /api/v1/provider`
  - `GET /api/v1/provider/auth`
  - `POST /api/v1/provider/:provider/oauth/authorize`
  - `POST /api/v1/provider/:provider/oauth/callback`
  - `GET /api/v1/provider/account`
  - `POST /api/v1/provider/account/use`
  - `DELETE /api/v1/provider/account/:account_id`
  - `GET /api/v1/config/providers`
- Manual-only route remains separate: `POST /api/v1/provider/stream`.

## Persistence behavior

- Account data persists through `opencode-storage` (`account`, `account_state`; `id=1` singleton active state).
- Callback success is observable by re-reading account state endpoints.
- Active-account updates are validated against persisted account/org data.
- Account removal relies on storage cleanup to prevent dangling active references.

## Tool replay notes

- Anthropic and Google both receive runtime tool declarations from `ModelRequest.tools`.
- Google/Gemini request building strips `additionalProperties` from function parameter schemas because Gemini rejects that keyword in this path.
- Google/Gemini `thoughtSignature` is replayed on the enclosing `Part`, not nested inside `functionCall`.
- Session history stores tool results under runtime `tool` role; the Google adapter normalizes replay to API-compatible `user` role when emitting `functionResponse` parts.

Those details matter for parity with the live Google wire contract even though persisted runtime history stays provider-agnostic.

## Current limitations

- OAuth pending state is in-memory only (restart during flow requires a new authorize call).
- OpenAI is not tool-capable in the current session runtime path.
- Catalog startup currently prefers cache overlay and does not force network refresh at boot.

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
