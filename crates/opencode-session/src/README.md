# opencode-session/src

Internal runtime modules for session execution.

## Modules

- `engine.rs` — `Session` trait plus `SessionEngine` prompt, detached prompt, cancel, and status implementations.
- `permission_runtime.rs` — in-memory permission ask/reply service with durable `always` rule persistence.
- `question_runtime.rs` — in-memory question ask/reply/reject service.
- `run_state.rs` — in-memory active-run ownership and runtime snapshot helpers.
- `types.rs` — session-facing DTOs such as `SessionHandle`, runtime request/reply payloads, and blocked-aware `SessionRuntimeStatus`.
- `runtime.rs` — provider streaming/tool-loop helpers used by the engine.

## Current notes

- Runtime status now supports `idle`, `busy`, and blocked objects (`permission`/`question` + `requestID`).
- Detached prompt support is a wrapper over the same engine/runtime flow rather than a separate persistence model.
- Permission `always` replies persist normalized allow rules in `opencode-storage` and can auto-unblock covered requests.
