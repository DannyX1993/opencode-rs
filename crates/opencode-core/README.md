# opencode-core

Lowest-level shared types for the Rust workspace.

## Status

Active. This crate provides foundational code used across the current binary, server, storage, and provider layers.

## Purpose

`opencode-core` centralizes the common pieces that should not depend on higher-level runtime crates:

- cascading JSONC config loading
- typed IDs
- DTOs shared between storage and HTTP layers
- workspace error types
- tracing bootstrap helpers
- async context helpers and boxed stream aliases

## Configuration Behavior

`Config::load(project_dir)` merges:

1. `~/.config/opencode/config.jsonc`
2. `<project_dir>/.opencode/config.jsonc`
3. environment variables

Environment support includes keys such as `OPENCODE_MODEL`, `OPENCODE_LOG_LEVEL`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GOOGLE_API_KEY`, and `OPENCODE_SERVER_PORT`.

## Why Other Crates Depend On It

- `opencode-cli` uses config and tracing bootstrap
- `opencode-server` uses shared DTOs and typed errors
- `opencode-storage` uses DTOs and typed IDs for persistence contracts
- `opencode-provider` uses shared streaming and error-adjacent types

## Test

```sh
cargo test -p opencode-core
```

## Notes

This crate should stay low-level. Business workflow code belongs in higher-level crates.
