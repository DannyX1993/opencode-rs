# opencode-session/src

Internal runtime modules for session execution.

## Modules

- `engine.rs` — `Session` trait plus `SessionEngine` prompt, detached prompt, cancel, and status implementations.
- `run_state.rs` — in-memory active-run ownership and runtime snapshot helpers.
- `types.rs` — session-facing DTOs such as `SessionHandle`, `DetachedPromptAccepted`, and `SessionRuntimeStatus`.
- `runtime.rs` — provider streaming/tool-loop helpers used by the engine.

## Current notes

- Runtime status is intentionally exposed as `idle|busy`.
- Detached prompt support is a wrapper over the same engine/runtime flow rather than a separate persistence model.
