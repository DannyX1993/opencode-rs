# opencode-session

Session runtime core for the Rust agent loop.

## Status

Partial. The crate now has a functional bounded session runtime loop: prompt/cancel flow, detached prompt acceptance, runtime-status snapshots, history-based replay, provider-driven built-in tool execution for Anthropic/Google, persisted tool artifacts, lifecycle events, and cancellation-safe run ownership. Full runtime parity is still intentionally out of scope.

## What Exists Today

- `Session` trait with `prompt`, `prompt_detached`, `cancel`, `status`, and `list_statuses`
- `SessionEngine` that:
  - verifies target session existence
  - resolves model/provider from request/default config
  - rebuilds provider requests from persisted session history on each provider pass
  - writes user message + assistant shell into storage
  - streams provider output and appends text deltas as message parts
  - executes supported built-in tools when Anthropic/Google emit tool-use events
  - persists assistant `tool_use` parts plus `tool` role `tool_result` messages for replay
  - publishes lifecycle and token-usage events over `opencode-bus`
- read-only runtime occupancy snapshots backed by `RunState` rather than durable tables
- per-session exclusivity + cancellation support via `RunState`
- HTTP-facing handle metadata (`assistant_message_id`, `resolved_model`) for route responses
- detached acceptance metadata for server parity handlers
- runtime-focused tests for streaming, tool loops, cancellation, run-state exclusivity, and error mapping

## Supported scope in this crate

- Anthropic and Google can drive the bounded runtime tool loop.
- Persisted history is the source of truth for replay between provider passes.
- Tool results are stored in a provider-agnostic session shape; provider adapters normalize any wire-specific replay requirements.
- Runtime status remains intentionally small and server-facing: `idle|busy`.

## What Does Not Exist Yet

- OpenAI runtime tool-loop parity for session prompts
- permission/approval flows, interactive questions, task/subagent orchestration, or broader TS-parity prompt lifecycle
- broader CLI/TUI integration (`opencode prompt` command remains stubbed in binary runtime)
- durable runtime-status history or SSE replay storage

## Runtime Behavior Snapshot

- `prompt` returns `SessionError::NotFound` if the session row does not exist.
- Concurrent prompt for the same session returns `SessionError::Busy`.
- `cancel` returns `SessionError::NoActiveRun` if no run is active.
- `status` returns `SessionError::NotFound` for unknown sessions and `idle|busy` for known sessions.
- `list_statuses` is a live snapshot intended for current active/runtime-visible sessions.
- Cancellation keeps already-persisted deltas and emits `SessionCancelled`.
- Anthropic/Google tool-capable turns persist `tool_use` and `tool_result` artifacts before replaying the next provider pass.
- `prompt_detached` returns immediate acceptance metadata and publishes `SessionError` if background execution fails terminally.
- Out-of-scope providers/tool families fail clearly instead of pretending tool support.

## Workspace Integration

- `opencode-server` exposes this crate through:
  - `POST /api/v1/sessions/:sid/prompt`
  - `POST /api/v1/sessions/:sid/cancel`
  - `GET /api/v1/session/status`
  - `GET /api/v1/session/:sid/status`
  - `POST /api/v1/session/:sid/abort`
  - `POST /api/v1/session/:sid/prompt`
- `opencode/src/lib.rs` wires `SessionEngine` into `AppState` when starting the server.

## Test

```sh
cargo test -p opencode-session
```

For server + runtime integration validation:

```sh
cargo test -p opencode-server -p opencode --lib
```
