# opencode-session

Session abstraction for the future Rust agent loop.

## Status

Stub. The crate defines the `Session` trait, request/handle types, and a `SessionEngine` that currently returns `SessionError::NotFound` for operations.

## What Exists Today

- `Session` trait with `prompt` and `cancel`
- typed request/handle structures in `types.rs`
- `SessionEngine` stub implementation
- basic tests confirming the stub behavior

## What Does Not Exist Yet

- real prompt orchestration
- tool execution loop
- provider streaming integration through a live session engine
- persistence and event fan-out coordinated by the engine itself

## Why The Crate Still Matters

Other crates can depend on the session abstraction now without waiting for the full implementation. `opencode-server` already uses `Arc<dyn Session>` in `AppState`.

## Test

```sh
cargo test -p opencode-session
```
