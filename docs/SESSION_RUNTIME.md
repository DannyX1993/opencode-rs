# Session Runtime Tool Loop (`expand-tool-runtime-parity`)

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

### 3) History-backed provider/tool loop

During `prompt` execution:

1. Session existence is validated via storage.
2. User message + assistant shell message are persisted.
3. The full session history is rebuilt into a provider request.
4. For Anthropic and Google, runtime tool definitions are attached to that request.
5. Provider stream is consumed:
   - `TextDelta` → appended as `PartRow` to assistant message
   - `ToolUse*` → accumulated into one persisted assistant `tool_use` part
   - `Usage` → `ProviderTokensUsed` bus event
   - `Done`/stream end → `SessionCompleted` bus event
6. When a supported tool call completes:
   - `ToolStarted` / `ToolFinished { ok }` bus events are published
   - a dedicated `tool` role message with `tool_result` part is persisted
   - the next provider pass is rebuilt from storage history and the same turn continues
7. Cancellation emits `SessionCancelled` and returns `SessionError::Cancelled`.

### 4) Supported providers in this slice

- `anthropic`: supported for provider-driven tool execution
- `google`: supported for provider-driven tool execution
- `openai`: available as a provider adapter, but still out of scope for session runtime tool execution

### 5) Persisted replay model

High level persisted shapes:

- assistant tool request part: `{ "type": "tool_use", ... }`
- tool result part: `{ "type": "tool_result", ... }`

Replay is storage-first. The next provider pass is rebuilt from persisted session history instead of transient in-memory tool state.

That matters for cancellation/error safety and for provider-specific replay fixes such as Google `thoughtSignature` and `functionResponse` normalization.

## HTTP integration

`opencode-server` exposes runtime entrypoints:

- `POST /api/v1/sessions/:sid/prompt`
- `POST /api/v1/sessions/:sid/cancel`

Prompt response includes runtime metadata (`assistant_message_id`, `resolved_model`) when available.

Manual validation path:

1. `PUT /api/v1/projects/:id`
2. `POST /api/v1/projects/:pid/sessions`
3. `POST /api/v1/sessions/:sid/prompt`
4. `GET /api/v1/sessions/:sid/messages`

## Deferred/non-goals in this slice

- Permission/question interactive loops are deferred.
- Full CLI parity (`opencode prompt <text>`) is deferred.
- TUI/session UX and richer event APIs remain future slices.
- OpenAI tool execution parity is deferred.
- Broader TypeScript tool-catalog/runtime parity is deferred.

## Error model (current)

Common returned errors:

- `NotFound` for missing session
- `Busy` for concurrent prompt on same session
- `NoActiveRun` for cancel without active run
- `Cancelled` when prompt run was cancelled
- `Provider` / `RuntimeInternal` for provider/runtime failures

Out-of-scope provider/tool paths fail clearly rather than silently downgrading into fake tool support.

## Relevant code locations

- `crates/opencode-session/src/engine.rs`
- `crates/opencode-session/src/run_state.rs`
- `crates/opencode-session/src/runtime.rs`
- `crates/opencode-session/src/types.rs`
- `crates/opencode-server/src/routes/session.rs`
- `crates/opencode-server/src/router.rs`
- `opencode/src/lib.rs`
