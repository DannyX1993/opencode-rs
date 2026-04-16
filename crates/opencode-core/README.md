# opencode-core

Lowest-level shared types for the Rust workspace.

## Status

Active. This crate provides foundational code used across the current binary, server, storage, and provider layers.

## Purpose

`opencode-core` centralizes the common pieces that should not depend on higher-level runtime crates:

- cascading JSONC config loading
- typed IDs
- DTOs shared between storage and HTTP layers
- provider/account parity DTOs (account state rows, control-account compatibility, account/org response DTOs)
- workspace error types
- tracing bootstrap helpers
- async context helpers and boxed stream aliases

## Configuration Behavior

`opencode-core` now exposes both low-level config loading and a shared runtime `ConfigService`:

- `Config::load(project_dir)` performs one-shot layered merge (`defaults < global < local < env`).
- `ConfigService::resolve()` does the same merge with an in-memory cache for runtime reuse.
- `ConfigService::read_scope(scope)` returns persisted raw config for `local` or `global` scope.
- `ConfigService::update_scope(scope, payload)` merges and persists only the targeted scope, then invalidates cached resolved config on success.
- `ConfigService::resolve_bind(cli_overrides)` applies CLI host/port overrides on top of resolved config without affecting non-bind fields.

Environment support includes keys such as `OPENCODE_MODEL`, `OPENCODE_LOG_LEVEL`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GOOGLE_API_KEY`, `OPENCODE_SERVER_HOST`, `OPENCODE_SERVER_PORT`, and `OPENCODE_AUTH_TOKEN`.

## Why Other Crates Depend On It

- `opencode-cli` uses config and tracing bootstrap
- `opencode-server` uses shared DTOs and typed errors
- `opencode-storage` uses DTOs and typed IDs for persistence contracts
- `opencode-provider` uses shared streaming and provider/account-domain DTO contracts

## Provider/Auth/Account parity note

`opencode-core` now carries the shared account-state DTO boundary used across storage (`opencode-storage`), domain services (`opencode-provider`), and HTTP routes (`opencode-server`).
This keeps provider/account payloads stable and transport-neutral while preserving legacy `control_account` compatibility.

## Test

```sh
cargo test -p opencode-core
```

## Notes

This crate should stay low-level. Business workflow code belongs in higher-level crates.
