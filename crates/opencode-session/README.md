# opencode-session

Session runtime core for the Rust agent loop.

## Status

Partial. The crate now has a functional bounded session runtime loop: prompt/cancel flow, permission/question interactive runtimes, blocked runtime-status snapshots, detached prompt acceptance, history-based replay, provider-driven built-in tool execution for Anthropic/Google, persisted tool artifacts, lifecycle events, and cancellation-safe run ownership. Full runtime parity is still intentionally out of scope.

## What Exists Today

- `Session` trait with `prompt`, `prompt_detached`, `cancel`, `status`, and `list_statuses`
- `SessionEngine` that:
  - verifies target session existence
  - resolves model/provider from request/default config
  - rebuilds provider requests from persisted session history on each provider pass
  - writes user message + assistant shell into storage
  - streams provider output and appends text deltas as message parts
  - executes supported built-in tools when Anthropic/Google emit tool-use events
  - pauses on permission asks and question asks through dedicated runtime services
  - persists assistant `tool_use` parts plus `tool` role `tool_result` messages for replay
  - publishes lifecycle and token-usage events over `opencode-bus`
- read-only runtime occupancy snapshots backed by `RunState` plus pending permission/question queues
- per-session exclusivity + cancellation support via `RunState`
- HTTP-facing handle metadata (`assistant_message_id`, `resolved_model`) for route responses
- detached acceptance metadata for server parity handlers
- permission runtime semantics:
  - `ask` blocks until a reply and emits `PermissionAsked`
  - `reply: once` resumes one pending request
  - `reply: always` persists normalized durable allow rules and resumes all covered same-session requests
  - `reply: reject` fails all same-session pending permission asks
- question runtime semantics:
  - `ask` blocks until `reply` or `reject`
  - replies preserve answer ordering by question index
  - rejects fail the waiting caller and emit `QuestionRejected`
- runtime-focused tests for streaming, tool loops, cancellation, run-state exclusivity, and error mapping

## Supported scope in this crate

- Anthropic and Google can drive the bounded runtime tool loop.
- Persisted history is the source of truth for replay between provider passes.
- Tool results are stored in a provider-agnostic session shape; provider adapters normalize any wire-specific replay requirements.
- Runtime status is server-facing and includes `idle`, `busy`, and blocked states (`permission` or `question`) with request correlation ids.

## What Does Not Exist Yet

- OpenAI runtime tool-loop parity for session prompts
- task/subagent orchestration or broader TS-parity prompt lifecycle
- broader CLI/TUI integration (`opencode prompt` command remains stubbed in binary runtime)
- durable runtime-status history or SSE replay storage

## Runtime Behavior Snapshot

- `prompt` returns `SessionError::NotFound` if the session row does not exist.
- Concurrent prompt for the same session returns `SessionError::Busy`.
- `cancel` returns `SessionError::NoActiveRun` if no run is active.
- `status` returns `SessionError::NotFound` for unknown sessions and `idle|busy|blocked` for known sessions.
- `list_statuses` is a live snapshot intended for current active/runtime-visible sessions.
- Cancellation keeps already-persisted deltas and emits `SessionCancelled`.
- Anthropic/Google tool-capable turns persist `tool_use` and `tool_result` artifacts before replaying the next provider pass.
- `prompt_detached` returns immediate acceptance metadata and publishes `SessionError` if background execution fails terminally.
- `cancel` also drains pending permission/question requests for that session by rejecting them.
- `PermissionReplyKind::Always` writes durable allow rules into project permission storage using normalized/deduplicated rule merge helpers.
- Out-of-scope providers/tool families fail clearly instead of pretending tool support.

## Workspace Integration

- `opencode-server` exposes this crate through:
  - `POST /api/v1/sessions/:sid/prompt`
  - `POST /api/v1/sessions/:sid/cancel`
  - `GET /api/v1/session/status`
  - `GET /api/v1/session/:sid/status`
  - `POST /api/v1/session/:sid/abort`
  - `POST /api/v1/session/:sid/prompt`
  - `GET /api/v1/permission`
  - `POST /api/v1/permission/reply`
  - `GET /api/v1/question`
  - `POST /api/v1/question/reply`
  - `POST /api/v1/question/reject`
- `opencode/src/lib.rs` wires `SessionEngine` into `AppState` when starting the server.

## Test

```sh
cargo test -p opencode-session
```

For server + runtime integration validation:

```sh
cargo test -p opencode-server -p opencode --lib
```
