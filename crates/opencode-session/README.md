# opencode-session

Session runtime core for the Rust agent loop.

## Status

Partial. The crate now has a functional runtime core (`prompt` + `cancel` flow, run-state coordination, provider stream consumption, and incremental assistant-part persistence), but not yet full tool/runtime parity.

## What Exists Today

- `Session` trait with `prompt` and `cancel`
- `SessionEngine` that:
  - verifies target session existence
  - resolves model/provider from request/default config
  - writes user message + assistant shell into storage
  - streams provider output and appends text deltas as message parts
  - publishes lifecycle and token-usage events over `opencode-bus`
- per-session exclusivity + cancellation support via `RunState`
- HTTP-facing handle metadata (`assistant_message_id`, `resolved_model`) for route responses
- runtime-focused tests for streaming, cancellation, run-state exclusivity, and error mapping

## What Does Not Exist Yet

- tool-use event execution loop (`ToolUse*` provider stream events are deferred)
- full TS-parity prompt lifecycle (permissions, interactive questions, richer run-state/reporting)
- broader CLI/TUI integration (`opencode prompt` command remains stubbed in binary runtime)

## Runtime Behavior Snapshot

- `prompt` returns `SessionError::NotFound` if the session row does not exist.
- Concurrent prompt for the same session returns `SessionError::Busy`.
- `cancel` returns `SessionError::NoActiveRun` if no run is active.
- Cancellation keeps already-persisted deltas and emits `SessionCancelled`.
- Unsupported tool-use stream events currently return a runtime-internal error by design (deferred scope).

## Workspace Integration

- `opencode-server` exposes this crate through:
  - `POST /api/v1/sessions/:sid/prompt`
  - `POST /api/v1/sessions/:sid/cancel`
- `opencode/src/lib.rs` wires `SessionEngine` into `AppState` when starting the server.

## Test

```sh
cargo test -p opencode-session
```

For server + runtime integration validation:

```sh
cargo test -p opencode-server -p opencode --lib
```
