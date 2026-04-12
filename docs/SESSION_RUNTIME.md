# Session Runtime Core (`port-session-runtime-core`)

This document describes the **current Rust implementation** of session runtime behavior in `opencode-rs`.

It is intentionally scoped to what has landed in this repository and should not be read as a full parity claim with `opencode-ts`.

## Scope implemented so far

### 1) Prompt lifecycle entry and cancellation

- `opencode-session::engine::Session` defines:
  - `prompt(SessionPrompt) -> Result<SessionHandle, SessionError>`
  - `cancel(SessionId) -> Result<(), SessionError>`
- `SessionEngine` is wired into server state and called by HTTP routes.

### 2) Per-session run-state coordination

- `RunState` enforces one active run per session (`Busy` on concurrent prompt).
- Active runs carry a cancellation token via `RunGuard`.
- `cancel(session_id)` is idempotent from API perspective and reports `NoActiveRun` when no run exists.

### 3) Stream projection to storage/events (runtime core slice)

During `prompt` execution:

1. Session existence is validated via storage.
2. User message + assistant shell message are persisted.
3. Provider stream is consumed:
   - `TextDelta` → appended as `PartRow` to assistant message
   - `Usage` → `ProviderTokensUsed` bus event
   - `Done`/stream end → `SessionCompleted` bus event
4. Cancellation emits `SessionCancelled` and returns `SessionError::Cancelled`.

## HTTP integration

`opencode-server` exposes runtime entrypoints:

- `POST /api/v1/sessions/:sid/prompt`
- `POST /api/v1/sessions/:sid/cancel`

Prompt response includes runtime metadata (`assistant_message_id`, `resolved_model`) when available.

## Deferred/non-goals in this slice

- Tool-use provider stream events (`ToolUseStart`, `ToolUseInputDelta`, `ToolUseEnd`) are deferred.
- Permission/question interactive loops are deferred.
- Full CLI parity (`opencode prompt <text>`) is deferred.
- TUI/session UX and richer event APIs remain future slices.

## Error model (current)

Common returned errors:

- `NotFound` for missing session
- `Busy` for concurrent prompt on same session
- `NoActiveRun` for cancel without active run
- `Cancelled` when prompt run was cancelled
- `Provider` / `RuntimeInternal` for provider/runtime failures

## Relevant code locations

- `crates/opencode-session/src/engine.rs`
- `crates/opencode-session/src/run_state.rs`
- `crates/opencode-session/src/runtime.rs`
- `crates/opencode-session/src/types.rs`
- `crates/opencode-server/src/routes/session.rs`
- `crates/opencode-server/src/router.rs`
- `opencode/src/lib.rs`
